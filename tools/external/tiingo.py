"""Tiingo daily bars against the engine's price-based sensors (rules in README.md, declared first).

    python3 -I tiingo.py fetch CACHE TOKEN_FILE              # one request per security into CACHE
    python3 -I tiingo.py field CACHE FIELD.tsv > report.json
    python3 -I tiingo.py prices CACHE PRICES.tsv FIELD.tsv > report.json

CACHE holds Tiingo's JSON as downloaded (`<TICKER>.json`, licensed: never in the repo) and
`fetched.json` (url without the token, file, sha256, UTC fetch time). FIELD.tsv is
`tiingo-field.sql`'s output, PRICES.tsv `tiingo-prices.sql`'s (`psql -A -t -F $'\\t'`). The
token is read from TOKEN_FILE and sent only as the Authorization header. Exit 0 on pass.
"""
from __future__ import annotations

import bisect
import hashlib
import json
import math
import operator
import sys
import time
import urllib.request
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

SECURITY_IDS = [
    "SPY", "QQQ", "IWM", "DIA", "RSP", "MDY", "VTV", "ARKK", "XLK", "XLF", "XLE", "XLV", "XLY",
    "XLI", "XLC", "XLB", "XLP", "XLU", "XLRE", "IEF", "TLT", "TIP", "LQD", "HYG", "EMB", "GLD",
    "SLV", "USO", "UNG", "DBA", "UUP", "IBIT", "ETHA", "EFA", "EEM", "FXI", "EWJ", "EWZ", "INDA",
    "EWY", "VGK", "SMH", "KRE", "XHB", "XRT", "XBI", "URA", "AAPL", "MSFT", "NVDA", "AMD", "AVGO",
    "AMZN", "GOOGL", "META", "TSLA", "PLTR", "JPM", "LLY", "WMT", "BABA", "TSM", "MSTR", "COIN",
]
N = 64
GROUPS = [("US_BETA_BREADTH_STYLE", 8), ("US_SECTORS", 11), ("RATES_CREDIT_INFLATION", 6),
          ("COMMODITIES_DOLLAR_CRYPTO", 8), ("INTERNATIONAL", 8), ("MECHANISMS_THEMES", 6),
          ("LEADERSHIP_EQUITIES", 17)]
LABELS = [g for g, (_, size) in enumerate(GROUPS) for _ in range(size)]
PAIRS = [(a, b) for a in range(N) for b in range(a + 1, N)]
URL = "https://api.tiingo.com/tiingo/daily/{}/prices?startDate=2019-12-01&endDate=2026-11-17"
PAUSE = 0.5
PANEL = 376          # FIELD_PANEL_SESSIONS (vyreon-cli day.rs)
CAP, MIN_CTX = 126, 63
DEP_WINDOW, DEP_MIN = 250, 126
RMT_WINDOW, RMT_MIN = 250, 126
MIN_CROSS = 51

# tolerances (README "Tiingo daily prices")
RET_TOL = 5e-4
CORR_TOL = 0.01


# ---------------------------------------------------------------- fetch and cache

