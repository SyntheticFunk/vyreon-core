"""OCC cleared option volume against the engine's VS-O06 (rules in README.md, declared first).

    python3 -I occ.py fetch CACHE DATE...                # OCC's all-underlying CSV per date
    python3 -I occ.py compare CACHE VALUES.tsv > report.json

CACHE holds OCC's responses as downloaded (`volume-<YYYYMMDD>.csv`) and `fetched.json`: url, file,
sha256 and UTC fetch time of each. VALUES.tsv: as `occ.sql` prints it from core's published store
(`psql -A -F $'\\t'`). Exit 0 on `pass` (the gate while the adjusted-root fix is pending; README).
"""
from __future__ import annotations

import csv
import hashlib
import json
import re
import sys
import time
import urllib.request
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

URL = ("https://marketdata.theocc.com/volume-query?reportDate={}&format=csv&volumeQueryType=O"
       "&symbolType=ALL&symbol=&reportType=D&accountType=ALL&productKind=ALL&porc=BOTH")
PAUSE = 1.0
AGENT = "Mozilla/5.0 (compatible; data-check/1.0)"  # generic; OCC refuses urllib's default
TIERS = [  # (minimum OCC contracts, volume band (lo, hi) on ours/OCC - 1, put-share tolerance)
    (10_000, (-0.02, 0.005), 0.005),
    (1_000, (-0.05, 0.01), 0.02),
]
# the research pilot (2026-11-18) compared these security-days
SEEN = {("2026-11-12", s) for s in ("SPY", "NVDA", "IWM", "TSLA", "AAPL", "GLD", "MSTR", "EWZ", "XLU", "UUP")}


def fetch(cache: str, dates: list[str]) -> int:
    out = Path(cache)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "fetched.json"
    log = json.loads(log_path.read_text()) if log_path.is_file() else []
    for d in dates:
        ymd = d.replace("-", "")
        url = URL.format(ymd)
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": AGENT}), timeout=120) as r:
            body = r.read()
        if not body.startswith(b"quantity,underlying,symbol,actype,porc,exchange,actdate"):
            raise SystemExit(f"{d}: OCC answered without the volume header")
        name = f"volume-{ymd}.csv"
        (out / name).write_bytes(body)
        log.append({"url": url, "file": name, "sha256": hashlib.sha256(body).hexdigest(),
                    "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")})
        log_path.write_text(json.dumps(log, indent=1) + "\n")
        time.sleep(PAUSE)
    return 0


def root_kind(symbol: str, ticker: str) -> str:
    """Information only (found after the first comparison; verdicts do not use it): a leading
    1-4 is a FLEX root (Cboe and NYSE FLEX symbology: 1/2 American/European physical, 3/4 cash);
    a trailing digit an adjusted (corporate-action) root; anything else is unidentified."""
    if re.fullmatch(r"[1-4]" + re.escape(ticker) + r"\d?", symbol):
        return "FLEX" if symbol[1:] == ticker else "FLEX_ADJUSTED"
    if re.fullmatch(re.escape(ticker) + r"\d", symbol):
        return "ADJUSTED"
    return "UNIDENTIFIED"


def load(cache: Path, d: str) -> tuple[dict, list[dict]]:
    """underlying -> {symbol -> {'C': sum, 'P': sum}} for date d, and parity anomalies."""
    ymd = d.replace("-", "")
    by = defaultdict(lambda: defaultdict(lambda: {"C": 0, "P": 0}))
    want = datetime.strptime(d, "%Y-%m-%d").strftime("%m/%d/%Y")
    with open(cache / f"volume-{ymd}.csv", newline="") as fh:
        for r in csv.DictReader(fh):
            if not r.get("quantity"):
                continue
            if r["actdate"] != want:
                raise SystemExit(f"{d}: OCC row dated {r['actdate']}")
            by[r["underlying"]][r["symbol"]][r["porc"]] += int(r["quantity"])
    odd = [{"underlying": u, "symbol": s, "side": k, "sum": v}
           for u, syms in by.items() for s, cp in syms.items() for k, v in cp.items() if v % 2]
    return by, odd


def tier(total: float):
    for floor, band, tol in TIERS:
        if total >= floor:
            return floor, band, tol
    return None


def judge(ours_v: float, ours_p: float | None, calls: float, puts: float) -> dict:
    total = calls + puts
    out = {"occ_contracts": total, "occ_put_share": round(puts / total, 6) if total else None}
    t = tier(total)
    out["rel"] = round(ours_v / total - 1, 6) if total else None
    if ours_p is not None and total:
        out["put_share_diff"] = round(ours_p - puts / total, 6)
    if t is None:
        out["judged"] = False
        return out
    floor, (lo, hi), tol = t
    vol_ok = lo <= ours_v / total - 1 <= hi
    share_ok = ours_p is not None and abs(ours_p - puts / total) <= tol
    out.update(judged=True, tier=floor, volume_band=[lo, hi], put_share_tol=tol,
               volume_ok=vol_ok, put_share_ok=share_ok, inside=vol_ok and share_ok)
    return out


