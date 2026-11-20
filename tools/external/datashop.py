"""Cboe DataShop option EOD against Intrinio's option inputs, per contract (rules in README.md,
declared first).

    python3 -I datashop.py fetch ORDER SIDE [DATE...]      # Intrinio's side, as the engine fetches it
    python3 -I datashop.py compare ORDER SIDE OUT_PREFIX    # writes OUT_PREFIX-<part>.json

ORDER is the DataShop order folder (`SHA256SUMS`, `item_*/UnderlyingOptionsEODCalcs_<date>.zip`).
SIDE holds Intrinio's filtered snapshot rows, the EOD bodies and `fetched.json` (url without query
or key, sha256 of the provider's bytes, fetch time). The Intrinio key is read from INTRINIO_KEY_FILE
(default the engine checkout's api.txt) and sent only as a Bearer header. Exit 0 on pass.
"""
from __future__ import annotations

import csv
import gzip
import hashlib
import io
import json
import os
import statistics
import sys
import threading
import time
import urllib.parse
import urllib.request
import zipfile
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

API = "https://api-v2.intrinio.com"
KEY_FILE = os.environ.get("INTRINIO_KEY_FILE", str(Path.home() / "vyreon-engine" / "api.txt"))
NY = ZoneInfo("America/New_York")
EARLY_CLOSE = {"2025-12-24"}  # NYSE/Cboe 13:00 close inside the order's months
ITEMS = {"item_000122947": ("SPY", "TSLA", "UUP"), "item_000122949": ("XLK",), "item_000122951": ("ETHA",)}
HEADER = ("CONTRACT ID,OPEN INTEREST,TRADE PRICE,TRADE SIZE,TOTAL TRADE VOLUME,LAST TRADE TIMESTAMP,"
          "TRADE HIGH PRICE,TRADE LOW PRICE,ASK PRICE,ASK SIZE,LAST ASK TIMESTAMP,BID PRICE,BID SIZE,LAST BID TIMESTAMP")
# Round cc, 12: the keyed API calls run outside core's request budget (C-025), so they stay well
# under the margin core leaves (2,000 a minute less core's ceiling, 1,800 by default): one fetch thread and at most
# one keyed call every 0.7 s (under 100 a minute, retries included). Never run during a build.
MIN_INTERVAL = 0.7
LOCK = threading.Lock()
_PACE = threading.Lock()
_last_call = 0.0


def paced() -> None:
    """Waits until MIN_INTERVAL has passed since the previous keyed call, in any thread."""
    global _last_call
    with _PACE:
        wait = _last_call + MIN_INTERVAL - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        _last_call = time.monotonic()
CAP = 200  # listed examples per category; counts are always complete


# ------------------------------------------------------------------ calendar and keys

def close_at(d: str) -> datetime:
    hh = 13 if d in EARLY_CLOSE else 16
    return datetime.fromisoformat(d).replace(hour=hh, tzinfo=NY).astimezone(timezone.utc)


def open_at(d: str) -> datetime:
    return datetime.fromisoformat(d).replace(hour=9, minute=30, tzinfo=NY).astimezone(timezone.utc)


def cboe_instant(d: str) -> datetime:
    """Cboe's 15:45 snapshot; 12:45 on an early close (layout v1.1 note 5): close - 15 min."""
    return close_at(d) - timedelta(minutes=15)


def utc(t: datetime) -> str:
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def skey(x: float) -> str:
    return f"{x:.3f}"


def osi_key(code: str):
    """Intrinio's EOD code `UUP250417C00019000` -> (root, expiration, strike, C/P)."""
    root, ymd, cp, mils = code[:-15], code[-15:-9], code[-9], code[-8:]
    if not root or cp not in "CP" or not ymd.isdigit() or not mils.isdigit():
        raise ValueError(f"EOD code {code}")
    return root, f"20{ymd[:2]}-{ymd[2:4]}-{ymd[4:]}", skey(int(mils) / 1000), cp


def snap_key(cid: str):
    """Snapshot id `SPY_250401C400.00` -> key."""
    root, rest = cid.split("_", 1)
    ymd, cp, strike = rest[:6], rest[6], rest[7:]
    if not ymd.isdigit() or cp not in "CP":
        raise ValueError(f"snapshot id {cid}")
    return root, f"20{ymd[:2]}-{ymd[2:4]}-{ymd[4:]}", skey(float(strike)), cp


