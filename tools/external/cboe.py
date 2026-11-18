"""Cboe volatility indices against the engine's VS-IVS01 iv30 (bands in README.md, declared first).

    python3 cboe.py VALUES.tsv > report.json

VALUES.tsv: one row per (date, object) with columns date, object_id, state, iv30, reason, as
`values.sql` prints them from core's published store (`psql -A -F $'\t'`).
"""
from __future__ import annotations

import csv
import hashlib
import json
import sys
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent / "cboe"
PAIRS = {"SPY": "VIX", "QQQ": "VXN", "IWM": "RVX", "GLD": "GVZ", "USO": "OVX"}
BAND = (-1.0, 8.0)
MIN_CORRELATION = 0.9
MIN_RANGE = 3.0


def verify() -> None:
    for line in (HERE / "SHA256SUMS").read_text().splitlines():
        digest, name = line.split()
        if hashlib.sha256((HERE / name).read_bytes()).hexdigest() != digest:
            raise SystemExit(f"{name}: checksum differs from SHA256SUMS")


def closes(index: str) -> dict[str, float]:
    out = {}
    with open(HERE / f"{index}_History.csv", newline="") as fh:
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


def main(path: str) -> int:
    verify()
    rows = list(csv.DictReader(open(path, newline=""), delimiter="\t"))
    report, ok = {"band": BAND, "min_correlation": MIN_CORRELATION, "min_range": MIN_RANGE, "pairs": {}}, True
    for obj, index in PAIRS.items():
        close = closes(index)
        sessions, xs, ys = [], [], []
        for r in sorted((r for r in rows if r["object_id"] == obj), key=lambda r: r["date"]):
            s = {"date": r["date"], "state": r["state"], index: close.get(r["date"])}
            if r["state"] != "SUPPORTED" or not r["iv30"]:
                s["reason"] = r.get("reason")
            elif s[index] is None:
                s["reason"] = "no Cboe close for the date"
            else:
                ours = 100 * float(r["iv30"])
                gap = s[index] - ours
                s.update(ours=round(ours, 3), gap=round(gap, 3), inside=BAND[0] <= gap <= BAND[1])
                ok &= s["inside"]
                xs.append(s[index]); ys.append(ours)
            sessions.append(s)
        rng = max(xs) - min(xs) if xs else 0.0
        rho = pearson(xs, ys)
        judged = rng >= MIN_RANGE
        corr_ok = (rho is not None and rho >= MIN_CORRELATION) if judged else None
        if judged:
            ok &= bool(corr_ok)
        report["pairs"][f"{index}/{obj}"] = {
            "compared": len(xs), "index_range": round(rng, 3),
            "correlation": None if rho is None else round(rho, 4),
            "correlation_judged": judged, "correlation_ok": corr_ok,
            "gap_min": min((s["gap"] for s in sessions if "gap" in s), default=None),
            "gap_max": max((s["gap"] for s in sessions if "gap" in s), default=None),
            "sessions": sessions,
        }
    report["pass"] = ok
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
