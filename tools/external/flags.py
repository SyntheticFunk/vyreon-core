"""Flag agreement, the whole-store gate (d): our iv30's extremes against Cboe's own index (README "Flag
agreement", declared 2026-11-21 before any phase-1 value existed).

    psql -X -A -P footer=off -F $'\t' -d vyreon -v from=A -v to=B -f flags.sql > flags.tsv
    python3 -I flags.py flags.tsv FIRST LAST CACHE > report.json

FIRST..LAST: the sessions judged (flags.tsv must reach 63 sessions before FIRST). CACHE: the nine more
indices (`cboe.py fetch`), checked against the committed `cboe/fetched.json`; the five read the
committed files. Exit 0 when every judged pair passes, 1 when one fails, 2 when nothing is judged or the
gate is incomplete (the nine not read, or an index with no close on a judged session through LAST).

Both series are ranked exactly as the engine ranks a reading for D-070 (`h5-derived` `unusual::score`,
`vyreon_coordinates::unusual_flag`): today against the prior 63 sessions of the XNYS calendar (today
excluded), at least 48 of them present (D-404's floor), withheld when the prior values hold one or two
distinct values, when the robust scale is not positive or below a millionth of the values; percentile
= share of the prior values below today, ties (within 1e-12 relative, D-134) counting half. Extreme:
percentile >= 1 - 0.10 (high) or <= 0.10 (low). The robust-z half of D-070 is not judged; the
engine's full flag on each index-extreme session is reported beside. D-273: our prior sessions of
another science or method version of VS-IVS01 are not compared (the engine's `score_of_version`, which
also checks the parents' versions; the gate sees VS-IVS01's own labels only).
"""
from __future__ import annotations

import csv
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cboe  # noqa: E402  (the frozen index files and their checks)

WINDOW = 63
MIN_ELIGIBLE = (WINDOW * 3 + 3) // 4  # h1_numerics::completeness_floor(63) = 48
FLAG_EDGE = 0.10
TIE_REL = 1e-12
MIN_SHARE = 0.80
MIN_INDEX_EXTREMES = 10
PANEL_ONLY = {"SLV", "FXI"}  # judged on 63-session panels only (README, owner 2026-11-19)


def xnys_sessions(first: str, last: str) -> list[str]:
    import exchange_calendars as xc
    cal = xc.get_calendar("XNYS", start=first, end=last)
    return [d.date().isoformat() for d in cal.sessions_in_range(first, last)]


def _median(v: list[float]) -> float:
    v.sort()
    n = len(v)
    return v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2.0


def tied(a: float, b: float) -> bool:
    return a == b or abs(a - b) <= TIE_REL * max(abs(a), abs(b))


def score_of_version(today: float | None, prior: list[float | None], other: list[float | None]) -> tuple[str, float | None]:
    """The engine's `unusual::score_of_version`: the minimum met only with other-version sessions withholds."""
    state, p = score(today, prior)
    if state == "INSUFFICIENT_HISTORY":
        n = lambda v: sum(1 for x in v if x is not None and math.isfinite(x))
        if n(other) > 0 and n(prior) + n(other) >= MIN_ELIGIBLE:
            return "HISTORY_OF_ANOTHER_METHOD_VERSION", None
    return state, p