def root_owner(root: str, wanted: set[str]):
    """close_quotes::root_owner: the symbol itself, or one digit after (ETHA1) or before (4SPY)."""
    if root in wanted:
        return root
    if root and root[-1].isdigit() and root[:-1] in wanted:
        return root[:-1]
    if root and root[0].isdigit() and root[1:] in wanted:
        return root[1:]
    return None


def root_kind(root: str, sym: str) -> str:
    if root == sym:
        return "STANDARD"
    if root[0].isdigit():
        return "FLEX"
    if root[-1].isdigit():
        return "ADJUSTED"
    return "OTHER"


# ------------------------------------------------------------------ the order

def order_days(order: Path) -> dict[str, tuple[str, Path]]:
    """date -> (item, zip), after checking every zip against SHA256SUMS."""
    sums = {}
    for line in (order / "SHA256SUMS").read_text().splitlines():
        if line.strip():
            h, name = line.split(None, 1)
            sums[os.path.normpath(name.strip())] = h
    out = {}
    for item in ITEMS:
        for z in sorted((order / item).glob("UnderlyingOptionsEODCalcs_*.zip")):
            rel = os.path.normpath(str(z.relative_to(order)))
            if hashlib.sha256(z.read_bytes()).hexdigest() != sums.get(rel):
                raise SystemExit(f"{rel}: differs from SHA256SUMS")
            out[z.stem.rsplit("_", 1)[1]] = (item, z)
    return out


def load_cboe(z: Path, d: str) -> dict:
    with zipfile.ZipFile(z) as zf:
        names = zf.namelist()
        if len(names) != 1:
            raise SystemExit(f"{z}: {names}")
        rows = {}
        for r in csv.DictReader(io.TextIOWrapper(zf.open(names[0]), "utf-8")):
            if r["quote_date"] != d:
                raise SystemExit(f"{z}: quote_date {r['quote_date']}")
            k = (r["root"], r["expiration"], skey(float(r["strike"])), r["option_type"])
            if k in rows:
                raise SystemExit(f"{z}: duplicate {k}")
            rows[k] = r
    return rows


# ------------------------------------------------------------------ fetch

def key() -> str:
    return Path(KEY_FILE).read_text().strip()


def api_json(path: str) -> tuple[dict, bytes]:
    for attempt in range(5):
        try:
            paced()
            req = urllib.request.Request(API + path, headers={"Authorization": "Bearer " + key()})
            with urllib.request.urlopen(req, timeout=180) as r:
                body = r.read()
            return json.loads(body), body
        except Exception as e:  # noqa: BLE001 - retried, then reported without the key
            err = f"{type(e).__name__} {getattr(e, 'code', '')}".strip()
            time.sleep(5 * (attempt + 1))
    raise SystemExit(f"GET {path}: {err}")


class Hashing(io.RawIOBase):
    def __init__(self, r):
        self.r, self.h, self.n = r, hashlib.sha256(), 0

    def readable(self):
        return True

    def readinto(self, b):
        chunk = self.r.read(len(b))
        self.h.update(chunk)
        self.n += len(chunk)
        b[:len(chunk)] = chunk
        return len(chunk)