def fetch(cache: str, token_file: str) -> int:
    out = Path(cache)
    out.mkdir(parents=True, exist_ok=True)
    token = Path(token_file).read_text().strip()
    log_path = out / "fetched.json"
    log = json.loads(log_path.read_text()) if log_path.is_file() else []
    have = {f["file"] for f in log}
    for ticker in SECURITY_IDS:
        name = f"{ticker}.json"
        if name in have:
            continue
        url = URL.format(ticker)
        req = urllib.request.Request(url, headers={"Authorization": f"Token {token}",
                                                   "Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read()
        rows = json.loads(body)
        if not isinstance(rows, list) or not rows or "close" not in rows[0]:
            raise SystemExit(f"{ticker}: Tiingo answered without daily rows")
        (out / name).write_bytes(body)
        log.append({"url": url, "file": name, "sha256": hashlib.sha256(body).hexdigest(),
                    "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")})
        log_path.write_text(json.dumps(log, indent=1) + "\n")
        time.sleep(PAUSE)
    return 0


class Bars:
    """Tiingo's bars per security: date -> (open, high, low, close, volume, div, split)."""

    def __init__(self, cache: Path):
        self.files = json.loads((cache / "fetched.json").read_text())
        committed = Path(__file__).resolve().parent / "tiingo" / "fetched.json"
        if committed.is_file() and json.loads(committed.read_text()) != self.files:
            raise SystemExit("cache's fetched.json differs from the committed tiingo/fetched.json")
        self.by = {}
        for f in self.files:
            body = (cache / f["file"]).read_bytes()
            if hashlib.sha256(body).hexdigest() != f["sha256"]:
                raise SystemExit(f"{f['file']}: cache differs from fetched.json")
            t = f["file"][:-5]
            self.by[t] = {r["date"][:10]: (float(r["open"]), float(r["high"]), float(r["low"]),
                                           float(r["close"]), float(r["volume"]), float(r["divCash"]),
                                           float(r["splitFactor"])) for r in json.loads(body)}
        missing = [t for t in SECURITY_IDS if t not in self.by]
        if missing:
            raise SystemExit(f"cache lacks {missing}")
        self.calendar = sorted(self.by["SPY"])
        self.index = {d: i for i, d in enumerate(self.calendar)}

    def bar(self, t, d):
        return self.by[t].get(d)

    def prev(self, d):
        i = self.index[d]
        return self.calendar[i - 1] if i > 0 else None

    def factor(self, t, d):
        """f_d: the factor relating the bars before d to d (None without both bars)."""
        p = self.prev(d)
        b, a = self.bar(t, d), self.bar(t, p) if p else None
        if not b or not a:
            return None
        div, split = b[5], b[6]
        f = 1.0
        if 0.0 < div < a[3]:
            f *= 1.0 - div / a[3]
        return f / split

    def adj_return(self, t, d):
        p = self.prev(d)
        b, a = self.bar(t, d), self.bar(t, p) if p else None
        if not b or not a:
            return None
        return math.log(b[3] / (a[3] * self.factor(t, d)))

    def raw_return(self, t, d):
        p = self.prev(d)
        b, a = self.bar(t, d), self.bar(t, p) if p else None
        return math.log(b[3] / a[3]) if b and a else None

    def split_free_return(self, t, d):
        p = self.prev(d)
        b, a = self.bar(t, d), self.bar(t, p) if p else None
        return math.log(b[3] * b[6] / a[3]) if b and a else None


# ---------------------------------------------------------------- numerics

def dot(a, b):
    return math.fsum(map(operator.mul, a, b))


def mean(v):
    return math.fsum(v) / len(v)


def type7(values, p):
    s = sorted(values)
    h = (len(s) - 1) * p
    lo, hi = math.floor(h), math.ceil(h)
    return s[lo] if lo == hi else s[lo] + (s[hi] - s[lo]) * (h - lo)


def median(values):
    return type7(values, 0.5)


def avg_ranks(values):
    order = sorted(range(len(values)), key=lambda i: values[i])
    ranks = [0.0] * len(values)
    i = 0
    while i < len(values):
        j = i + 1
        while j < len(values) and values[order[j]] == values[order[i]]:
            j += 1
        r = (i + 1 + j) / 2.0
        for k in order[i:j]:
            ranks[k] = r
        i = j
    return ranks


def pearson(x, y):
    mx, my = mean(x), mean(y)
    xc = [v - mx for v in x]
    yc = [v - my for v in y]
    xx, yy = dot(xc, xc), dot(yc, yc)
    if xx == 0 or yy == 0:
        return None
    return dot(xc, yc) / math.sqrt(xx * yy)


def corr_matrix(cols):
    """cols: N column vectors of equal length -> N x N Pearson (None if a column is constant)."""
    cent, ss = [], []
    for c in cols:
        m = mean(c)
        v = [x - m for x in c]
        s = dot(v, v)
        if s == 0:
            return None
        cent.append(v)
        ss.append(s)
    n = len(cols)
    r = [[1.0] * n for _ in range(n)]
    for a in range(n):
        for b in range(a + 1, n):
            r[a][b] = r[b][a] = dot(cent[a], cent[b]) / math.sqrt(ss[a] * ss[b])
    return r


def eigenvalues(matrix):
    """Cyclic Jacobi on a symmetric matrix; descending eigenvalues."""
    n = len(matrix)
    a = [row[:] for row in matrix]
    for _ in range(100):
        off = math.fsum(a[p][q] ** 2 for p in range(n) for q in range(p + 1, n))
        if off < 1e-26:
            break
        for p in range(n - 1):
            ap = a[p]
            for q in range(p + 1, n):
                apq = ap[q]
                if apq == 0.0:
                    continue
                aq = a[q]
                theta = (aq[q] - ap[p]) / (2.0 * apq)
                t = (1.0 if theta >= 0 else -1.0) / (abs(theta) + math.sqrt(theta * theta + 1.0))
                c = 1.0 / math.sqrt(t * t + 1.0)
                s = t * c
                for row in a:
                    rp, rq = row[p], row[q]
                    row[p] = c * rp - s * rq
                    row[q] = s * rp + c * rq
                for k in range(n):
                    pk, qk = ap[k], aq[k]
                    ap[k] = c * pk - s * qk
                    aq[k] = s * pk + c * qk
    else:
        raise SystemExit("Jacobi did not converge")
    values = sorted((a[i][i] for i in range(n)), reverse=True)
    trace = math.fsum(matrix[i][i] for i in range(n))
    if abs(math.fsum(values) - trace) > 1e-9:
        raise SystemExit("Jacobi trace check failed")
    return values


def leading_vector(matrix):
    """Unit leading eigenvector by power iteration (sign: largest component positive)."""
    n = len(matrix)
    x = [1.0 / math.sqrt(n)] * n
    for _ in range(100000):
        y = [dot(row, x) for row in matrix]
        norm = math.sqrt(dot(y, y))
        y = [v / norm for v in y]
        if math.sqrt(math.fsum((u - v) ** 2 for u, v in zip(x, y))) < 1e-14:
            x = y
            break
        x = y
    else:
        raise SystemExit("power iteration did not converge")
    return x


def compare_rank_one(u, v):
    d = abs(dot(u, v))
    return d * d, math.degrees(math.acos(min(1.0, d)))


def midrank(current, refs):
    below = sum(1 for r in refs if r < current and not ties(r, current))
    tie = sum(1 for r in refs if ties(r, current))
    return (below + 0.5 * tie) / len(refs)


def ties(a, b):
    return abs(a - b) <= 1e-12 * max(abs(a), abs(b))


# ---------------------------------------------------------------- comparison bookkeeping

class Book:
    def __init__(self):
        self.q = defaultdict(lambda: {"compared": 0, "failed": 0, "not_comparable": 0,
                                      "max_abs_delta": 0.0, "worst": None})
        self.failures = []
        self.not_comparable = defaultdict(int)
        self.reported = defaultdict(lambda: {"compared": 0, "agree": 0, "examples": []})
        self.listed = {}

    def judge(self, quantity, where, ours, tiingo, tol, extra=None):
        q = self.q[quantity]
        if tiingo is None:
            q["failed"] += 1
            q["compared"] += 1
            self.failures.append({"quantity": quantity, **where, "stored": ours, "tiingo": None,
                                  "cause": "stored supported, recompute undefined on the same inputs"})
            return False
        delta = tiingo - ours
        q["compared"] += 1
        if abs(delta) >= q["max_abs_delta"]:
            q["max_abs_delta"] = abs(delta)
            q["worst"] = {**where, "stored": ours, "tiingo": tiingo, "tolerance": tol}
        ok = abs(delta) <= tol
        if not ok:
            q["failed"] += 1
            self.failures.append({"quantity": quantity, **where, "stored": ours, "tiingo": tiingo,
                                  "delta": delta, "tolerance": tol, **(extra or {})})
        return ok

    def skip(self, quantity, reason):
        self.q[quantity]["not_comparable"] += 1
        self.not_comparable[f"{quantity}: {reason}"] += 1

    def report_only(self, quantity, where, ours, tiingo, close=None):
        r = self.reported[quantity]
        r["compared"] += 1
        same = (ours == tiingo) if close is None else (tiingo is not None and abs(ours - tiingo) <= close)
        if same:
            r["agree"] += 1
        elif len(r["examples"]) < 5:
            r["examples"].append({**where, "stored": ours, "tiingo": tiingo})

    def summary(self):
        return {k: {**v, "max_abs_delta": float(f"{v['max_abs_delta']:.6g}")} for k, v in sorted(self.q.items())}

    def passed(self):
        return not self.failures


def percentile_tolerance(current, refs, level_tol):
    m = sum(1 for r in refs if abs(r - current) <= 2 * level_tol)
    return m / len(refs)


# ---------------------------------------------------------------- stored bodies

def read_field(path):
    """{date: {owner: body}}, {date: build}."""
    out, builds = defaultdict(dict), {}
    with open(path) as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line:
                continue
            d, owner, build, body = line.split("\t", 3)
            out[d][owner] = json.loads(body)
            builds[d] = build
    return out, builds


def coords(body):
    return {c["id"]: c for c in body.get("coordinates", [])}


def arrays(body):
    return {a["role"]: a for a in body.get("arrays", [])}


def withheld_cells(field, bars):
    """The engine's unsupported (security, session) cells from the FIELD-DEP01 masks (README)."""
    unsupported, covered = set(), set()
    for d, owners in field.items():
        dep = owners.get("FIELD-DEP01")
        if not dep:
            continue
        axis = dep["mask_axis"]
        covered.update(axis)
        if dep["mask_count"] != len(PAIRS):
            raise SystemExit(f"{d}: FIELD-DEP01 holds {dep['mask_count']} masks, not {len(PAIRS)}")
        false = defaultdict(set)  # (pair) -> rows not true
        for role, rows in dep["mask_not_true"].items():
            _, a, b, _ = role.split(".")
            false[(SECURITY_IDS.index(a), SECURITY_IDS.index(b))] = set(rows)
        for row, session in enumerate(axis):
            for i in range(N):
                if all(row in false.get((min(i, j), max(i, j)), ()) for j in range(N) if j != i):
                    unsupported.add((i, session))
        # the masks' axis is the engine's calendar: Tiingo's SPY dates must agree on it
        for s in axis:
            if s not in bars.index:
                raise SystemExit(f"calendar: engine session {s} has no Tiingo SPY bar")
        lo, hi = bars.index[axis[0]], bars.index[axis[-1]]
        if bars.calendar[lo:hi + 1] != axis:
            raise SystemExit(f"{d}: Tiingo's SPY dates differ from FIELD-DEP01's session axis")
    return unsupported, covered


# ---------------------------------------------------------------- check 1: FIELD

class Panel:
    """The engine's adjusted return cells on Tiingo's values: supported where the engine says."""

    def __init__(self, bars: Bars, unsupported: set):
        self.bars, self.unsupported = bars, unsupported
        self.memo = {}

    def cell(self, i, session):
        """('ok', r) | ('withheld', None) | ('tiingo_missing', None)."""
        key = (i, session)
        if key not in self.memo:
            if key in self.unsupported:
                self.memo[key] = ("withheld", None)
            else:
                r = self.bars.adj_return(SECURITY_IDS[i], session)
                self.memo[key] = ("ok", r) if r is not None else ("tiingo_missing", None)
        return self.memo[key]

    def raw_cell(self, i, session):
        if (i, session) in self.unsupported:
            return ("withheld", None)
        r = self.bars.split_free_return(SECURITY_IDS[i], session)
        return ("ok", r) if r is not None else ("tiingo_missing", None)

    def rows(self, sessions, raw=False):
        """Rows of all 64 values, or the reason they are not available."""
        out = []
        for s in sessions:
            row = []
            for i in range(N):
                state, v = (self.raw_cell if raw else self.cell)(i, s)
                if state != "ok":
                    return state, s
                row.append(v)
            out.append(row)
        return "ok", out


def columns(rows, keep=None):
    keep = range(len(rows[0])) if keep is None else keep
    return [[r[i] for r in rows] for i in keep]


def upper(matrix):
    return [matrix[a][b] for a, b in PAIRS]


def sync_level(panel, sessions):
    state, rows = panel.rows(sessions)
    if state != "ok":
        return {"state": state, "at": rows}
    r = corr_matrix(columns(rows))
    if r is None:
        return {"state": "degenerate"}
    off = upper(r)
    return {"state": "ok", "off_diagonal_mean": mean(off), "median": type7(off, 0.5),
            "q10": type7(off, 0.10), "q90": type7(off, 0.90), "matrix": r}


def cross_section(panel, session):
    vals, ords = [], []
    for i in range(N):
        state, v = panel.cell(i, session)
        if state == "tiingo_missing":
            return None
        if state == "ok":
            vals.append(v)
            ords.append(i)
    return ords, vals


def br01(vals):
    n = len(vals)
    pos, neg, zero = sum(v > 0 for v in vals), sum(v < 0 for v in vals), sum(v == 0 for v in vals)
    sp = pos / n - neg / n
    return {"positive_count": pos, "negative_count": neg, "zero_count": zero,
            "positive_fraction": pos / n, "negative_fraction": neg / n, "zero_fraction": zero / n,
            "signed_prevalence": sp, "directional_synchronization": abs(sp), "fragmentation": 1 - abs(sp)}


def dist01(vals):
    q = [type7(vals, p) for p in (0.10, 0.25, 0.50, 0.75, 0.90)]
    c = median(vals)
    return {"q10": q[0], "q25": q[1], "median": q[2], "q75": q[3], "q90": q[4],
            "cross_sectional_mad": median([abs(v - c) for v in vals]), "iqr": q[3] - q[1]}


def xconc01(ords, vals):
    a = [abs(v) for v in vals]
    total = math.fsum(a)
    if total == 0:
        return None
    shares = [x / total for x in a]
    hhi = dot(shares, shares)
    ranked = sorted(zip(a, ords), key=lambda t: (-t[0], t[1]))
    return {"absolute_move_sum": total, "absolute_move_hhi": hhi, "effective_count": 1 / hhi,
            "top_one_share": ranked[0][0] / total, "top_five_share": math.fsum(x for x, _ in ranked[:5]) / total,
            "top_five.share": [x / total for x, _ in ranked[:5]],
            "top_five.object_id": [SECURITY_IDS[o] for _, o in ranked[:5]]}


DAILY_TOL = {
    "FIELD-BR01": {"positive_count": 1, "negative_count": 1, "zero_count": 1, "positive_fraction": 1 / 64,
                   "negative_fraction": 1 / 64, "zero_fraction": 1 / 64, "signed_prevalence": 2 / 64,
                   "directional_synchronization": 2 / 64, "fragmentation": 2 / 64},
    "FIELD-DIST01": {"q10": RET_TOL, "q25": RET_TOL, "median": RET_TOL, "q75": RET_TOL, "q90": RET_TOL,
                     "cross_sectional_mad": 1e-3, "iqr": 1e-3},
    "FIELD-XCONC01": {"absolute_move_sum": 64 * RET_TOL, "absolute_move_hhi": 0.005, "top_one_share": 0.005,
                      "top_five_share": 0.005, "effective_count": "rel0.05"},
}
CONTEXTS = {"FIELD-BR01": ["directional_synchronization"], "FIELD-DIST01": ["cross_sectional_mad"],
            "FIELD-XCONC01": ["absolute_move_hhi", "top_five_share"],
            "FIELD-SYNC01": ["W21.off_diagonal_mean", "W63.off_diagonal_mean"]}
SYNC_TOL = {"off_diagonal_mean": CORR_TOL, "median": CORR_TOL, "q10": CORR_TOL, "q90": CORR_TOL}


class FieldRun:
    def __init__(self, bars, panel):
        self.bars, self.panel = bars, panel
        self.memo = {}

    def sessions_before(self, e, k):
        i = self.bars.index[e]
        return self.bars.calendar[i - k + 1:i + 1] if i - k + 1 >= 0 else None

    def daily(self, owner, e):
        key = (owner, e)
        if key in self.memo:
            return self.memo[key]
        if owner == "FIELD-SYNC01":
            out = {}
            for w in (21, 63):
                lv = sync_level(self.panel, self.sessions_before(e, w))
                if lv["state"] != "ok":
                    out[f"W{w}"] = lv
                    continue
                out[f"W{w}"] = lv
                for k in SYNC_TOL:
                    out[f"W{w}.{k}"] = lv[k]
            if e not in self.keep_matrix:
                for w in ("W21", "W63"):
                    out[w].pop("matrix", None)
            res = out
        else:
            cs = cross_section(self.panel, e)
            if cs is None:
                res = {"state": "tiingo_missing"}
            elif len(cs[1]) < MIN_CROSS:
                res = {"state": "insufficient"}
            elif owner == "FIELD-BR01":
                res = br01(cs[1])
            elif owner == "FIELD-DIST01":
                res = dist01(cs[1])
            else:
                res = xconc01(*cs) or {"state": "zero_denominator"}
        self.memo[key] = res
        return res


def judge_coordinate(book, owner, cid, stored, tiingo_value, tol, where):
    quantity = f"{owner} {cid}"
    c = stored.get(cid)
    if c is None:
        book.skip(quantity, "role absent from the stored body")
        return
    if c["state"] != "SUPPORTED":
        book.skip(quantity, f"stored {c['state']} {c.get('reason')}")
        return
    if isinstance(tol, str):  # relative
        tol = float(tol[3:]) * abs(c["value"])
    book.judge(quantity, where, c["value"], tiingo_value, tol)


def judge_context(book, run, owner, level, d, stored_coords, stored_arrays, tol):
    cid = f"{level}.level_percentile_cap126"
    ref = stored_arrays.get(f"{cid}.reference")
    c = stored_coords.get(cid)
    if ref is None or c is None:
        book.skip(f"{owner} {cid}", "context absent from the stored body")
        return
    labels = ref["axes"][0]["labels"]
    mine = []
    for endpoint, cell in zip(labels, ref["cells"]):
        if cell["state"] != "SUPPORTED":
            continue
        lv = run.daily(owner, endpoint).get(level)
        q = f"{owner} {cid}.reference (level history)"
        if (q, endpoint) not in run.judged:
            run.judged.add((q, endpoint))
            book.judge(q, {"date": d, "endpoint": endpoint}, cell["value"], lv, tol)
        mine.append(lv)
    if c["state"] != "SUPPORTED":
        book.skip(f"{owner} {cid}", f"stored {c['state']} {c.get('reason')}")
        return
    current = run.daily(owner, d).get(level)
    if current is None or any(v is None for v in mine) or not mine:
        book.judge(f"{owner} {cid}", {"date": d}, c["value"], None, 0)
        return
    pt = percentile_tolerance(current, mine, tol)
    book.judge(f"{owner} {cid}", {"date": d}, c["value"], midrank(current, mine), pt,
               {"levels_within_twice_the_level_tolerance": round(pt * len(mine))})


def field_check(cache, path):
    bars = Bars(Path(cache))
    field, builds = read_field(path)
    unsupported, covered = withheld_cells(field, bars)
    panel = Panel(bars, unsupported)
    run = FieldRun(bars, panel)
    run.keep_matrix = set(field)
    run.judged = set()
    book = Book()
    notes = {}
    for d in sorted(field):
        owners = field[d]
        iT = bars.index[d]
        panel_sessions = bars.calendar[max(0, iT - PANEL + 1):iT + 1]
        withheld_in_panel = sum(1 for (i, s) in unsupported if panel_sessions[0] <= s <= d)
        note = notes[d] = {"engine_build": builds[d], "engine_withheld_cells_in_panel": withheld_in_panel}

        # 7. cross-section owners and 8. the per-return input check
        for owner in ("FIELD-BR01", "FIELD-DIST01", "FIELD-XCONC01"):
            body = owners[owner]
            sc, sa = coords(body), arrays(body)
            mine = run.daily(owner, d)
            for cid, tol in DAILY_TOL[owner].items():
                judge_coordinate(book, owner, cid, sc, mine.get(cid), tol, {"date": d})
            for level in CONTEXTS[owner]:
                judge_context(book, run, owner, level, d, sc, sa, DAILY_TOL[owner][level])
            if owner == "FIELD-XCONC01" and "top_five.share" in mine:
                cells = sa["top_five.share"]["cells"]
                for k, cell in enumerate(cells):
                    if cell["state"] == "SUPPORTED":
                        book.judge(f"{owner} top_five.share", {"date": d, "rank": k + 1}, cell["value"],
                                   mine["top_five.share"][k], 0.005)
                ids = [c["value"] for c in sa["top_five.object_id"]["cells"]]
                book.report_only(f"{owner} top_five.object_id", {"date": d}, ids, mine["top_five.object_id"])
            if owner == "FIELD-DIST01":
                for i, cell in enumerate(sa["object_return_state"]["cells"]):
                    q = "FIELD-DIST01 object_return_state (input: adjusted log return on T)"
                    if cell["state"] != "SUPPORTED":
                        book.skip(q, f"stored {cell['state']} {cell.get('reason')}")
                        continue
                    state, r = panel.cell(i, d)
                    if state == "tiingo_missing":
                        book.skip(q, "Tiingo lacks the bar")
                        continue
                    book.judge(q, {"date": d, "object_id": SECURITY_IDS[i]}, cell["value"], r, RET_TOL)

        # 1. SYNC01
        body = owners["FIELD-SYNC01"]
        sc, sa = coords(body), arrays(body)
        mine = run.daily("FIELD-SYNC01", d)
        for w in (21, 63):
            lv = mine[f"W{w}"]
            if lv["state"] not in ("ok",):
                for k in SYNC_TOL:
                    c = sc.get(f"W{w}.{k}")
                    if c and c["state"] == "SUPPORTED":
                        if lv["state"] == "tiingo_missing":
                            book.skip(f"FIELD-SYNC01 W{w}.{k}", "Tiingo lacks a bar in the window")
                        else:
                            book.judge(f"FIELD-SYNC01 W{w}.{k}", {"date": d}, c["value"], None, 0)
                    else:
                        book.skip(f"FIELD-SYNC01 W{w}.{k}", f"stored {c and c['state']}")
                continue
            for k, tol in SYNC_TOL.items():
                judge_coordinate(book, "FIELD-SYNC01", f"W{w}.{k}", sc, lv[k], tol, {"date": d})
            window = run.sessions_before(d, w)
            for cid, val in ((f"W{w}.pair_count", len(PAIRS)), (f"W{w}.window_start", window[0]),
                             (f"W{w}.window_end", window[-1])):
                c = sc.get(cid)
                book.report_only(f"FIELD-SYNC01 {cid} (exact)", {"date": d}, c and c["value"], val)
                if c and c["value"] != val:
                    book.failures.append({"quantity": f"FIELD-SYNC01 {cid}", "date": d, "stored": c["value"],
                                          "tiingo": val, "cause": "window identity differs"})
            arr = sa.get(f"W{w}.pearson_matrix")
            if arr and arr.get("cells"):
                m = lv["matrix"]
                for a, b in PAIRS:
                    cell = arr["cells"][a * N + b]
                    if cell["state"] == "SUPPORTED":
                        book.judge(f"FIELD-SYNC01 W{w}.pearson_matrix cell", {"date": d, "pair": f"{SECURITY_IDS[a]}.{SECURITY_IDS[b]}"},
                                   cell["value"], m[a][b], 0.02)
        for level in CONTEXTS["FIELD-SYNC01"]:
            judge_context(book, run, "FIELD-SYNC01", level, d, sc, sa, CORR_TOL)

        # 2-3. RMT01 and DIM01
        eligible = [s for s in reversed(panel_sessions)
                    if all(panel.cell(i, s)[0] != "withheld" for i in range(N))][:RMT_WINDOW][::-1]
        rmt, dim = coords(owners["FIELD-RMT01"]), coords(owners["FIELD-DIM01"])
        rmt_arr = arrays(owners["FIELD-RMT01"])
        note["rmt_rows"] = len(eligible)
        stored_rows = rmt.get("leading_eigenvalue", {}).get("eligible_count")
        if stored_rows != len(eligible):
            book.failures.append({"quantity": "FIELD-RMT01 rows (consistency)", "date": d, "stored": stored_rows,
                                  "tiingo": len(eligible), "cause": "the engine mask gives another row count"})
        state, rows = panel.rows(eligible)
        spectrum = None
        if len(eligible) < RMT_MIN or state != "ok":
            for cid in ("leading_eigenvalue",):
                book.skip(f"FIELD-RMT01 {cid}", "Tiingo lacks a bar in the window" if state == "tiingo_missing" else "too few rows")
        else:
            r = corr_matrix(columns(rows))
            spectrum = eigenvalues(r)
            note["leading_share_tiingo"] = spectrum[0] / N
            judge_coordinate(book, "FIELD-RMT01", "leading_eigenvalue", rmt, spectrum[0], "rel0.01", {"date": d})
            if rmt.get("leading_eigenvalue", {}).get("state") == "SUPPORTED":
                note["leading_share_stored"] = rmt["leading_eigenvalue"]["value"] / N
            sp = rmt_arr.get("spectrum")
            for k, cell in enumerate(sp["cells"] if sp else []):
                if k == 0:
                    continue
                if cell["state"] == "SUPPORTED":
                    book.judge("FIELD-RMT01 spectrum ranks 2..64", {"date": d, "rank": k + 1}, cell["value"], spectrum[k], 0.02)
            clipped = [max(v, 0.0) for v in spectrum]
            total = math.fsum(clipped)
            cum, acc = [], 0.0
            for v in clipped:
                acc += v
                cum.append(acc)
            k95 = next(i + 1 for i, c in enumerate(cum) if c / total >= 0.95)
            pr = total * total / dot(clipped, clipped)
            ent = math.exp(-math.fsum(q * math.log(q) for q in (v / total for v in clipped) if q > 0))
            judge_coordinate(book, "FIELD-DIM01", "k95", dim, k95, 1, {"date": d})
            judge_coordinate(book, "FIELD-DIM01", "participation_ratio", dim, pr, "rel0.02", {"date": d})
            judge_coordinate(book, "FIELD-DIM01", "entropy_effective_rank", dim, ent, "rel0.02", {"date": d})
            rl = arrays(owners["FIELD-DIM01"]).get("reconstruction_loss")
            for k, cell in enumerate(rl["cells"] if rl else []):
                if cell["state"] == "SUPPORTED":
                    book.judge("FIELD-DIM01 reconstruction_loss", {"date": d, "k": k + 1}, cell["value"],
                               1.0 - cum[k] / total, (0.01 * spectrum[0] + 0.02 * k) / 64)

        # 4-5. DEP01 and GRP01
        dep_rows = bars.calendar[max(0, iT - DEP_WINDOW + 1):iT + 1]
        dep = coords(owners["FIELD-DEP01"])
        if owners["FIELD-DEP01"]["mask_axis"] != dep_rows:
            raise SystemExit(f"{d}: FIELD-DEP01 rows differ from the last 250 calendar sessions")
        cells = [[panel.cell(i, s) for i in range(N)] for s in dep_rows]
        scales = []
        for row in cells:
            if any(st == "tiingo_missing" for st, _ in row):
                scales.append("tiingo_missing")
                continue
            vals = [abs(v) for st, v in row if st == "ok"]
            sc_ = median(vals) if len(vals) >= MIN_CROSS else None
            scales.append(sc_ if sc_ and sc_ > 0 else None)
        pearsons = {}
        dep_missing = 0
        for a, b in PAIRS:
            pre = f"pair.{SECURITY_IDS[a]}.{SECURITY_IDS[b]}"
            x, y, sx, sy, missing = [], [], [], [], False
            for row, s in zip(cells, scales):
                (sa_, va), (sb_, vb) = row[a], row[b]
                if sa_ == "withheld" or sb_ == "withheld":
                    continue
                if sa_ == "tiingo_missing" or sb_ == "tiingo_missing" or s == "tiingo_missing":
                    missing = True
                    break
                x.append(va); y.append(vb)
                if s is not None:
                    sx.append(va / s); sy.append(vb / s)
            if missing:
                dep_missing += 1
                for m in ("pearson", "spearman", "signed_concordance", "categorical_sign_agreement", "common_scale_pearson"):
                    book.skip(f"FIELD-DEP01 {m}", "Tiingo lacks a bar in the pair's window")
                continue
            n_ = len(x)
            cc = dep.get(f"{pre}.common_count")
            book.judge("FIELD-DEP01 common_count (the engine's mask: consistency)", {"date": d, "pair": pre[5:]},
                       cc and cc["value"], n_, 0)
            dep_q = lambda m: f"FIELD-DEP01 {m}"
            for m, val, tol in (
                ("pearson", pearson(x, y) if n_ >= DEP_MIN else None, CORR_TOL),
                ("spearman", pearson(avg_ranks(x), avg_ranks(y)) if n_ >= DEP_MIN else None, CORR_TOL),
                ("common_scale_pearson", pearson(sx, sy) if len(sx) >= DEP_MIN else None, CORR_TOL),
                ("signed_concordance", (math.fsum(sgn(p) * sgn(q) for p, q in zip(x, y)) / n_) if n_ >= DEP_MIN else None, 6 / max(n_, 1)),
                ("categorical_sign_agreement", (sum(sgn(p) == sgn(q) for p, q in zip(x, y)) / n_) if n_ >= DEP_MIN else None, 3 / max(n_, 1)),
            ):
                c = dep.get(f"{pre}.{m}")
                if c is None or c["state"] != "SUPPORTED":
                    book.skip(dep_q(m), f"stored {c and c['state']} {c and c.get('reason')}")
                    continue
                book.judge(dep_q(m), {"date": d, "pair": pre[5:]}, c["value"], val, tol)
                if m == "pearson" and val is not None:
                    pearsons[(a, b)] = val
        note["dep01_pairs_not_comparable"] = dep_missing
        grp_body = owners["FIELD-GRP01"]
        grp, grp_arr = coords(grp_body), arrays(grp_body)
        if dep_missing:
            book.skip("FIELD-GRP01 (all)", "a pair was not comparable")
        else:
            within = [v for (a, b), v in pearsons.items() if LABELS[a] == LABELS[b]]
            between = [v for (a, b), v in pearsons.items() if LABELS[a] != LABELS[b]]
            wm, bm = median(within), median(between)
            judge_coordinate(book, "FIELD-GRP01", "within_pair_count", grp, len(within), 0, {"date": d})
            judge_coordinate(book, "FIELD-GRP01", "between_pair_count", grp, len(between), 0, {"date": d})
            judge_coordinate(book, "FIELD-GRP01", "within_median", grp, wm, CORR_TOL, {"date": d})
            judge_coordinate(book, "FIELD-GRP01", "between_median", grp, bm, CORR_TOL, {"date": d})
            judge_coordinate(book, "FIELD-GRP01", "median_difference", grp, wm - bm, CORR_TOL, {"date": d})
            gm = []
            for g in range(len(GROUPS)):
                vals = [v for (a, b), v in pearsons.items() if LABELS[a] == LABELS[b] == g]
                gm.append(median(vals))
            judge_coordinate(book, "FIELD-GRP01", "group_equal_weighted_difference", grp, mean(gm) - bm, CORR_TOL, {"date": d})
            for g, cell in enumerate(grp_arr["group.within_median"]["cells"]):
                if cell["state"] == "SUPPORTED":
                    book.judge("FIELD-GRP01 group.within_median", {"date": d, "group": GROUPS[g][0]}, cell["value"], gm[g], CORR_TOL)

        # 6. SUB01
        sub_body = owners["FIELD-SUB01"]
        sub, sub_arr = coords(sub_body), arrays(sub_body)
        if sub.get("pearson_spearman.overlap", {}).get("state") != "SUPPORTED":
            book.skip("FIELD-SUB01 (all)", f"stored {sub.get('pearson_spearman.overlap', {}).get('reason')}")
        elif spectrum is None or len(rows) < 250:
            book.skip("FIELD-SUB01 (all)", "RMT rows not comparable")
        else:
            sub_check(book, panel, d, eligible, rows, sub, sub_arr, note)
    return book, notes, bars, builds


def sgn(v):
    return 1.0 if v > 0 else (-1.0 if v < 0 else 0.0)


def positive_row_scales(rows):
    scales = [median([abs(v) for v in r]) for r in rows]
    pos = [s for s in scales if s > 0]
    rep = median(pos) if pos else 1.0
    return [s if s > 0 else rep for s in scales]


def sub_check(book, panel, d, sessions, rows, sub, sub_arr, note):
    lead = lambda rr, keep=None: leading_vector(corr_matrix(columns(rr, keep)))
    u = lead(rows)
    T = len(rows)
    ranked_cols = [avg_ranks(c) for c in columns(rows)]
    ranked = [[ranked_cols[i][t] for i in range(N)] for t in range(T)]
    scales = positive_row_scales(rows)
    removed = [[v / s for v in r] for r, s in zip(rows, scales)]
    state, raw = panel.rows(sessions, raw=True)
    results = {}
    for name, cand in (("pearson_spearman", ranked), ("primary_common_scale_removed", removed),
                       ("adjusted_raw", raw if state == "ok" else None)):
        if cand is None:
            book.skip(f"FIELD-SUB01 {name}", "raw returns not comparable")
            continue
        results[name] = compare_rank_one(u, lead(cand))
    trailing = {}
    for L in (63, 126, 189, 249, 250):
        trailing[L] = compare_rank_one(u, lead(rows[T - L:]))
        results[f"trailing.L{L}"] = trailing[L]
    ends = list(range(126, T + 1, 21))
    if ends[-1] != T:
        ends.append(T)
    vecs = [lead(rows[e - 126:e]) for e in ends]
    rolling = [compare_rank_one(vecs[k], vecs[k + 1]) for k in range(len(ends) - 1)]
    leave = []
    start = 0
    for g, (_, size) in enumerate(GROUPS):
        keep = [i for i in range(N) if not (start <= i < start + size)]
        start += size
        cand = lead(rows, keep)
        restricted = [u[i] for i in keep]
        norm = math.sqrt(dot(restricted, restricted))
        leave.append(compare_rank_one([x / norm for x in restricted], cand))
    for name, (ov, ang) in results.items():
        judge_coordinate(book, "FIELD-SUB01", f"{name}.overlap", sub, ov, CORR_TOL, {"date": d})
        judge_coordinate(book, "FIELD-SUB01", f"{name}.maximum_principal_angle_deg", sub, ang, 1.5, {"date": d})
    judge_coordinate(book, "FIELD-SUB01", "trailing.minimum_overlap", sub, min(o for o, _ in trailing.values()), CORR_TOL, {"date": d})
    judge_coordinate(book, "FIELD-SUB01", "rolling126.minimum_adjacent_overlap", sub, min(o for o, _ in rolling), CORR_TOL, {"date": d})
    judge_coordinate(book, "FIELD-SUB01", "leave_group_out.minimum_overlap", sub, min(o for o, _ in leave), CORR_TOL, {"date": d})
    for role, values, tol in (("rolling126.overlap", [o for o, _ in rolling], CORR_TOL),
                              ("rolling126.maximum_principal_angle_deg", [a for _, a in rolling], 1.5),
                              ("leave_group_out.overlap", [o for o, _ in leave], CORR_TOL),
                              ("leave_group_out.maximum_principal_angle_deg", [a for _, a in leave], 1.5)):
        arr = sub_arr.get(role)
        if not arr or len(arr["cells"]) != len(values):
            book.failures.append({"quantity": f"FIELD-SUB01 {role}", "date": d, "cause": "array shape differs",
                                  "stored": arr and len(arr["cells"]), "tiingo": len(values)})
            continue
        for k, cell in enumerate(arr["cells"]):
            if cell["state"] == "SUPPORTED":
                book.judge(f"FIELD-SUB01 {role}", {"date": d, "index": k}, cell["value"], values[k], tol)
    P = sub_arr.get("base_projector_k1")
    if P and P["cells"][0]["state"] == "SUPPORTED":
        vPv = math.fsum(u[a] * P["cells"][a * N + b]["value"] * u[b] for a in range(N) for b in range(N))
        q = book.q["FIELD-SUB01 base_projector_k1 (v'Pv >= 0.99)"]
        q["compared"] += 1
        q["max_abs_delta"] = max(q["max_abs_delta"], 1 - vPv)
        if vPv < 0.99:
            q["failed"] += 1
            book.failures.append({"quantity": "FIELD-SUB01 base_projector_k1", "date": d, "vPv": vPv})
        note["sub01_vPv"] = vPv


def field_main(cache, path):
    book, notes, bars, builds = field_check(cache, path)
    report = {
        "check": "Tiingo recompute of FIELD (tools/external/README.md, 'Tiingo daily prices', check 1)",
        "declared_in": "0663e30", "dates": sorted(notes), "per_date": notes,
        "tiingo_files": bars.files, "summary": book.summary(),
        "reported_not_judged": dict(book.reported), "not_comparable": dict(book.not_comparable),
        "failures": book.failures, "pass": book.passed(),
    }
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if report["pass"] else 1



# ---------------------------------------------------------------- check 2: prices (ta.rs port)

class Bar:
    __slots__ = ("date", "open", "high", "low", "close", "volume", "dividend", "split_ratio")

    def __init__(self, date, o, h, l, c, v, div=0.0, split=1.0):
        self.date, self.open, self.high, self.low, self.close, self.volume = date, o, h, l, c, v
        self.dividend, self.split_ratio = div, split


def back_adjust(raw):
    n = len(raw)
    pfs, vfs = [1.0] * n, [1.0] * n
    pf = vf = 1.0
    for i in range(n - 1, -1, -1):
        pfs[i], vfs[i] = pf, vf
        r = raw[i]
        if i > 0:
            prior = raw[i - 1].close
            if r.dividend > 0 and prior > 0 and r.dividend < prior:
                pf *= 1.0 - r.dividend / prior
            if r.split_ratio > 0 and r.split_ratio != 1.0:
                pf *= r.split_ratio
                vf /= r.split_ratio
    return [Bar(r.date, r.open * p, r.high * p, r.low * p, r.close * p, r.volume * v)
            for r, p, v in zip(raw, pfs, vfs)]


def sma(x, n):
    return sum(x[len(x) - n:]) / n if 0 < n <= len(x) else None


def ema_series(x, n):
    out = [None] * len(x)
    if n == 0 or len(x) < n:
        return out
    alpha = 2.0 / (n + 1.0)
    e = sum(x[:n]) / n
    out[n - 1] = e
    for i in range(n, len(x)):
        e += alpha * (x[i] - e)
        out[i] = e
    return out


def wilder_series(x, n):
    out = [None] * len(x)
    if n == 0 or len(x) < n:
        return out
    s = sum(x[:n]) / n
    out[n - 1] = s
    for i in range(n, len(x)):
        s += (x[i] - s) / n
        out[i] = s
    return out


def rsi_series(closes, n):
    out = [None] * len(closes)
    if len(closes) <= n:
        return out
    ch = [b - a for a, b in zip(closes, closes[1:])]
    g = wilder_series([max(c, 0.0) for c in ch], n)
    l = wilder_series([max(-c, 0.0) for c in ch], n)
    for i in range(len(ch)):
        if g[i] is not None and l[i] is not None:
            out[i + 1] = (100.0 if g[i] > 0 else None) if l[i] == 0 else 100.0 - 100.0 / (1.0 + g[i] / l[i])
    return out


def macd(closes):
    e12, e26 = ema_series(closes, 12), ema_series(closes, 26)
    line = [a - b for a, b in zip(e12, e26) if a is not None and b is not None]
    if not line:
        return None
    sig = ema_series(line, 9)
    if not sig or sig[-1] is None:
        return None
    return line[-1], sig[-1], line[-1] - sig[-1]


def bollinger(closes, n, k):
    if len(closes) < n:
        return None
    w = closes[len(closes) - n:]
    m = sum(w) / n
    sd = math.sqrt(sum((x - m) ** 2 for x in w) / n)
    up, lo = m + k * sd, m - k * sd
    width = up - lo
    if not width > 0:
        return None
    return {"middle": m, "upper": up, "lower": lo, "percent_b": (closes[-1] - lo) / width,
            "bandwidth": width / m if m > 0 else None}


def stochastic(bars, n):
    if n < 1 or len(bars) < n:
        return None
    raw = []
    for i in range(n - 1, len(bars)):
        w = bars[i + 1 - n:i + 1]
        hh, ll = max(b.high for b in w), min(b.low for b in w)
        raw.append(100.0 * (bars[i].close - ll) / (hh - ll) if hh > ll else None)
    slow = []
    for i in range(2, len(raw)):
        slow.append((raw[i] + raw[i - 1] + raw[i - 2]) / 3.0 if None not in (raw[i], raw[i - 1], raw[i - 2]) else None)
    if len(slow) < 3 or None in slow[-3:]:
        return None
    last = slow[-3:]
    return last[2], sum(last) / 3.0


def cci(bars, n):
    tp = [(b.high + b.low + b.close) / 3.0 for b in bars]
    if len(tp) < n:
        return None
    w = tp[len(tp) - n:]
    m = sum(w) / n
    mad = sum(abs(x - m) for x in w) / n
    return (tp[-1] - m) / (0.015 * mad) if mad > 0 else None


def true_ranges(bars):
    return [max(b.high - b.low, abs(b.high - a.close), abs(b.low - a.close)) for a, b in zip(bars, bars[1:])]


def atr(bars, n):
    s = wilder_series(true_ranges(bars), n)
    return s[-1] if s else None


def adx(bars, n):
    tr = true_ranges(bars)
    pdm, mdm = [], []
    for a, b in zip(bars, bars[1:]):
        up, down = b.high - a.high, a.low - b.low
        pdm.append(up if up > down and up > 0 else 0.0)
        mdm.append(down if down > up and down > 0 else 0.0)
    st, sp, sm = wilder_series(tr, n), wilder_series(pdm, n), wilder_series(mdm, n)
    dx, pdi, mdi = [], 0.0, 0.0
    for i in range(len(tr)):
        if st[i] is not None and sp[i] is not None and sm[i] is not None:
            if st[i] <= 0:
                continue
            pdi, mdi = 100.0 * sp[i] / st[i], 100.0 * sm[i] / st[i]
            s = pdi + mdi
            dx.append(100.0 * abs(pdi - mdi) / s if s > 0 else 0.0)
    w = wilder_series(dx, n)
    if not w or w[-1] is None:
        return None
    return w[-1], pdi, mdi


def pivots(b):
    p = (b.high + b.low + b.close) / 3.0
    r = b.high - b.low
    return {"s3": b.low - 2.0 * (b.high - p), "s2": p - r, "s1": 2.0 * p - b.high, "pivot": p,
            "r1": 2.0 * p - b.low, "r2": p + r, "r3": b.high + 2.0 * (p - b.low)}


def fibonacci(bars):
    hi_i = max(range(len(bars)), key=lambda i: (bars[i].high, i))
    lo_i = min(range(len(bars)), key=lambda i: (bars[i].low, i))
    # Rust max_by keeps the last maximum, min_by the first minimum
    hi, lo = bars[hi_i].high, bars[lo_i].low
    rally = hi_i > lo_i
    lvl = lambda f: hi - f * (hi - lo) if rally else lo + f * (hi - lo)
    return hi, lo, rally, {"23.6": lvl(0.236), "38.2": lvl(0.382), "50.0": lvl(0.5), "61.8": lvl(0.618), "78.6": lvl(0.786)}


def beta_corr(a, m):
    ra = [math.log(y / x) for x, y in zip(a, a[1:])]
    rm = [math.log(y / x) for x, y in zip(m, m[1:])]
    n = len(ra)
    if n < 20 or n != len(rm):
        return None
    ma, mm = sum(ra) / n, sum(rm) / n
    cov = sum((x - ma) * (y - mm) for x, y in zip(ra, rm)) / n
    va, vm = sum((x - ma) ** 2 for x in ra) / n, sum((y - mm) ** 2 for y in rm) / n
    return (cov / vm, cov / math.sqrt(va * vm)) if vm > 0 and va > 0 else None


def obv(bars):
    out, v = [], 0.0
    for i, b in enumerate(bars):
        if i > 0:
            p = bars[i - 1].close
            v += b.volume if b.close > p else (-b.volume if b.close < p else 0.0)
        out.append(v)
    return out


def cmf(bars, n):
    if len(bars) < n:
        return None
    mfv = vol = 0.0
    for b in bars[len(bars) - n:]:
        r = b.high - b.low
        if r > 0:
            mfv += ((b.close - b.low) - (b.high - b.close)) / r * b.volume
        vol += b.volume
    return mfv / vol if vol > 0 else None


def mfi(bars, n):
    if len(bars) < n + 1:
        return None
    tp = lambda b: (b.high + b.low + b.close) / 3.0
    pos = neg = 0.0
    for i in range(len(bars) - n, len(bars)):
        a, b = tp(bars[i - 1]), tp(bars[i])
        flow = b * bars[i].volume
        if abs(b - a) <= 1e-9 * max(abs(a), abs(b)):
            continue
        if b > a:
            pos += flow
        elif b < a:
            neg += flow
    return (100.0 if pos > 0 else None) if neg == 0 else 100.0 - 100.0 / (1.0 + pos / neg)


def vpt(bars):
    out, v = [], 0.0
    for i, b in enumerate(bars):
        if i > 0 and bars[i - 1].close > 0:
            v += b.volume * (b.close / bars[i - 1].close - 1.0)
        out.append(v)
    return out


def vwma(bars, n):
    if len(bars) < n:
        return None
    w = bars[len(bars) - n:]
    vol = sum(b.volume for b in w)
    return sum(b.close * b.volume for b in w) / vol if vol > 0 else None


def volume_oscillator(bars, fast, slow):
    v = [b.volume for b in bars]
    f, s = ema_series(v, fast), ema_series(v, slow)
    if not f or f[-1] is None or s[-1] is None:
        return None
    return f[-1] / s[-1] - 1.0 if s[-1] > 0 else None


def geometry(bars):
    if len(bars) < 7:
        return None
    t, p = bars[-1], bars[-2]
    r = t.high - t.low
    return (t.high < p.high and t.low > p.low, t.high > p.high and t.low < p.low,
            all(r < b.high - b.low for b in bars[-7:-1]), t.open / p.close - 1.0)


def streak(xs, f):
    k = 0
    for x in reversed(xs):
        if x is None or not f(x):
            break
        k += 1
    return k


def parabolic_sar(bars):
    if len(bars) < 3:
        return None
    long = bars[1].close >= bars[0].close
    sar = bars[0].low if long else bars[0].high
    ep = bars[1].high if long else bars[1].low
    af = 0.02
    for i in range(2, len(bars)):
        sar += af * (ep - sar)
        b = bars[i]
        if long:
            sar = min(sar, bars[i - 1].low, bars[i - 2].low)
            if b.low < sar:
                long, sar, ep, af = False, max(ep, b.high, bars[i - 1].high), b.low, 0.02
            elif b.high > ep:
                ep, af = b.high, min(af + 0.02, 0.2)
        else:
            sar = max(sar, bars[i - 1].high, bars[i - 2].high)
            if b.high > sar:
                long, sar, ep, af = True, min(ep, b.low, bars[i - 1].low), b.high, 0.02
            elif b.low < ep:
                ep, af = b.low, min(af + 0.02, 0.2)
    return sar, long


def williams_r(bars, n):
    if len(bars) < n:
        return None
    w = bars[len(bars) - n:]
    hh, ll = max(b.high for b in w), min(b.low for b in w)
    return -100.0 * (hh - bars[-1].close) / (hh - ll) if hh > ll else None


def ichimoku(bars):
    n = len(bars)
    if n < 52 + 26:
        return None
    mid = lambda w: (max(b.high for b in w) + min(b.low for b in w)) / 2.0
    at = lambda end, k: mid(bars[end + 1 - k:end + 1])
    past = n - 1 - 26
    return {"tenkan": at(n - 1, 9), "kijun": at(n - 1, 26), "span_a": (at(past, 9) + at(past, 26)) / 2.0,
            "span_b": at(past, 52)}


def stoch_rsi(closes):
    allr = rsi_series(closes, 14)
    if len(allr) < 18 or None in allr[-18:]:
        return None
    rsi = allr[-18:]
    raw = []
    for i in range(13, len(rsi)):
        w = rsi[i - 13:i + 1]
        hi, lo = max(w), min(w)
        if not hi > lo:
            return None
        raw.append(100.0 * (rsi[i] - lo) / (hi - lo))
    k = [(raw[i] + raw[i - 1] + raw[i - 2]) / 3.0 for i in range(2, len(raw))]
    return k[-1], sma(k, 3)


def keltner(bars):
    e = ema_series([b.close for b in bars], 20)
    a = atr(bars, 10)
    if not e or e[-1] is None or a is None:
        return None
    return e[-1] + 2.0 * a, e[-1], e[-1] - 2.0 * a


def donchian(bars, n):
    if len(bars) < n:
        return None
    w = bars[len(bars) - n:]
    hi, lo = max(b.high for b in w), min(b.low for b in w)
    return hi, (hi + lo) / 2.0, lo


def regression_channel(closes, n):
    if len(closes) < n:
        return None
    y = closes[len(closes) - n:]
    xm = (n - 1.0) / 2.0
    ym = sum(y) / n
    sxx = sum((i - xm) ** 2 for i in range(n))
    sxy = sum((i - xm) * (y[i] - ym) for i in range(n))
    b = sxy / sxx
    a = ym - b * xm
    fit = a + b * (n - 1.0)
    sd = math.sqrt(sum((y[i] - (a + b * i)) ** 2 for i in range(n)) / n)
    return (fit, b / fit, sd) if fit > 0 else None


def technical_records(raw, spy, pair):
    """records() of technical.rs for TECHNICAL, PRICE-LEVELS, DRAWDOWN, RELATIVE-PERFORMANCE, BETA:
    {measurement: {role: value or None}}. `spy`: [(date, back-adjusted SPY close)]; `pair`:
    (rho, sessions) of the recomputed FIELD-DEP01 SPY pair, or None."""
    bars = back_adjust(raw)
    closes = [b.close for b in bars]
    last = closes[-1]
    out = {}
    rsi = rsi_series(closes, 14)
    rsi_now = rsi[-1]
    hist = [x for x in list(reversed(rsi))[1:253] if x is not None]
    t = {"rsi_14": rsi_now}
    t["rsi_14.percentile_252"] = (rsi_now, hist) if rsi_now is not None and len(hist) >= 126 else None
    s50, s200 = sma(closes, 50), sma(closes, 200)
    for k in (20, 50, 100, 200):
        t[f"sma_{k}"] = sma(closes, k)
    for k in (12, 26):
        t[f"ema_{k}"] = ema_series(closes, k)[-1]
    t["distance_sma_50"] = last / s50 - 1.0 if s50 else None
    t["distance_sma_200"] = last / s200 - 1.0 if s200 else None
    m = macd(closes)
    t["macd"], t["macd_signal"], t["macd_histogram"] = m if m else (None, None, None)
    bo = bollinger(closes, 20, 2.0) or {}
    for k in ("upper", "middle", "lower", "percent_b", "bandwidth"):
        t[f"bollinger_{k}"] = bo.get(k)
    st = stochastic(bars, 14)
    t["stochastic_k"], t["stochastic_d"] = st if st else (None, None)
    t["cci_20"] = cci(bars, 20)
    ad = adx(bars, 14)
    t["adx_14"], t["plus_di_14"], t["minus_di_14"] = ad if ad else (None, None, None)
    a14 = atr(bars, 14)
    t["atr_14"] = a14
    t["atr_14_percent"] = a14 / last if a14 is not None else None
    t["rate_of_change_10"] = last / closes[-11] - 1.0 if len(closes) > 10 else None
    vstart = 0
    vb = bars[vstart:]
    ob, vp = obv(vb), vpt(vb)
    back = lambda v, k: v[-1] - v[-1 - k] if len(v) > k else None
    t["obv_change_20"] = back(ob, 20)
    t["chaikin_money_flow_20"] = cmf(vb, 20)
    t["money_flow_index_14"] = mfi(vb, 14)
    t["volume_price_trend_change_20"] = back(vp, 20)
    vw = vwma(vb, 20)
    t["vwma_20"] = vw
    t["distance_vwma_20"] = last / vw - 1.0 if vw else None
    t["volume_oscillator_5_20"] = volume_oscillator(vb, 5, 20)
    g = geometry(bars)
    t["gap"] = g[3] if g else None
    yes = lambda b: "YES" if b else "NO"
    t["inside_bar"], t["outside_bar"], t["narrow_range_7"] = (yes(g[0]), yes(g[1]), yes(g[2])) if g else ("UNAVAILABLE",) * 3
    cond = lambda x: "OVERBOUGHT_BY_CONVENTION" if x >= 70 else ("OVERSOLD_BY_CONVENTION" if x <= 30 else "NEUTRAL")
    t["rsi_14.condition"] = cond(rsi_now) if rsi_now is not None else "UNAVAILABLE"
    mf = t["money_flow_index_14"]
    t["money_flow_index_14.condition"] = ("OVERBOUGHT_BY_CONVENTION" if mf >= 80 else ("OVERSOLD_BY_CONVENTION" if mf <= 20 else "NEUTRAL")) if mf is not None else "UNAVAILABLE"
    t["sma_50_200_state"] = ("SMA50_ABOVE_SMA200" if s50 > s200 else "SMA50_BELOW_SMA200") if s50 is not None and s200 is not None else "UNAVAILABLE"
    above200 = [closes[i] > sum(closes[i - 199:i + 1]) / 200.0 if i >= 199 else None for i in range(len(closes))]
    rs = lambda x: 1 if x >= 70 else (-1 if x <= 30 else 0)
    t["rsi_14.condition_sessions"] = streak(rsi, lambda v: rs(v) == rs(rsi_now)) if rsi_now is not None else None
    an = above200[-1]
    t["price_vs_sma_200"] = {True: "ABOVE", False: "BELOW", None: "UNAVAILABLE"}[an]
    t["price_vs_sma_200.sessions"] = streak(above200, lambda v: v == an) if an is not None else None
    sar = parabolic_sar(bars)
    t["parabolic_sar"] = sar[0] if sar else None
    t["parabolic_sar.trend"] = ("UP" if sar[1] else "DOWN") if sar else "UNAVAILABLE"
    t["williams_r_14"] = williams_r(bars, 14)
    ich = ichimoku(bars)
    for k, name in (("tenkan", "tenkan_9"), ("kijun", "kijun_26"), ("span_a", "span_a"), ("span_b", "span_b")):
        t[f"ichimoku.{name}"] = ich[k] if ich else None
    if ich:
        top, bottom = max(ich["span_a"], ich["span_b"]), min(ich["span_a"], ich["span_b"])
        t["ichimoku.price_vs_cloud"] = "ABOVE_CLOUD" if last > top else ("BELOW_CLOUD" if last < bottom else "INSIDE_CLOUD")
        t["ichimoku.tenkan_vs_kijun"] = "TENKAN_ABOVE" if ich["tenkan"] > ich["kijun"] else ("TENKAN_BELOW" if ich["tenkan"] < ich["kijun"] else "EQUAL")
    sr = stoch_rsi(closes)
    t["stoch_rsi_k"], t["stoch_rsi_d"] = sr if sr else (None, None)
    ke = keltner(bars)
    t["keltner_upper"], t["keltner_middle"], t["keltner_lower"] = ke if ke else (None,) * 3
    do = donchian(bars, 20)
    t["donchian_upper_20"], t["donchian_middle_20"], t["donchian_lower_20"] = do if do else (None,) * 3
    rg = regression_channel(closes, 50)
    if rg:
        t["regression_channel_50.middle"] = rg[0]
        t["regression_channel_50.upper"] = rg[0] + 2.0 * rg[2]
        t["regression_channel_50.lower"] = rg[0] - 2.0 * rg[2]
        t["regression_channel_50.slope_per_session"] = rg[1]
    cross = None
    if len(closes) > 200:
        prev = None
        for i in range(199, len(closes)):
            a = sum(closes[i - 49:i + 1]) / 50.0
            b = sum(closes[i - 199:i + 1]) / 200.0
            above = a > b
            if prev is not None and prev != above:
                cross = (raw[i].date, "GOLDEN_CROSS" if above else "DEATH_CROSS")
            prev = above
    t["last_sma_50_200_cross"] = cross[1] if cross else "NONE_IN_HISTORY"
    if cross:
        t["last_sma_50_200_cross_date"] = cross[0]
    out["DLM-TECHNICAL"] = t

    p = {}
    for k, v in pivots(bars[-1]).items():
        p[f"pivot.{k}"] = v
    for w in (63, 252):
        if len(bars) >= w:
            hi, lo, rally, lv = fibonacci(bars[-w:])
            p[f"fibonacci_{w}.swing_high"], p[f"fibonacci_{w}.swing_low"] = hi, lo
            p[f"fibonacci_{w}.direction"] = "RETRACING_A_RALLY" if rally else "RETRACING_A_DECLINE"
            for k, v in lv.items():
                p[f"fibonacci_{w}.level_{k}"] = v
    if len(bars) >= 252:
        y = bars[-252:]
        h52, l52 = max(b.high for b in y), min(b.low for b in y)
        p["week_52_high"], p["week_52_low"] = h52, l52
        p["distance_from_52_week_high"] = last / h52 - 1.0
        p["position_in_52_week_range"] = (last - l52) / (h52 - l52) if h52 > l52 else None
    out["DLM-PRICE-LEVELS"] = p

    b = {}
    if spy is not None:
        mine = {r.date: c for r, c in zip(raw, closes)}
        joint = [(mine[d], c) for d, c in spy if d in mine]
        j = joint[-64:]
        bc = beta_corr([x for x, _ in j], [y for _, y in j]) if len(j) == 64 else None
        b["beta_63"], b["correlation_63"] = bc if bc else (None, None)

        def returns(dated):
            res, prev = {}, None
            for d, c in dated:
                if prev is not None and c is not None:
                    res[d] = math.log(c / prev)
                prev = c
            return res
        ra = returns([(r.date, c) for r, c in zip(raw, closes)])
        rm = returns(spy)
        if pair is None:
            b["beta_1y"] = b["correlation_1y"] = None
        else:
            rho, sessions = pair
            if rho is None or any(s not in ra or s not in rm for s in sessions):
                b["beta_1y"] = b["correlation_1y"] = None
            else:
                def sd(v):
                    m_ = sum(v) / len(v)
                    return math.sqrt(sum((x - m_) ** 2 for x in v) / len(v))
                sa, sm = sd([ra[s] for s in sessions]), sd([rm[s] for s in sessions])
                b["beta_1y"], b["correlation_1y"] = (rho * sa / sm, rho) if sm > 0 else (None, None)
                b["one_year_sessions"] = len(sessions)
    out["DLM-BETA"] = b

    rel = {}
    spy_close = dict(spy) if spy is not None else None
    ret = lambda k: closes[-1] / closes[-1 - k] - 1.0 if len(closes) > k and closes[-1 - k] > 0 else None

    def spy_ret(k):
        if spy_close is None or len(raw) <= k:
            return None
        now, then = spy_close.get(raw[-1].date), spy_close.get(raw[-1 - k].date)
        return now / then - 1.0 if now is not None and then and then > 0 else None
    for k, label in ((21, "1m"), (63, "3m"), (126, "6m"), (252, "1y")):
        a_, s_ = ret(k), spy_ret(k)
        rel[f"return_{label}"], rel[f"spy_return_{label}"] = a_, s_
        rel[f"excess_vs_spy_{label}"] = a_ - s_ if a_ is not None and s_ is not None else None
    out["DLM-RELATIVE-PERFORMANCE"] = rel

    dd = {}
    if len(closes) >= 253:
        w = closes[-253:]
        peak, peak_i, max_dd = w[0], 0, 0.0
        for i, v in enumerate(w):
            if v >= peak:
                peak, peak_i = v, i
            max_dd = min(max_dd, v / peak - 1.0)
        rets = [y / x - 1.0 for x, y in zip(w, w[1:])]
        down = math.sqrt(sum(min(r, 0.0) ** 2 for r in rets) / len(rets)) * math.sqrt(252.0)
        worst = lambda k: min(w[i + k] / w[i] - 1.0 for i in range(len(w) - k))
        cur = last / peak - 1.0
        dd.update(current_drawdown=cur, max_drawdown_1y=max_dd, sessions_since_1y_high=len(w) - 1 - peak_i,
                  at_1y_high="YES" if cur >= 0 else "NO", downside_deviation_1y=down,
                  worst_1_session_return=worst(1), worst_5_session_return=worst(5), worst_21_session_return=worst(21))
    out["DLM-DRAWDOWN"] = dd
    for rec in out.values():
        rec["price_history_sessions"] = len(bars)
    return out


USD = {"sma_20", "sma_50", "sma_100", "sma_200", "ema_12", "ema_26", "macd", "macd_signal", "macd_histogram",
       "bollinger_upper", "bollinger_middle", "bollinger_lower", "atr_14", "parabolic_sar", "ichimoku.tenkan_9",
       "ichimoku.kijun_26", "ichimoku.span_a", "ichimoku.span_b", "keltner_upper", "keltner_middle",
       "keltner_lower", "donchian_upper_20", "donchian_middle_20", "donchian_lower_20",
       "regression_channel_50.middle", "regression_channel_50.upper", "regression_channel_50.lower",
       "week_52_high", "week_52_low"}
OSC = {"rsi_14", "stochastic_k", "stochastic_d", "adx_14", "plus_di_14", "minus_di_14", "williams_r_14",
       "stoch_rsi_k", "stoch_rsi_d"}
RETURNS = {"distance_sma_50", "distance_sma_200", "distance_from_52_week_high", "rate_of_change_10", "gap",
           "atr_14_percent", "bollinger_bandwidth", "current_drawdown", "max_drawdown_1y",
           "worst_1_session_return", "worst_5_session_return", "worst_21_session_return"}
VOLUME = {"obv_change_20", "chaikin_money_flow_20", "money_flow_index_14", "volume_price_trend_change_20",
          "vwma_20", "distance_vwma_20", "volume_oscillator_5_20"}


def dlm_rule(role, v):
    """('judge', tol) | ('percentile', level_tol) | ('report', closeness or None) | None (not compared)."""
    if role in ("definition", "represented_session", "recent_adjustments"):
        return None
    if role in VOLUME:
        return ("report", 0.01 * abs(v) if isinstance(v, (int, float)) else None)
    if role == "rsi_14.percentile_252":
        return ("percentile", 1.0)
    if role in USD or role.startswith("pivot.") or (role.startswith("fibonacci_") and not role.endswith("direction")):
        return ("judge", 0.01 + 1e-3 * abs(v))
    if role in OSC:
        return ("judge", 1.0)
    if role == "cci_20":
        return ("judge", 2.0 + 0.02 * abs(v))
    if role in RETURNS or role.split("_")[0] in ("return", "spy", "excess"):
        return ("judge", 1e-3 + 1e-3 * abs(v))
    if role in ("bollinger_percent_b", "position_in_52_week_range"):
        return ("judge", 0.01)
    if role == "downside_deviation_1y":
        return ("judge", 0.01 * abs(v))
    if role == "regression_channel_50.slope_per_session":
        return ("judge", 1e-5 + 0.01 * abs(v))
    if role.startswith("correlation_"):
        return ("judge", CORR_TOL)
    if role.startswith("beta_"):
        return ("judge", 0.01 + 0.01 * abs(v))
    return ("report", None)


P03_ROLES = ("CC", "PARKINSON", "GARMAN_KLASS", "ROGERS_SATCHELL", "YANG_ZHANG")


def var1(v):
    m = mean(v)
    return math.fsum((x - m) ** 2 for x in v) / (len(v) - 1)


def p03(bars, t, T, w):
    sessions = bars.calendar[bars.index[T] - w:bars.index[T] + 1]
    rows = [bars.bar(t, s) for s in sessions]
    if None in rows:
        return None
    cc, pk, gk, rs, op, intra = [], [], [], [], [], []
    ln2 = math.log(2.0)
    for k in range(1, len(rows)):
        o, h, l, c = rows[k][:4]
        pc = rows[k - 1][3] * bars.factor(t, sessions[k])
        ci, oi, di, hi = math.log(c / pc), math.log(o / pc), math.log(c / o), math.log(h / l)
        cc.append(ci * ci)
        pk.append(hi * hi / (4 * ln2))
        gk.append(0.5 * hi * hi - (2 * ln2 - 1) * di * di)
        rs.append(math.log(h / o) * math.log(h / c) + math.log(l / o) * math.log(l / c))
        op.append(oi)
        intra.append(di)
    kk = 0.34 / (1.34 + (w + 1.0) / (w - 1.0))
    est = [mean(cc), mean(pk), mean(gk), mean(rs), var1(op) + kk * var1(intra) + (1 - kk) * mean(rs)]
    return {name: math.sqrt(252.0 * v) if v >= 0 else None for name, v in zip(P03_ROLES, est)}


def read_prices(path):
    out = defaultdict(dict)
    with open(path) as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line:
                continue
            d, obj, mid, build, start, roles = line.split("\t", 5)
            out[(d, obj)][mid] = (build, start, {r["role_id"]: r for r in json.loads(roles)})
    return out


def rv(role):
    v = role.get("value")
    return v.get("data") if isinstance(v, dict) and "data" in v else v


def prices_check(cache, prices_path, field_path):
    bars = Bars(Path(cache))
    field, _ = read_field(field_path)
    unsupported, _ = withheld_cells(field, bars)
    panel = Panel(bars, unsupported)
    recs = read_prices(prices_path)
    book = Book()
    seen_close, seen_r1 = {}, {}
    builds = {}
    spy_cache = {}
    for (d, obj), mids in sorted(recs.items()):
        where = {"date": d, "object_id": obj}
        iT = bars.index[d]
        tb = bars.by[obj]
        # 1-2. VS-P01 inputs
        if "VS-P01" in mids:
            build, _, roles = mids["VS-P01"]
            builds[d] = build
            close = tb.get(d, (None,) * 4)[3]
            ctol = lambda c: max(0.005, 1e-4 * abs(c))
            for rid, val, tol in (("raw_close", close, None), ("adjusted_close", close, None),
                                  ("raw_r1", bars.raw_return(obj, d), RET_TOL),
                                  ("adjusted_r1", bars.adj_return(obj, d), RET_TOL)):
                r = roles.get(rid)
                q = f"VS-P01 {rid}"
                if not r or r["state"] != "SUPPORTED":
                    book.skip(q, f"stored {r and r['state']} {r and r.get('reason')}")
                    continue
                if val is None:
                    book.skip(q, "Tiingo lacks the bar")
                    continue
                book.judge(q, where, rv(r), val, ctol(rv(r)) if tol is None else tol)
            r = roles.get("adjusted_r5")
            sess = bars.calendar[iT - 4:iT + 1]
            r5 = [bars.adj_return(obj, s) for s in sess]
            if r and r["state"] == "SUPPORTED":
                if None in r5:
                    book.skip("VS-P01 adjusted_r5", "Tiingo lacks a bar")
                else:
                    book.judge("VS-P01 adjusted_r5", where, rv(r), math.fsum(r5), RET_TOL)
            for rid, fn, seen, tolf in (("raw_close.history", lambda s: tb.get(s, (None,) * 4)[3], seen_close, ctol),
                                        ("adjusted_r1.history", lambda s: bars.adj_return(obj, s), seen_r1, lambda c: RET_TOL)):
                r = roles.get(rid)
                q = f"VS-P01 {rid} (one per security-session)"
                if not r or r["state"] != "SUPPORTED":
                    book.skip(q, f"stored {r and r['state']}")
                    continue
                for cell in rv(r):
                    if cell["state"] != "SUPPORTED":
                        book.skip(q, f"cell {cell['state']} {cell.get('reason')}")
                        continue
                    key = (obj, cell["session"])
                    if key in seen:
                        if seen[key] != cell["value"]:
                            listed = book.listed.setdefault(f"{q}: a session stated differently by two bodies",
                                                            {"count": 0, "max_abs_difference": 0.0, "examples": []})
                            listed["count"] += 1
                            listed["max_abs_difference"] = max(listed["max_abs_difference"], abs(seen[key] - cell["value"]))
                            if len(listed["examples"]) < 3:
                                listed["examples"].append({"object_id": obj, "session": cell["session"],
                                                           "values": [seen[key], cell["value"]]})
                        continue
                    seen[key] = cell["value"]
                    val = fn(cell["session"])
                    if val is None:
                        book.skip(q, "Tiingo lacks the bar")
                        continue
                    book.judge(q, {"object_id": obj, "session": cell["session"]}, cell["value"], val, tolf(cell["value"]))
        # 3. VS-P03, PATH-L01, PATH-L02
        if "VS-P03" in mids:
            roles = mids["VS-P03"][2]
            for w in (5, 10, 21, 42):
                est = p03(bars, obj, d, w)
                for name in P03_ROLES:
                    rid = f"{name}.W{w}"
                    r = roles.get(rid)
                    q = f"VS-P03 {name}.W*"
                    if not r or r["state"] != "SUPPORTED":
                        book.skip(q, f"stored {r and r['state']} {r and r.get('reason')}")
                        continue
                    if est is None:
                        book.skip(q, "Tiingo lacks a bar")
                        continue
                    rel = 0.005 if name == "CC" else 0.02
                    book.judge(q, {**where, "window": w}, rv(r), est[name], rel * abs(rv(r)))
        today, prev = tb.get(d), tb.get(bars.calendar[iT - 1])
        for mid, rid, fn, tol in (
                ("PATH-L01", "gap", lambda: math.log(today[0] / (prev[3] * bars.factor(obj, d))), RET_TOL),
                ("PATH-L01", "intraday", lambda: math.log(today[3] / today[0]), RET_TOL),
                ("PATH-L02", "raw_log_high_low_range", lambda: math.log(today[1] / today[2]), 1e-3)):
            if mid not in mids:
                continue
            r = mids[mid][2].get(rid)
            q = f"{mid} {rid}"
            if not r or r["state"] != "SUPPORTED":
                book.skip(q, f"stored {r and r['state']} {r and r.get('reason')}")
                continue
            if today is None or prev is None:
                book.skip(q, "Tiingo lacks the bar")
                continue
            book.judge(q, where, rv(r), fn(), tol)
        # 4. DLM records
        if "DLM-TECHNICAL" not in mids:
            continue
        start = mids["DLM-TECHNICAL"][1]
        raw = history(bars, obj, start, d)
        if raw is None:
            for mid in ("DLM-TECHNICAL", "DLM-PRICE-LEVELS", "DLM-DRAWDOWN", "DLM-RELATIVE-PERFORMANCE", "DLM-BETA"):
                book.skip(f"{mid} (all)", "Tiingo lacks a session of the bar history")
            continue
        if d not in spy_cache:
            spy_start = recs[(d, "SPY")]["DLM-TECHNICAL"][1]
            spy_raw = history(bars, "SPY", spy_start, d)
            spy_cache[d] = list(zip([r.date for r in spy_raw], [b.close for b in back_adjust(spy_raw)])) if spy_raw else None
        pair = spy_pair(panel, bars, d, obj)
        mine = technical_records(raw, spy_cache[d], pair)
        for mid, values in mine.items():
            if mid not in mids:
                continue
            for rid, role in mids[mid][2].items():
                q = f"{mid} {rid}"
                if role["state"] != "SUPPORTED":
                    if rid not in ("definition", "represented_session", "recent_adjustments"):
                        book.skip(q, f"stored {role['state']} {role.get('reason')}")
                    continue
                stored = rv(role)
                rule = dlm_rule(rid, stored)
                if rule is None:
                    continue
                kind, tol = rule
                val = values.get(rid)
                if kind == "report":
                    book.report_only(q, where, stored, val, tol)
                elif kind == "percentile":
                    if val is None:
                        book.judge(q, where, stored, None, 0)
                    else:
                        cur, hist = val
                        book.judge(q, where, stored, midrank(cur, hist), percentile_tolerance(cur, hist, tol))
                else:
                    book.judge(q, where, stored, val, tol)
    return book, builds, bars


def history(bars, t, start, end):
    i0, i1 = bars.index.get(start), bars.index[end]
    if i0 is None:
        return None
    out = []
    for s in bars.calendar[i0:i1 + 1]:
        b = bars.bar(t, s)
        if b is None:
            return None
        out.append(Bar(s, b[0], b[1], b[2], b[3], b[4], b[5], 1.0 / b[6]))
    return out


def spy_pair(panel, bars, d, obj):
    """The recomputed FIELD-DEP01 (SPY, obj) pair: (pearson, its common-support sessions)."""
    iT = bars.index[d]
    if obj == "SPY":
        # spy_pairs_of (vyreon-cli derive): SPY's own pair is correlation 1 over the whole window
        window = bars.calendar[max(0, iT - DEP_WINDOW + 1):iT + 1]
        return (1.0 if len(window) >= DEP_MIN else None, window)
    i = SECURITY_IDS.index(obj)
    x, y, sessions = [], [], []
    for s in bars.calendar[max(0, iT - DEP_WINDOW + 1):iT + 1]:
        (sa, va), (sb, vb) = panel.cell(0, s), panel.cell(i, s)
        if sa == "withheld" or sb == "withheld":
            continue
        if sa != "ok" or sb != "ok":
            return (None, [])
        x.append(va); y.append(vb); sessions.append(s)
    return (pearson(x, y) if len(x) >= DEP_MIN else None, sessions)


def prices_main(cache, prices_path, field_path):
    book, builds, bars = prices_check(cache, prices_path, field_path)
    report = {
        "check": "Tiingo recompute of price-based base and derived sensors (tools/external/README.md, 'Tiingo daily prices', check 2)",
        "declared_in": "0663e30", "dates": sorted(builds), "builds": builds,
        "tiingo_files": bars.files, "summary": book.summary(),
        "reported_not_judged": dict(book.reported), "listed": book.listed,
        "not_comparable": dict(book.not_comparable),
        "tool_fixes_after_the_first_run": [
            "DLM-BETA beta_1y/correlation_1y for SPY: the port first had no SPY self-pair; the engine's "
            "spy_pairs_of gives SPY correlation 1.0 over the whole 250-session window (fixed to match the "
            "engine; no tolerance changed)",
            "a session stated differently by two VS-P01 bodies (2e-16, window-basis rounding) was filed as "
            "a failure; README says such sessions are listed (moved to 'listed')"],
        "failures": book.failures, "pass": book.passed(),
    }
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "fetch":
        sys.exit(fetch(a[1], a[2]))
    if a[0] == "field":
        sys.exit(field_main(a[1], a[2]))
    if a[0] == "prices":
        sys.exit(prices_main(a[1], a[2], a[3]))
    raise SystemExit(__doc__)
