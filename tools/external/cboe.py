"""Cboe volatility indices against the engine's VS-IVS01 iv30 (bands in README.md, declared first).

    python3 -I cboe.py fetch CACHE                       # the nine more indices into CACHE
    python3 -I cboe.py VALUES.tsv [CACHE] [--band-only] > report.json   (VALUES: psql -X -A -P footer=off ...)

VALUES.tsv: one row per (date, object) with columns date, object_id, state, iv30, reason, as
`values.sql` prints them from core's published store (`psql -A -P footer=off -F $'\t'`). The five original pairs
read the committed files in `cboe/`; with CACHE the nine more pairs are added, read from CACHE and
checked against the committed `cboe/fetched.json`. `--band-only` reports band 1 alone (dates of
mixed builds, not a correlation panel). Exit 0 on pass.
"""
from __future__ import annotations

import csv
import hashlib
import json
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent / "cboe"
PAIRS = {"SPY": "VIX", "QQQ": "VXN", "IWM": "RVX", "GLD": "GVZ", "USO": "OVX"}
BAND = (-1.0, 8.0)
MIN_CORRELATION = 0.9
MIN_RANGE = 3.0
# the nine more pairs (README "Nine more pairs"): upper bound max(8, 0.25 * ours)
MORE = {"AAPL": "VXAPL", "AMZN": "VXAZN", "GOOGL": "VXGOG", "EEM": "VXEEM", "EWZ": "VXEWZ",
        "TLT": "VXTLT", "DIA": "VXD", "SLV": "VXSLV", "FXI": "VXFXI"}
MORE_RATIO = 0.25
URL = "https://cdn.cboe.com/api/global/us_indices/daily_prices/{}_History.csv"
# sessions already looked at before these rules were applied (README): the research pilot's 12
# sessions for the nine; the five's first run of this check on the sample (2026-11-18)
SEEN_MORE = {"2025-04-11", "2025-12-10", "2025-12-11", "2026-11-02", "2026-11-03", "2026-11-04",
             "2026-11-05", "2026-11-06", "2026-11-09", "2026-11-10", "2026-11-11", "2026-11-12"}
SEEN_FIVE = {"2026-11-02", "2026-11-03", "2026-11-04", "2026-11-05", "2026-11-06", "2026-11-09",
             "2026-11-10", "2026-11-11", "2026-11-12", "2026-11-13"}


def fetch(cache: str) -> int:
    out = Path(cache)
    out.mkdir(parents=True, exist_ok=True)
    log = []
    for index in MORE.values():
        url = URL.format(index)
        with urllib.request.urlopen(url, timeout=60) as r:
            body = r.read()
        name = f"{index}_History.csv"
        (out / name).write_bytes(body)
        log.append({"url": url, "file": name, "sha256": hashlib.sha256(body).hexdigest(),
                    "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")})
        time.sleep(1)
    (out / "fetched.json").write_text(json.dumps(log, indent=1) + "\n")
    return 0


def verify() -> None:
    for line in (HERE / "SHA256SUMS").read_text().splitlines():
        digest, name = line.split()
        if hashlib.sha256((HERE / name).read_bytes()).hexdigest() != digest:
            raise SystemExit(f"{name}: checksum differs from SHA256SUMS")


def verify_cache(cache: Path) -> list[dict]:
    log = json.loads((HERE / "fetched.json").read_text())
    for f in log:
        if hashlib.sha256((cache / f["file"]).read_bytes()).hexdigest() != f["sha256"]:
            raise SystemExit(f"{f['file']}: cache differs from cboe/fetched.json")
    return log


def closes(index: str, where: Path = HERE) -> dict[str, float]:
    out = {}
    with open(where / f"{index}_History.csv", newline="") as fh:
        for r in csv.DictReader(fh):
            v = r.get("CLOSE") or r.get(index)
            out[datetime.strptime(r["DATE"], "%m/%d/%Y").strftime("%Y-%m-%d")] = float(v)
    return out