def listing(at: datetime) -> tuple[datetime, list[dict]]:
    j, _ = api_json("/options/snapshots?at_datetime=" + urllib.parse.quote(utc(at)))
    snaps = j.get("snapshots") or []
    if len(snaps) != 1 or not snaps[0].get("files"):
        raise SystemExit(f"snapshot listing at {utc(at)}: {[s.get('time') for s in snaps]}")
    t = datetime.strptime(snaps[0]["time"], "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
    files = snaps[0]["files"]

    def part(f):
        p = f.get("part")
        return 0 if len(files) == 1 else int(p)
    files = sorted(files, key=part)
    if [part(f) for f in files] != list(range(len(files))):
        raise SystemExit(f"snapshot {snaps[0]['time']}: parts {[f.get('part') for f in files]}")
    return t, files


def fetch_snapshot(side: Path, d: str, kind: str, wanted: set[str]) -> dict:
    """kind 'cmp': the listing at Cboe's instant, admitted only at that instant. kind 'eng': the
    engine's (close - 5 min, admitted 15 to 2 minutes before the close)."""
    close = close_at(d)
    want_at = cboe_instant(d) if kind == "cmp" else close - timedelta(minutes=5)
    t, files = listing(want_at)
    if kind == "cmp" and t != want_at:
        # README: admitted only at Cboe's instant; recorded, its quotes NO_SNAPSHOT_AT_INSTANT
        return {"date": d, "kind": kind, "file": None, "refused": f"listing gave {utc(t)}, not {utc(want_at)}",
                "snapshot_at": utc(t), "asked_at": utc(want_at),
                "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")}
    if kind == "eng" and not (close - timedelta(minutes=15) <= t <= close - timedelta(minutes=2)):
        raise SystemExit(f"{d}: engine snapshot at {utc(t)} outside 15 to 2 minutes before {utc(close)}")
    h, total, rows, latest, header = hashlib.sha256(), 0, [], 0.0, None
    for f in files:
        with urllib.request.urlopen(f["url"], timeout=600) as r:
            hr = Hashing(r)
            text = io.TextIOWrapper(gzip.GzipFile(fileobj=io.BufferedReader(hr, 1 << 20)), "utf-8", newline="")
            for i, line in enumerate(text):
                line = line.rstrip("\r\n")
                if i == 0 and line.startswith("CONTRACT ID"):
                    if line != HEADER:
                        raise SystemExit(f"{d} {kind}: snapshot header {line[:80]}")
                    header = line
                    continue
                root = line.split("_", 1)[0]
                if root_owner(root, wanted):
                    rows.append(line)
                    for i_ in (5, 10, 13):
                        v = line.split(",")[i_] if line.count(",") >= 13 else ""
                        if v:
                            latest = max(latest, float(v))
            # hash the provider's bytes one file after another
            h.update(hr.h.digest()) if len(files) > 1 else None
            single = hr.h.hexdigest()
            total += hr.n
    if header is None:
        raise SystemExit(f"{d} {kind}: snapshot without its header")
    name = f"snap-{kind}-{d}.csv.gz"
    body = (HEADER + "\n" + "\n".join(rows) + "\n").encode()
    (side / name).write_bytes(gzip.compress(body, mtime=0))
    return {"date": d, "kind": kind, "file": name, "file_sha256": hashlib.sha256((side / name).read_bytes()).hexdigest(),
            "snapshot_at": utc(t), "asked_at": utc(want_at), "parts": len(files),
            "urls": [f["url"].split("?", 1)[0] for f in files],
            "provider_sha256": single if len(files) == 1 else "parts:" + h.hexdigest(),
            "provider_bytes": total, "rows": len(rows), "latest_stamp": latest,
            "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")}


def fetch_eod(side: Path, d: str, root: str) -> dict:
    pages, urls, sha = [], [], hashlib.sha256()
    nxt = None
    while True:
        q = {"date": d, "include_related_symbols": "true", "page_size": "10000", "recalculate_stats": "false"}
        if nxt:
            q["next_page"] = nxt
        path = f"/options/prices/by_ticker/{root}/eod?" + urllib.parse.urlencode(sorted(q.items()))
        j, raw = api_json(path)
        sha.update(raw)
        pages.append(j)
        urls.append(API + path)
        nxt = j.get("next_page")
        if not nxt:
            break
    name = f"eod-{root}-{d}.json.gz"
    (side / name).write_bytes(gzip.compress(json.dumps(pages).encode(), mtime=0))
    return {"date": d, "kind": "eod", "root": root, "file": name,
            "file_sha256": hashlib.sha256((side / name).read_bytes()).hexdigest(), "urls": urls,
            "provider_sha256": sha.hexdigest(), "pages": len(pages),
            "rows": sum(len(p.get("prices") or []) for p in pages),
            "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")}


def fetch(order: str, side: str, dates: list[str]) -> int:
    days = order_days(Path(order))
    out = Path(side)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "fetched.json"
    log = json.loads(log_path.read_text()) if log_path.is_file() else []
    done = {(e["date"], e["kind"], e.get("root")) for e in log}
    jobs = []
    for d in sorted(dates or days):
        syms = set(ITEMS[days[d][0]])
        for kind in ("cmp", "eng"):
            if (d, kind, None) not in done:
                jobs.append(("snap", d, kind, syms))
        for s in sorted(syms):
            if (d, "eod", s) not in done:
                jobs.append(("eod", d, s, None))

    def run(job):
        what, d, x, syms = job
        e = fetch_snapshot(out, d, x, syms) if what == "snap" else fetch_eod(out, d, x)
        with LOCK:
            log.append(e)
            log_path.write_text(json.dumps(log, indent=1) + "\n")
            print(f"{d} {x}: {e.get('rows', e.get('refused'))} rows", file=sys.stderr, flush=True)

    with ThreadPoolExecutor(1) as ex:
        for f in [ex.submit(run, j) for j in jobs]:
            f.result()
    return 0


