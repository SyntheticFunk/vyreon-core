"""FRED/ALFRED vintages against the engine's CDD-MACRO01 (rules in README.md, declared first).

    python3 -I fred.py fetch CACHE BODIES.tsv            # the ALFRED vintages the bodies need
    python3 -I fred.py compare CACHE BODIES.tsv > report.json

CACHE holds ALFRED's CSVs as downloaded (`alfred-<SERIES>-<DATE>.csv`) and `fetched.json`: url,
file, sha256 and UTC fetch time of each. BODIES.tsv: date, object_id, engine_build, body as
`fred.sql` prints them (`psql -A -t -F $'\\t'`). Exit 0 on pass.
"""
from __future__ import annotations

import csv
import hashlib
import io
import json
import sys
import time
import urllib.request
from collections import defaultdict
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

SERIES = ("CPIAUCNS", "FEDFUNDS", "INDPRO")
REVISED = {"INDPRO"}  # the body states its revised vintage (engine D-305)
BASIS = "PROVIDER_CURRENT_REVISED_VINTAGE_AS_CAPTURED_NOT_FIRST_PRINT"
URL = "https://alfred.stlouisfed.org/graph/alfredgraph.csv?id={}&vintage_date={}"
PAUSE = 1.0
HISTORY = 24
NY = ZoneInfo("America/New_York")
SEEN = {"2026-11-12"}  # the research pilot


def bodies(path: str) -> list[tuple[str, str, str, dict]]:
    out = []
    for line in open(path):
        line = line.rstrip("\n")
        if not line:
            continue
        d, obj, build, body = line.split("\t", 3)
        roles = {r["role_id"]: r for r in json.loads(body)["measurement"]["value"]["roles"]}
        out.append((d, obj, build, roles))
    return out


def prev_day(d: str) -> str:
    return (date.fromisoformat(d) - timedelta(days=1)).isoformat()


def capture_date(roles: dict) -> str | None:
    r = roles.get("INDPRO.vintage_captured_at")
    if not r or r["state"] != "SUPPORTED" or not r["value"]:
        return None
    at = datetime.fromisoformat(r["value"].replace("Z", "+00:00"))
    return at.astimezone(NY).date().isoformat()


def needed(rows) -> set[tuple[str, str]]:
    want = set()
    for d, _, _, roles in rows:
        for s in SERIES:
            want.add((s, d)); want.add((s, prev_day(d)))
        c = capture_date(roles)
        if c:
            want.add(("INDPRO", c))
    return want


def fetch(cache: str, path: str) -> int:
    out = Path(cache)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "fetched.json"
    log = json.loads(log_path.read_text()) if log_path.is_file() else []
    have = {f["file"] for f in log}
    for s, d in sorted(needed(bodies(path))):
        name = f"alfred-{s}-{d}.csv"
        if name in have:
            continue
        url = URL.format(s, d)
        with urllib.request.urlopen(url, timeout=60) as r:
            body = r.read()
        if not body.startswith(b"observation_date,"):
            raise SystemExit(f"{name}: ALFRED answered without the CSV header")
        (out / name).write_bytes(body)
        log.append({"url": url, "file": name, "sha256": hashlib.sha256(body).hexdigest(),
                    "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")})
        log_path.write_text(json.dumps(log, indent=1) + "\n")
        time.sleep(PAUSE)
    return 0


class Vintages:
    def __init__(self, cache: Path):
        self.cache = cache
        self.files = json.loads((cache / "fetched.json").read_text())
        for f in self.files:
            if hashlib.sha256((cache / f["file"]).read_bytes()).hexdigest() != f["sha256"]:
                raise SystemExit(f"{f['file']}: cache differs from fetched.json")
        self.memo = {}

    def get(self, s: str, d: str) -> list[tuple[str, float]]:
        """Non-missing observations (date, value) of series s as it stood on d."""
        if (s, d) not in self.memo:
            text = (self.cache / f"alfred-{s}-{d}.csv").read_text()
            rows = list(csv.reader(io.StringIO(text)))[1:]
            self.memo[(s, d)] = [(r[0], float(r[1])) for r in rows if len(r) > 1 and r[1] not in ("", ".")]
        return self.memo[(s, d)]


def history_diff(ours: list[dict], ref: list[tuple[str, float]], upto: str) -> list[dict]:
    want = [x for x in ref if x[0] <= upto][-HISTORY:]
    got = [(c["observation_date"], c["value"]) for c in ours]
    diffs = []
    wd, gd = dict(want), dict(got)
    for k in sorted(set(wd) | set(gd)):
        if k not in gd:
            diffs.append({"observation_date": k, "ours": None, "alfred": wd[k]})
        elif k not in wd:
            diffs.append({"observation_date": k, "ours": gd[k], "alfred": None})
        elif gd[k] != wd[k]:
            diffs.append({"observation_date": k, "ours": gd[k], "alfred": wd[k]})
    if len(got) != len(gd):
        diffs.append({"duplicate_dates_in_ours": len(got) - len(gd)})
    return diffs