def score(today: float | None, prior: list[float | None]) -> tuple[str, float | None]:
    """The engine's `unusual::score`, percentile only: ("SUPPORTED", p) or (withheld reason, None)."""
    if today is None or not math.isfinite(today):
        return "TODAY_WITHHELD", None
    eligible = [v for v in prior if v is not None and math.isfinite(v)]
    if len(eligible) < MIN_ELIGIBLE:
        return "INSUFFICIENT_HISTORY", None
    distinct = len(set(eligible))
    if distinct == 1:
        return "FLAT_HISTORY", None
    if distinct <= 2:
        return "STEP_HISTORY", None
    m = _median(list(eligible))
    dev = sorted(abs(v - m) for v in eligible)
    mad = _median(list(dev))
    q90 = dev[-(-len(dev) * 9 // 10) - 1]
    scale = max(1.4826 * mad, q90 / 1.6449)
    if not scale > 0.0:
        return "FLAT_HISTORY", None
    if scale < 1e-6 * max(abs(m), abs(today)):
        return "DEGENERATE_SCALE", None
    below = sum(0.5 if tied(v, today) else 1.0 if v < today else 0.0 for v in eligible)
    return "SUPPORTED", below / len(eligible)


def edge(p: float | None) -> str | None:
    if p is None:
        return None
    if p >= 1.0 - FLAG_EDGE:
        return "high"
    if p <= FLAG_EDGE:
        return "low"
    return None


def engine_flag(z: float, p: float, line: float) -> bool:
    """`vyreon_coordinates::unusual_flag`."""
    return abs(z) >= line and ((z > 0.0 and p >= 1.0 - FLAG_EDGE) or (z < 0.0 and p <= FLAG_EDGE))


def _num(s: str | None) -> float | None:
    return float(s) if s not in (None, "") else None


def pair(obj: str, index: str, rows: dict, close: dict[str, float], calendar: list[str],
         judged_sessions: list[str]) -> dict:
    ours = {d: _num(r["iv30"]) if r["state"] == "SUPPORTED" else None for d, r in rows.items()}
    version = {d: r.get("version") for d, r in rows.items()}
    pos = {d: i for i, d in enumerate(calendar)}
    sessions = []
    for t in judged_sessions:
        i = pos[t]
        prior_days = calendar[max(0, i - WINDOW):i]
        s_idx, p_idx = score(close.get(t), [close.get(d) for d in prior_days])
        same = [ours.get(d) if version.get(d) == version.get(t) else None for d in prior_days]
        other = [ours.get(d) if version.get(d) != version.get(t) else None for d in prior_days]
        s_our, p_our = score_of_version(ours.get(t), same, other)
        r = rows.get(t)
        s = {"date": t, "index": close.get(t), "index_state": s_idx, "index_percentile": p_idx,
             "index_edge": edge(p_idx), "ours": ours.get(t), "ours_state": s_our,
             "ours_percentile": p_our, "ours_edge": edge(p_our)}
        if r is None:
            s["ours_published"] = "NOT_PUBLISHED"
        elif r["state"] != "SUPPORTED":
            s["ours_published"] = f'{r["state"]} {r["reason"]}'.strip()
        if r is not None and r.get("z") and r.get("percentile"):
            z, p = float(r["z"]), float(r["percentile"])
            line = _num(r.get("line"))
            s["engine"] = {"z": z, "percentile": p, "line": line,
                           "flagged": engine_flag(z, p, line) if line is not None else None}
            if line is None:
                s["engine"]["line_state"] = f'{r.get("line_state") or "ABSENT"} {r.get("line_reason") or ""}'.strip()
            if p_our is not None and abs(p - p_our) > 1e-12:
                s["engine_percentile_differs"] = True
        elif r is not None and r.get("z_state"):
            s["engine"] = {"state": r["z_state"], "reason": r.get("z_reason")}
        sessions.append(s)

    both = [s for s in sessions if s["index_state"] == "SUPPORTED" and s["ours_state"] == "SUPPORTED"]
    idx_ext = [s for s in both if s["index_edge"]]
    our_ext = [s for s in both if s["ours_edge"]]
    agree1 = sum(1 for s in idx_ext if s["ours_edge"] == s["index_edge"])
    agree2 = sum(1 for s in our_ext if s["index_edge"] == s["ours_edge"])
    share1 = agree1 / len(idx_ext) if idx_ext else None
    share2 = agree2 / len(our_ext) if our_ext else None
    why_not = None
    if len(idx_ext) < MIN_INDEX_EXTREMES:
        why_not = f"{len(idx_ext)} index-extreme sessions (< {MIN_INDEX_EXTREMES})"
    elif obj in PANEL_ONLY and len(judged_sessions) < WINDOW:
        why_not = f"judged on 63-session panels only; this panel has {len(judged_sessions)}"
    judged = why_not is None
    ok = (share1 is not None and share1 >= MIN_SHARE and share2 is not None and share2 >= MIN_SHARE) if judged else None
    return {
        "sessions_judged": len(judged_sessions),
        "comparable": len(both),
        "index_extreme": len(idx_ext), "index_extreme_agreeing": agree1,
        "share_index_to_ours": None if share1 is None else round(share1, 4),
        "ours_extreme": len(our_ext), "ours_extreme_agreeing": agree2,
        "share_ours_to_index": None if share2 is None else round(share2, 4),
        "judged": judged, "not_judged_because": why_not, "pass": ok,
        "engine_flagged_on_index_extreme": sum(1 for s in idx_ext if s.get("engine", {}).get("flagged")),
        "engine_percentile_differs": [s["date"] for s in sessions if s.get("engine_percentile_differs")],
        "not_comparable": [{"date": s["date"], "index": s["index_state"], "ours": s["ours_state"],
                            "published": s.get("ours_published")} for s in sessions if s not in both],
        "extremes": [s for s in sessions if s in both and (s["index_edge"] or s["ours_edge"])],
    }


def main(path: str, first: str, last: str, cache: str | None) -> int:
    cboe.verify()
    by_obj: dict[str, dict] = {}
    with open(path, newline="") as fh:
        for r in csv.DictReader(fh, delimiter="\t"):
            rows = by_obj.setdefault(r["object_id"], {})
            if r["date"] in rows:
                raise SystemExit(f'{path}: two rows for {r["object_id"]} on {r["date"]} (profile revisions '
                                 f'{rows[r["date"]].get("profile_revision")}, {r.get("profile_revision")}): refused')
            if int(r.get("unusual_profiles") or 0) > 1:
                raise SystemExit(f'{path}: unusualness of {r["object_id"]} on {r["date"]} from two profile revisions: refused')
            rows[r["date"]] = r
    dates = sorted({d for rows in by_obj.values() for d in rows})
    calendar = xnys_sessions("2022-01-03", max(last, dates[-1] if dates else last))
    known = set(calendar)
    stray = [d for d in dates if d not in known]
    if stray:
        raise SystemExit(f"dates in {path} that are not XNYS sessions: {stray[:5]}")
    judged = [d for d in calendar if first <= d <= last]
    if not judged:
        raise SystemExit(f"no XNYS session in {first}..{last}")
    first_prior = calendar[calendar.index(judged[0]) - WINDOW]
    if not dates or dates[0] > first_prior:
        print(f"note: {path} starts {dates[0] if dates else 'nowhere'}; the first session's window starts "
              f"{first_prior}: missing sessions count as not published", file=sys.stderr)
    pairs = [(o, i, cboe.closes(i)) for o, i in cboe.PAIRS.items()]
    report = {"declared": "README.md Flag agreement (2026-11-21)", "window": WINDOW, "min_eligible": MIN_ELIGIBLE,
              "edge": FLAG_EDGE, "min_share": MIN_SHARE, "min_index_extremes": MIN_INDEX_EXTREMES,
              "first": judged[0], "last": judged[-1], "complete": cache is not None, "pairs": {}}
    if cache:
        report["more_files"] = cboe.verify_cache(Path(cache))
        pairs += [(o, i, cboe.closes(i, Path(cache))) for o, i in cboe.MORE.items()]
    # an index whose file ends before LAST cannot judge the last sessions: the gate is incomplete
    short = {i: max(c) for _, i, c in pairs if c and max(c) < judged[-1]}
    if short:
        report["complete"] = False
        report["indices_ending_before_last"] = short
    for obj, index, close in pairs:
        report["pairs"][f"{index}/{obj}"] = pair(obj, index, by_obj.get(obj, {}), close, calendar, judged)
    verdicts = [p["pass"] for p in report["pairs"].values() if p["judged"]]
    report["judged_pairs"] = len(verdicts)
    report["pass"] = (all(verdicts) if verdicts else None) if report["complete"] else None
    json.dump(report, sys.stdout, indent=1)
    print()
    if not report["complete"] or not verdicts:
        return 2
    return 0 if all(verdicts) else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    if len(a) not in (3, 4):
        raise SystemExit(__doc__)
    sys.exit(main(a[0], a[1], a[2], a[3] if len(a) == 4 else None))