def compare(cache: str, values: str) -> int:
    cdir = Path(cache)
    files = json.loads((cdir / "fetched.json").read_text())
    for f in files:
        if hashlib.sha256((cdir / f["file"]).read_bytes()).hexdigest() != f["sha256"]:
            raise SystemExit(f"{f['file']}: cache differs from fetched.json")
    rows = list(csv.DictReader(open(values, newline=""), delimiter="\t"))
    dates = sorted({r["date"] for r in rows})
    occ, anomalies = {}, []
    for d in dates:
        occ[d], odd = load(cdir, d)
        anomalies += [dict(a, date=d) for a in odd]
    items, counts, by_kind = [], defaultdict(int), defaultdict(float)
    for r in rows:
        d, obj = r["date"], r["object_id"]
        it = {"date": d, "object_id": obj, "engine_build": r["engine_build"], "seen": (d, obj) in SEEN}
        syms = occ[d].get(obj)
        if r["volume_state"] != "SUPPORTED" or not r["volume"]:
            it.update(verdict="NOT_COMPARABLE", reason=f"volume {r['volume_state']} {r['volume_reason']}".strip())
        elif not syms:
            it.update(verdict="NOT_COMPARABLE", reason="OCC has no rows for the underlying")
        else:
            ours_v = float(r["volume"])
            ours_p = float(r["put_share"]) if r["put_share_state"] == "SUPPORTED" and r["put_share"] else None
            if ours_p is None:
                it["put_share_state"] = f"{r['put_share_state']} {r['put_share_reason']}".strip()
            std = syms.get(obj, {"C": 0, "P": 0})
            adj = {s: cp for s, cp in syms.items() if s != obj}
            sc, sp = std["C"] / 2, std["P"] / 2
            ac = sum(cp["C"] for cp in adj.values()) / 2
            ap = sum(cp["P"] for cp in adj.values()) / 2
            it.update(ours_volume=ours_v, ours_put_share=ours_p)
            it["standard"] = judge(ours_v, ours_p, sc, sp)
            if ac + ap:
                it["adjusted_roots"] = {s: {"calls": cp["C"] / 2, "puts": cp["P"] / 2, "kind": root_kind(s, obj)}
                                        for s, cp in sorted(adj.items())}
                for s_, cp in adj.items():
                    by_kind[root_kind(s_, obj)] += (cp["C"] + cp["P"]) / 2
                it["adjusted_contracts"] = ac + ap
                it["adjusted_share_of_total"] = round((ac + ap) / (sc + sp + ac + ap), 6)
                it["all_roots"] = judge(ours_v, ours_p, sc + ac, sp + ap)
                a, s = it["all_roots"], it["standard"]
                if not a["judged"] and not s["judged"]:
                    it["verdict"] = "NOT_JUDGED"
                elif a.get("inside"):
                    it["verdict"] = "PASS_ALL_ROOTS"
                elif s.get("inside"):
                    it["verdict"] = "ADJUSTED_MISSING"
                else:
                    it["verdict"] = "FAIL"
            else:
                s = it["standard"]
                it["verdict"] = "NOT_JUDGED" if not s["judged"] else ("PASS" if s["inside"] else "FAIL")
        counts[(it["verdict"], it["seen"])] += 1
        items.append(it)
    fails = [i for i in items if i["verdict"] == "FAIL"]
    adj_missing = [i for i in items if i["verdict"] == "ADJUSTED_MISSING"]
    summary = {
        "security_days": len(items),
        "verdicts": {v: {"seen": counts[(v, True)], "blind": counts[(v, False)]}
                     for v in sorted({k[0] for k in counts})},
        "fail": [{k: i.get(k) for k in ("date", "object_id", "seen", "ours_volume", "ours_put_share",
                                        "standard", "all_roots")} for i in fails],
        "adjusted_missing": [{"date": i["date"], "object_id": i["object_id"], "seen": i["seen"],
                              "adjusted_contracts": i["adjusted_contracts"],
                              "adjusted_share_of_total": i["adjusted_share_of_total"],
                              "all_roots_rel": i["all_roots"]["rel"],
                              "all_roots_put_share_diff": i["all_roots"].get("put_share_diff")}
                             for i in adj_missing],
        "standard_rel_range_judged": [
            min((i["standard"]["rel"] for i in items if i.get("standard", {}).get("judged")), default=None),
            max((i["standard"]["rel"] for i in items if i.get("standard", {}).get("judged")), default=None)],
        "parity_anomalies": len(anomalies),
        "nonstandard_contracts_by_root_kind": dict(by_kind),
        "standard_total_ours": sum(i.get("ours_volume") or 0 for i in items),
        "standard_total_occ": sum(i["standard"]["occ_contracts"] for i in items if "standard" in i),
    }
    report = {
        "check": "OCC cleared option volume against VS-O06 (tools/external/README.md)",
        "occ_terms": "UNVERIFIED: the owner must read OCC's terms in a browser before this check is relied on",
        "dates": dates, "occ_files": files, "tiers": TIERS, "summary": summary,
        "pass": not fails, "pass_all_roots": not fails and not adj_missing,
        "pass_blind": not any(not i["seen"] for i in fails),
        "parity_anomalies": anomalies[:200], "items": items,
    }
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "fetch":
        sys.exit(fetch(a[1], a[2:]))
    sys.exit(compare(a[1], a[2]))
