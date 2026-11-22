"""flags.py on real series only (D-102): Cboe's committed closes stand in for both sides.

    python3 -I -m pytest tools/external/test_flags.py      (or: python3 -I tools/external/test_flags.py)
"""
import csv
import io
import json
import sys
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cboe  # noqa: E402
import flags  # noqa: E402


def _tsv(tmp: Path, obj: str, series: dict[str, float], scale: float = 0.01) -> Path:
    out = tmp / "flags.tsv"
    with open(out, "w", newline="") as fh:
        w = csv.writer(fh, delimiter="\t")
        w.writerow(["date", "object_id", "state", "iv30", "reason", "z_state", "z_reason", "z", "percentile", "line"])
        for d, v in sorted(series.items()):
            w.writerow([d, obj, "SUPPORTED", repr(v * scale), "", "", "", "", "", ""])
    return out


def _run(path: Path, first: str, last: str) -> tuple[int, dict]:
    buf = io.StringIO()
    with redirect_stdout(buf):
        code = flags.main(str(path), first, last, None)
    return code, json.loads(buf.getvalue())


def test_a_series_agrees_with_itself(tmp_path=Path("/tmp")):
    # SPY's iv30 replaced by VIX/100 over 2024-2026: every edge is the index's own, so both shares are 1
    vix = cboe.closes("VIX")
    sessions = set(flags.xnys_sessions("2024-01-02", "2026-11-17"))
    path = _tsv(tmp_path, "SPY", {d: v for d, v in vix.items() if d in sessions})
    code, report = _run(path, "2024-07-09", "2026-11-17")
    p = report["pairs"]["VIX/SPY"]
    assert p["judged"] and p["index_extreme"] >= 10
    assert p["share_index_to_ours"] == 1.0 and p["share_ours_to_index"] == 1.0 and p["pass"]
    assert code == 2  # without the nine (CACHE) the gate is incomplete: no verdict


def test_another_market_is_judged_and_can_fail(tmp_path=Path("/tmp")):
    # QQQ's iv30 replaced by GVZ (gold) against VXN (Nasdaq-100): a real, different market
    gvz = cboe.closes("GVZ")
    sessions = set(flags.xnys_sessions("2024-01-02", "2026-11-17"))
    path = _tsv(tmp_path, "QQQ", {d: v for d, v in gvz.items() if d in sessions})
    _, report = _run(path, "2024-07-09", "2026-11-17")
    p = report["pairs"]["VXN/QQQ"]
    assert p["judged"]
    assert p["share_index_to_ours"] < flags.MIN_SHARE or p["share_ours_to_index"] < flags.MIN_SHARE
    assert p["pass"] is False


def test_percentile_is_the_prior_63_sessions_today_excluded():
    # VIX on its 2025-04-08 peak: higher than every one of the prior 63 closes
    vix = cboe.closes("VIX")
    cal = flags.xnys_sessions("2024-01-02", "2025-12-31")
    i = cal.index("2025-04-08")
    state, p = flags.score(vix[cal[i]], [vix.get(d) for d in cal[i - 63:i]])
    assert state == "SUPPORTED" and p == 1.0 and flags.edge(p) == "high"
    # a short window (47 of 63 present) is withheld, as the engine's floor of 48
    prior = [vix.get(d) for d in cal[i - 63:i]]
    assert flags.score(vix[cal[i]], [None] * 16 + prior[16:])[0] == "INSUFFICIENT_HISTORY"


if __name__ == "__main__":
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        for name, f in list(globals().items()):
            if name.startswith("test_"):
                f(Path(d)) if f.__code__.co_argcount else f()
                print("ok", name)