# ------------------------------------------------------------------ compare: loading Intrinio's side

def num(v: str):
    return float(v) if v not in ("", None) else None


def side_quote(price, size):
    """D-072: a side priced 0.00 with size 0 or blank, or blank, is no quote."""
    if price is None or (price == 0 and not size):
        return None
    return round(price * 10000)


def load_snap(path: Path) -> tuple[dict, dict]:
    rows, dup = {}, {}
    text = gzip.decompress(path.read_bytes()).decode().split("\n")
    if text[0] != HEADER:
        raise SystemExit(f"{path}: header")
    for line in text[1:]:
        if not line:
            continue
        f = line.split(",")
        k = snap_key(f[0])
        r = {"oi": num(f[1]), "vol": num(f[4]), "trade_t": num(f[5]),
             "ask": num(f[8]), "ask_size": num(f[9]), "ask_t": num(f[10]),
             "bid": num(f[11]), "bid_size": num(f[12]), "bid_t": num(f[13])}
        if k in rows and rows[k] != r:
            dup[k] = [rows[k], r]
        rows.setdefault(k, r)
    for k in dup:
        rows.pop(k, None)
    return rows, dup


def load_eod(path: Path) -> dict:
    rows = {}
    for page in json.loads(gzip.decompress(path.read_bytes())):
        for p in page.get("prices") or []:
            k = osi_key(p["option"]["code"])
            if k in rows:
                raise SystemExit(f"{path}: EOD duplicate {k}")
            rows[k] = p["price"]
    return rows


def verify_side(side: Path) -> list[dict]:
    log = json.loads((side / "fetched.json").read_text())
    for e in log:
        if e.get("file") is None:
            continue
        if hashlib.sha256((side / e["file"]).read_bytes()).hexdigest() != e["file_sha256"]:
            raise SystemExit(f"{e['file']}: differs from fetched.json")
    return log


def ks(k) -> str:
    return f"{k[0]} {k[1]} {k[2]} {k[3]}"


def rate(n_eq: int, n: int):
    return round(n_eq / n, 6) if n else None


# ------------------------------------------------------------------ compare