def pearson(xs: list[float], ys: list[float]) -> float | None:
    n = len(xs)
    if n < 3:
        return None
    mx, my = sum(xs) / n, sum(ys) / n
    sxy = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    sxx = sum((x - mx) ** 2 for x in xs)
    syy = sum((y - my) ** 2 for y in ys)
    return sxy / (sxx * syy) ** 0.5 if sxx and syy else None


def pair(rows: list[dict], obj: str, index: str, close: dict, more: bool, band_only: bool) -> tuple[dict, bool, bool]:
    seen_set = SEEN_MORE if more else SEEN_FIVE
    sessions, xs, ys = [], [], []
    ok = ok_blind = True
    for r in sorted((r for r in rows if r["object_id"] == obj), key=lambda r: r["date"]):
        s = {"date": r["date"], "seen": r["date"] in seen_set, "state": r["state"], index: close.get(r["date"])}
        if r["state"] != "SUPPORTED" or not r["iv30"]:
            s["reason"] = r.get("reason")
        elif s[index] is None:
            s["reason"] = "no Cboe close for the date (NOT_COMPARABLE)"
        else:
            ours = 100 * float(r["iv30"])
            gap = s[index] - ours
            upper = max(BAND[1], MORE_RATIO * ours) if more else BAND[1]
            s.update(ours=round(ours, 3), gap=round(gap, 3), upper=round(upper, 3), inside=BAND[0] <= gap <= upper)
            if more:
                s["inside_fixed_band_of_the_five"] = BAND[0] <= gap <= BAND[1]
            ok &= s["inside"]
            if not s["seen"]:
                ok_blind &= s["inside"]
            xs.append(s[index]); ys.append(ours)
        sessions.append(s)
    rng = max(xs) - min(xs) if xs else 0.0
    rho = pearson(xs, ys)
    judged = rng >= MIN_RANGE and not band_only
    corr_ok = (rho is not None and rho >= MIN_CORRELATION) if judged else None
    if judged:
        ok &= bool(corr_ok)
    out = {
        "compared": len(xs), "compared_blind": sum(1 for s in sessions if "gap" in s and not s["seen"]),
        "not_compared": [{"date": s["date"], "reason": s.get("reason")} for s in sessions if "gap" not in s],
        "index_range": round(rng, 3),
        "correlation": None if rho is None else round(rho, 4),
        "correlation_judged": judged, "correlation_ok": corr_ok,
        "gap_min": min((s["gap"] for s in sessions if "gap" in s), default=None),
        "gap_max": max((s["gap"] for s in sessions if "gap" in s), default=None),
        "pass": ok, "pass_blind_band1": ok_blind,
        "sessions": sessions,
    }
    return out, ok, ok_blind


def main(path: str, cache: str | None, band_only: bool) -> int:
    verify()
    rows = list(csv.DictReader(open(path, newline=""), delimiter="\t"))
    report = {"band": BAND, "min_correlation": MIN_CORRELATION, "min_range": MIN_RANGE,
              "band_only": band_only, "dates": sorted({r["date"] for r in rows}), "pairs": {}}
    ok = ok_blind = True
    for obj, index in PAIRS.items():
        p, a, b = pair(rows, obj, index, closes(index), False, band_only)
        report["pairs"][f"{index}/{obj}"] = p
        ok &= a; ok_blind &= b
    if cache:
        report["more_band"] = f"[-1, max(8, {MORE_RATIO} * 100*iv30)]"
        report["more_files"] = verify_cache(Path(cache))
        for obj, index in MORE.items():
            p, a, b = pair(rows, obj, index, closes(index, Path(cache)), True, band_only)
            report["pairs"][f"{index}/{obj}"] = p
            ok &= a; ok_blind &= b
    report["pass"] = ok
    report["pass_blind_band1"] = ok_blind
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if ok else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "fetch":
        sys.exit(fetch(a[1]))
    flags = [x for x in a if x.startswith("--")]
    pos = [x for x in a if not x.startswith("--")]
    sys.exit(main(pos[0], pos[1] if len(pos) > 1 else None, "--band-only" in flags))