def check_series(v: Vintages, d: str, s: str, roles: dict) -> dict:
    val, obs, hist = roles.get(f"{s}.value"), roles.get(f"{s}.observation_date"), roles.get(f"{s}.history")
    if not val or val["state"] != "SUPPORTED" or not obs or obs["state"] != "SUPPORTED":
        r = val or obs or {}
        return {"verdict": "NOT_COMPARABLE", "reason": f"{r.get('state')} {r.get('reason')}"}
    ours_obs, ours_val = obs["value"], val["value"]
    vt, vp = v.get(s, d), v.get(s, prev_day(d))
    latest_t, latest_p = vt[-1][0], vp[-1][0]
    if ours_obs == latest_t:
        clock, ref_day = "MATCH", d
    elif ours_obs > latest_t:
        clock, ref_day = "LOOKAHEAD", d
    elif ours_obs == latest_p:
        clock, ref_day = "RELEASE_DAY", prev_day(d)
    else:
        clock, ref_day = "LAG", d
    out = {"clock": clock, "ours_observation_date": ours_obs, "alfred_latest_on_T": latest_t,
           "alfred_latest_on_T_minus_1": latest_p}
    if s in REVISED:
        basis = roles.get(f"{s}.vintage_basis", {}).get("value")
        cap = capture_date(roles)
        if basis != BASIS or not cap:
            out.update(verdict="FAIL", reason=f"INDPRO vintage basis {basis!r} / capture {cap!r} not as D-305 states")
            return out
        ref_day = cap
    ref = v.get(s, ref_day)
    out["reference_vintage"] = ref_day
    rv = dict(ref).get(ours_obs)
    out.update(ours_value=ours_val, alfred_value=rv, value_equal=(rv == ours_val))
    if not hist or hist["state"] != "SUPPORTED":
        out["history"] = f"NOT_COMPARABLE {hist and hist['state']} {hist and hist.get('reason')}"
        hdiff = []
    else:
        hdiff = history_diff(hist["value"], ref, ours_obs)
        out["history_cells"] = len(hist["value"])
        out["history_diffs"] = hdiff
    if s in REVISED and hist and hist["state"] == "SUPPORTED":
        rt = dict(v.get(s, d))
        out["revised_after_T"] = [{"observation_date": c["observation_date"], "ours": c["value"],
                                   "alfred_on_T": rt.get(c["observation_date"])}
                                  for c in hist["value"] if rt.get(c["observation_date"]) != c["value"]]
    failed = clock == "LOOKAHEAD" or not out["value_equal"] or bool(hdiff)
    out["verdict"] = "FAIL" if failed else ("PASS" if clock in ("MATCH", "RELEASE_DAY") else "PASS_LAG")
    return out


def compare(cache: str, path: str) -> int:
    v = Vintages(Path(cache))
    rows = bodies(path)
    groups = defaultdict(list)  # (date, series, result json) -> objects
    builds = {}
    for d, obj, build, roles in rows:
        builds[d] = build
        for s in SERIES:
            res = check_series(v, d, s, roles)
            groups[(d, s, json.dumps(res, sort_keys=True))].append(obj)
    results, counts, ok, ok_blind = [], defaultdict(int), True, True
    for (d, s, res), objs in sorted(groups.items()):
        r = json.loads(res)
        r.update(date=d, series=s, engine_build=builds[d], seen=d in SEEN, bodies=len(objs),
                 objects=objs if len(objs) < 64 else "all 64")
        counts[(s, r["verdict"], r["seen"])] += len(objs)
        if r["verdict"] == "FAIL":
            ok = False
            ok_blind &= r["seen"]
        results.append(r)
    report = {
        "check": "FRED/ALFRED vintages against CDD-MACRO01 (tools/external/README.md)",
        "tolerance": "exact (equal IEEE-754 doubles of the published decimals)",
        "dates": sorted(builds), "builds": builds, "alfred_files": v.files,
        "summary": {f"{s} {verdict} {'seen' if seen else 'blind'}": n for (s, verdict, seen), n in sorted(counts.items())},
        "pass": ok, "pass_blind": ok_blind, "results": results,
    }
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if ok else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    sys.exit(fetch(a[1], a[2]) if a[0] == "fetch" else compare(a[1], a[2]))