def compare(order: str, side_dir: str, prefix: str) -> int:
    days = order_days(Path(order))
    side = Path(side_dir)
    log = verify_side(side)
    by = {(e["date"], e["kind"], e.get("root")): e for e in log}
    missing = [(d, k) for d in days for k in ("cmp", "eng") if (d, k, None) not in by]
    missing += [(d, s) for d, (item, _) in days.items() for s in ITEMS[item] if (d, "eod", s) not in by]
    if missing:
        raise SystemExit(f"Intrinio side incomplete: {missing[:10]}")

    q = defaultdict(Counter)         # (sym) -> verdict counts (comparison 1)
    q_size = defaultdict(Counter)
    q_diff, q_examples = [], defaultdict(list)
    q_eng = defaultdict(Counter)     # 15:55 vs Cboe 1545 / EOD
    q_eng_mid = defaultdict(lambda: defaultdict(list))
    v = defaultdict(Counter)
    v_raw = defaultdict(Counter)
    v_tot = []
    v_diff = []
    oi = defaultdict(Counter)
    oi_diff = []
    oi_other = defaultdict(Counter)  # comparison snapshot OI, EOD OI
    oi_shift = defaultdict(Counter)
    cov = {}
    cov_missing_live = []
    dcodes = Counter()
    iv = defaultdict(list)
    snaps_meta = {}
    per_day_cache = {}

    for d in sorted(days):
        item, z = days[d]
        syms = ITEMS[item]
        cb = load_cboe(z, d)
        for r in cb.values():
            dcodes[r["delivery_code"]] += 1
        cmp_file = by[(d, "cmp", None)]["file"]
        cmp_rows, cmp_dup = load_snap(side / cmp_file) if cmp_file else (None, {})
        eng_rows, eng_dup = load_snap(side / by[(d, "eng", None)]["file"])
        eod = {}
        for s in syms:
            eod.update(load_eod(side / by[(d, "eod", s)]["file"]))
        snaps_meta[d] = {k: {x: by[(d, k, None)].get(x) for x in ("snapshot_at", "provider_sha256", "rows", "latest_stamp", "refused")}
                         for k in ("cmp", "eng")}
        owner = lambda k: root_owner(k[0], set(syms))  # noqa: E731
        inst = cboe_instant(d).timestamp()
        o_t, c_t = open_at(d).timestamp(), close_at(d).timestamp()
        month = d[:7]

        # 1. quotes at the same instant
        for k, c in cb.items():
            sym = owner(k) or c["underlying_symbol"]
            g = f"{sym} {month}"
            cbid, cask = num(c["bid_1545"]), num(c["ask_1545"])
            cb_b = side_quote(cbid, num(c["bid_size_1545"]))
            cb_a = side_quote(cask, num(c["ask_size_1545"]))
            # 1b. the engine's 15:55 snapshot against Cboe 15:45 and EOD (reported)
            e = None if k in eng_dup else eng_rows.get(k)
            if e is not None:
                eb, ea = side_quote(e["bid"], e["bid_size"]), side_quote(e["ask"], e["ask_size"])
                for tag, b_, a_ in (("vs_1545", cb_b, cb_a),
                                    ("vs_eod", side_quote(num(c["bid_eod"]), num(c["bid_size_eod"])),
                                     side_quote(num(c["ask_eod"]), num(c["ask_size_eod"])))):
                    q_eng[g][tag + ("_equal" if (eb, ea) == (b_, a_) else "_diff")] += 1
                    if None not in (eb, ea, b_, a_) and b_ + a_ > 0:
                        q_eng_mid[g][tag].append(abs((eb + ea) - (b_ + a_)) / (b_ + a_))
            if cmp_rows is None:
                q[g]["NO_SNAPSHOT_AT_INSTANT"] += 1
                continue
            if k in cmp_dup:
                q[g]["DUPLICATE"] += 1
                continue
            s = cmp_rows.get(k)
            if s is None:
                q[g]["NOT_IN_SNAPSHOT"] += 1
                continue
            ib, ia = side_quote(s["bid"], s["bid_size"]), side_quote(s["ask"], s["ask_size"])
            stamps = [t for t in (s["bid_t"], s["ask_t"]) if t is not None]
            quoted_no_t = (s["bid"] is not None and s["bid_t"] is None) or (s["ask"] is not None and s["ask_t"] is None)
            t = max(stamps) if stamps else None
            same = ib == cb_b and ia == cb_a
            if quoted_no_t:
                cls = "NOT_COMPARABLE"
            elif t is None or t < inst - 1:
                cls = "EQUAL" if same else "DIFF"
            elif t < inst + 1:
                cls = "BOUNDARY"
            else:
                cls = "AFTER"
            q[g][cls] += 1
            if cls in ("BOUNDARY", "AFTER"):
                q[g][cls + ("_EQUAL" if same else "_DIFF")] += 1
            if cls in ("EQUAL", "DIFF"):
                for iq, cq, iz, cz in ((ib, cb_b, s["bid_size"], c["bid_size_1545"]), (ia, cb_a, s["ask_size"], c["ask_size_1545"])):
                    if iq is not None and iq == cq:
                        q_size[g]["size_equal" if (iz or 0) == float(cz or 0) else "size_diff"] += 1
            if cls == "DIFF":
                worst = max(abs((ib or 0) - (cb_b or 0)), abs((ia or 0) - (cb_a or 0))) / 10000
                q_diff.append({"date": d, "contract": ks(k), "intrinio_bid": s["bid"], "intrinio_bid_size": s["bid_size"],
                               "intrinio_ask": s["ask"], "intrinio_ask_size": s["ask_size"],
                               "cboe_bid": cbid, "cboe_bid_size": c["bid_size_1545"], "cboe_ask": cask,
                               "cboe_ask_size": c["ask_size_1545"],
                               "last_quote_et": datetime.fromtimestamp(t, NY).strftime("%H:%M:%S.%f") if t else None,
                               "worst_abs_diff": round(worst, 4)})

        # 2. volume: the engine's (EOD; D-125 snapshot where EOD null and traded in session)
        tot = defaultdict(lambda: Counter())
        for k in set(cb) | set(eod):
            sym = owner(k) or (cb[k]["underlying_symbol"] if k in cb else "?")
            g = f"{sym} {month}"
            c = cb.get(k)
            cv = int(c["trade_volume"]) if c else 0
            p = eod.get(k)
            raw = (p.get("volume") or 0) if p else 0
            ev, src = raw, "eod"
            if p is not None and p.get("volume") is None:
                e = eng_rows.get(k)
                if e and e["vol"] and e["trade_t"] and o_t <= e["trade_t"] < c_t:
                    ev, src = int(e["vol"]), "snapshot_d125"
            tot[sym]["intrinio"] += ev
            tot[sym]["cboe"] += cv
            tot[sym]["intrinio_raw_eod"] += raw
            traded = ev > 0 or cv > 0
            verdict = "EQUAL" if ev == cv else "DIFF"
            v[g][verdict] += 1
            if traded:
                v[g]["traded_" + verdict] += 1
                v_raw[g]["traded_" + ("EQUAL" if raw == cv else "DIFF")] += 1
            if src == "snapshot_d125":
                v[g]["from_snapshot_d125"] += 1
            if verdict == "DIFF":
                v[g]["intrinio_low" if ev < cv else "intrinio_high"] += 1
                if p is None:
                    v[g]["DIFF_not_in_eod"] += 1
                if c is None:
                    v[g]["DIFF_not_in_cboe"] += 1
                v_diff.append({"date": d, "contract": ks(k), "intrinio": ev, "source": src if p else "absent_from_eod",
                               "cboe": cv, "diff": ev - cv, "in_cboe": c is not None})
        for sym, t in sorted(tot.items()):
            v_tot.append({"date": d, "underlying": sym, "intrinio": t["intrinio"], "cboe": t["cboe"],
                          "intrinio_raw_eod": t["intrinio_raw_eod"],
                          "rel": round(t["intrinio"] / t["cboe"] - 1, 6) if t["cboe"] else None})

        # 3. open interest: the engine's (15:55 snapshot), same quote_date
        for k, c in cb.items():
            sym = owner(k) or c["underlying_symbol"]
            g = f"{sym} {month}"
            co = int(c["open_interest"])
            if k in eng_dup:
                oi[g]["DUPLICATE"] += 1
                continue
            e = eng_rows.get(k)
            if e is None:
                oi[g]["NOT_IN_SNAPSHOT"] += 1
                continue
            if e["oi"] is None:
                oi[g]["NOT_COMPARABLE_BLANK"] += 1
                continue
            same = int(e["oi"]) == co
            oi[g]["EQUAL" if same else "DIFF"] += 1
            if co or e["oi"]:
                oi[g]["nonzero_" + ("EQUAL" if same else "DIFF")] += 1
            if not same:
                oi[g]["intrinio_low" if e["oi"] < co else "intrinio_high"] += 1
                oi_diff.append({"date": d, "contract": ks(k), "intrinio": int(e["oi"]), "cboe": co, "diff": int(e["oi"]) - co,
                                "eod_open_interest": (eod.get(k) or {}).get("open_interest"),
                                "comparison_snapshot_oi": ((cmp_rows or {}).get(k) or {}).get("oi")})
            x = (cmp_rows or {}).get(k)
            if x is not None and x["oi"] is not None:
                oi_other[g]["cmp_snapshot_" + ("EQUAL" if int(x["oi"]) == co else "DIFF")] += 1
            p = eod.get(k)
            if p is not None:
                oi_other[g]["eod_" + ("EQUAL" if (p.get("open_interest") or 0) == co else "DIFF")] += 1
        per_day_cache[d] = (item, {k: int(c["open_interest"]) for k, c in cb.items()},
                            {k: int(e["oi"]) for k, e in eng_rows.items() if e["oi"] is not None})

        # 4. coverage
        live = lambda c: (num(c["bid_1545"]) or 0) > 0 and (num(c["ask_1545"]) or 0) > 0 or int(c["trade_volume"]) or int(c["open_interest"])  # noqa: E731
        for name, other in (("cmp_snapshot", cmp_rows), ("engine_snapshot", eng_rows), ("eod", eod)):
            if other is None:
                continue
            for k in set(cb) | set(other):
                sym = owner(k) or (cb[k]["underlying_symbol"] if k in cb else "?")
                cg = cov.setdefault(f"{sym} {month}", {}).setdefault(name, defaultdict(Counter))
                kind = root_kind(k[0], sym)
                where = "both" if (k in cb and k in other) else ("cboe_only" if k in cb else "intrinio_only")
                cg["by_root"][f"{k[0]} ({kind}) {where}"] += 1
                if where != "both":
                    cg["by_expiration"][f"{k[1]} {where}"] += 1
                if where == "cboe_only":
                    if live(cb[k]):
                        cg["counts"]["cboe_only_live"] += 1
                        if name == "engine_snapshot" and len(cov_missing_live) < CAP * 5:
                            c = cb[k]
                            cov_missing_live.append({"date": d, "contract": ks(k), "bid_1545": c["bid_1545"], "ask_1545": c["ask_1545"],
                                                     "trade_volume": c["trade_volume"], "open_interest": c["open_interest"],
                                                     "in_eod": k in eod})
                    else:
                        cg["counts"]["cboe_only_dead"] += 1
                elif where == "intrinio_only":
                    if name == "eod":
                        p = eod[k]
                        tag = "in_snapshot" if k in eng_rows else "not_in_snapshot"
                        tag += "_with_volume" if p.get("volume") else ("_with_oi" if p.get("open_interest") else "_no_volume_no_oi")
                        cg["counts"]["intrinio_only_" + tag] += 1
                    else:
                        r = other[k]
                        cg["counts"]["intrinio_only_" + ("live" if (r["vol"] or r["oi"] or (r["bid"] or 0) > 0) else "dead")] += 1
                    if len(cg["examples_intrinio_only"]) < 20:
                        cg["examples_intrinio_only"][ks(k)] += 1
                cg["counts"][where] += 1

        # 5. IV (reported)
        qd = date.fromisoformat(d)
        for k, c in cb.items():
            p = eod.get(k)
            civ, dl = num(c["implied_volatility_1545"]), num(c["delta_1545"])
            dte = (date.fromisoformat(k[1]) - qd).days
            if p and p.get("implied_volatility") and civ and dl is not None and 0.25 <= abs(dl) <= 0.75 and 7 <= dte <= 60:
                iv[f"{owner(k) or c['underlying_symbol']} {month}"].append(abs(p["implied_volatility"] - civ))

    # OI convention test: shifted alignments within each item
    ds = sorted(per_day_cache)
    for a, b in zip(ds, ds[1:]):
        if per_day_cache[a][0] != per_day_cache[b][0]:
            continue
        (_, cb_a, in_a), (_, cb_b, in_b) = per_day_cache[a], per_day_cache[b]
        item = per_day_cache[a][0]
        for tag, ii, cc in (("same_day", in_b, cb_b), ("intrinio_T_vs_cboe_T+1", in_a, cb_b), ("cboe_T_vs_intrinio_T+1", in_b, cb_a)):
            for k in set(ii) & set(cc):
                if ii[k] or cc[k]:
                    oi_shift[item][tag + ("_equal" if ii[k] == cc[k] else "_diff")] += 1
    convention = {}
    for item, c in oi_shift.items():
        r = {t: rate(c[t + "_equal"], c[t + "_equal"] + c[t + "_diff"]) for t in ("same_day", "intrinio_T_vs_cboe_T+1", "cboe_T_vs_intrinio_T+1")}
        r["verdict"] = "CONVENTION" if max(r["intrinio_T_vs_cboe_T+1"] or 0, r["cboe_T_vs_intrinio_T+1"] or 0) > (r["same_day"] or 0) else "SAME_DAY_HOLDS"
        convention[item] = r

    def pct(xs, p):
        if not xs:
            return None
        xs = sorted(xs)
        return round(xs[min(len(xs) - 1, int(p * len(xs)))], 6)

    quotes = {
        "check": "Cboe DataShop 15:45 NBBO against Intrinio's 15:45 snapshot, per contract (README comparison 1)",
        "rates": {g: {"judged": c["EQUAL"] + c["DIFF"], "equal": c["EQUAL"], "diff": c["DIFF"],
                      "agreement": rate(c["EQUAL"], c["EQUAL"] + c["DIFF"]), **{x: c[x] for x in sorted(c) if x not in ("EQUAL", "DIFF")},
                      "boundary_agreement": rate(c["BOUNDARY_EQUAL"], c["BOUNDARY"]), "after_agreement": rate(c["AFTER_EQUAL"], c["AFTER"]),
                      "sizes_on_equal_judged_sides": dict(q_size[g])}
                  for g, c in sorted(q.items())},
        "engine_1555": {g: {**dict(c), "agreement_vs_1545": rate(c["vs_1545_equal"], c["vs_1545_equal"] + c["vs_1545_diff"]),
                            "agreement_vs_eod": rate(c["vs_eod_equal"], c["vs_eod_equal"] + c["vs_eod_diff"]),
                            "median_rel_mid_diff_vs_1545": pct(q_eng_mid[g]["vs_1545"], 0.5),
                            "median_rel_mid_diff_vs_eod": pct(q_eng_mid[g]["vs_eod"], 0.5)}
                        for g, c in sorted(q_eng.items())},
        "diff_count": len(q_diff),
        "worst_diffs": sorted(q_diff, key=lambda x: -x["worst_abs_diff"])[:CAP],
        "snapshots": snaps_meta,
        "pass": not q_diff,
    }
    volume = {
        "check": "Intrinio per-contract option volume (engine: EOD, D-125) against Cboe trade_volume (README comparison 2)",
        "rates": {g: {**dict(c), "agreement_all": rate(c["EQUAL"], c["EQUAL"] + c["DIFF"]),
                      "agreement_traded": rate(c["traded_EQUAL"], c["traded_EQUAL"] + c["traded_DIFF"]),
                      "raw_eod_agreement_traded": rate(v_raw[g]["traded_EQUAL"], v_raw[g]["traded_EQUAL"] + v_raw[g]["traded_DIFF"])}
                  for g, c in sorted(v.items())},
        "totals": v_tot,
        "diff_count": len(v_diff),
        "worst_diffs": sorted(v_diff, key=lambda x: -abs(x["diff"]))[:CAP],
        "pass": not v_diff,
    }
    oi_rep = {
        "check": "Intrinio 15:55 snapshot open interest (engine, D-079) against Cboe start-of-day OCC open interest, same quote_date (README comparison 3)",
        "rates": {g: {**dict(c), "agreement": rate(c["EQUAL"], c["EQUAL"] + c["DIFF"]),
                      "agreement_nonzero": rate(c["nonzero_EQUAL"], c["nonzero_EQUAL"] + c["nonzero_DIFF"]),
                      "reported_other": dict(oi_other[g])}
                  for g, c in sorted(oi.items())},
        "convention": convention,
        "diff_count": len(oi_diff),
        "worst_diffs": sorted(oi_diff, key=lambda x: -abs(x["diff"]))[:CAP],
        "pass": not oi_diff,
    }
    coverage = {
        "check": "Contracts in one source and not the other (README comparison 4)",
        "delivery_code_values": dict(dcodes),
        "by_group": {g: {n: {k: dict(x) for k, x in c.items()} for n, c in p.items()} for g, p in sorted(cov.items())},
        "cboe_live_missing_from_engine_snapshot": cov_missing_live,
    }
    ivrep = {
        "check": "Intrinio EOD provider IV against Cboe implied_volatility_1545, |delta| 0.25-0.75, 7-60 days (README comparison 5, reported only)",
        "by_group": {g: {"n": len(x), "median_abs_diff": pct(x, 0.5), "p90_abs_diff": pct(x, 0.9)} for g, x in sorted(iv.items())},
    }
    parts = {"quotes": quotes, "volume": volume, "oi": oi_rep, "coverage": coverage, "iv": ivrep}
    for n, rep in parts.items():
        Path(f"{prefix}-{n}.json").write_text(json.dumps(rep, indent=1, default=str) + "\n")
    summary = {"pass": quotes["pass"] and volume["pass"] and oi_rep["pass"],
               "quotes_pass": quotes["pass"], "volume_pass": volume["pass"], "oi_pass": oi_rep["pass"],
               "quote_rates": {g: r["agreement"] for g, r in quotes["rates"].items()},
               "volume_rates_traded": {g: r["agreement_traded"] for g, r in volume["rates"].items()},
               "oi_rates": {g: r["agreement"] for g, r in oi_rep["rates"].items()},
               "oi_convention": convention,
               "order_days": len(days), "intrinio_side_files": len(log)}
    Path(f"{prefix}-summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary, indent=1))
    return 0 if summary["pass"] else 1


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "fetch":
        sys.exit(fetch(a[1], a[2], a[3:]))
    sys.exit(compare(a[1], a[2], a[3]))
