"""SEC EDGAR filer facts against the stored company figures (rules in README.md, declared first).

    python3 -I edgar.py fetch CACHE TICKER...      # SEC JSON into CACHE, with CACHE/fetched.json
    python3 -I edgar.py compare CACHE BODIES.tsv > report.json
    python3 -I edgar.py events CACHE EVENTS.tsv > report.json   # CDD-EVENTS01 filing times

CACHE holds SEC's own responses as downloaded (companyfacts, submissions and their extra pages,
company_tickers.json) and `fetched.json`: url, file, sha256 and UTC fetch time of each. The
User-Agent SEC requires is read from `sec-contact.txt` at the checkout root (never printed).
BODIES.tsv: date, measurement_id, object_id, body as `edgar.sql` prints them from core's
published store (`psql -A -t -F $'\\t'`). Exit 0 on pass, 1 otherwise.
"""
from __future__ import annotations

import hashlib
import itertools
import json
import math
import re
import sys
import time
import urllib.request
from collections import Counter, defaultdict
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[2]
CONTACT = ROOT / "sec-contact.txt"
PAUSE = 0.25  # seconds between SEC requests: 4 a second, SEC allows 10
TOL = 1e-9
NY = ZoneInfo("America/New_York")
PERIODIC = {"10-Q", "10-K", "20-F", "40-F", "10-KT", "10-QT"}
ANNUAL_FORMS = {"10-K", "20-F", "40-F", "10-KT"}
QUARTER_FORMS = {"10-Q", "10-QT"}
MAX_COMBOS = 4096


# ---------------------------------------------------------------------------------------- fetch
def _get(url: str) -> bytes:
    agent = " ".join(CONTACT.read_text().split())  # one header line, whatever the file's layout
    req = urllib.request.Request(url, headers={"User-Agent": agent, "Accept-Encoding": "identity"})
    with urllib.request.urlopen(req, timeout=60) as r:
        body = r.read()
    time.sleep(PAUSE)
    return body


def fetch(cache: str, tickers: list[str]) -> int:
    out = Path(cache)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "fetched.json"
    log = json.loads(log_path.read_text()) if log_path.is_file() else []

    def save(url: str, name: str) -> bytes:
        body = _get(url)
        (out / name).write_bytes(body)
        log.append({"url": url, "file": name, "sha256": hashlib.sha256(body).hexdigest(),
                    "fetched_at": datetime.now(timezone.utc).isoformat(timespec="seconds")})
        log_path.write_text(json.dumps(log, indent=1) + "\n")
        return body

    names = json.loads(save("https://www.sec.gov/files/company_tickers.json", "company_tickers.json"))
    cik = {v["ticker"]: int(v["cik_str"]) for v in names.values()}
    for t in tickers:
        c = f"{cik[t]:010d}"
        save(f"https://data.sec.gov/api/xbrl/companyfacts/CIK{c}.json", f"companyfacts-{t}.json")
        sub = json.loads(save(f"https://data.sec.gov/submissions/CIK{c}.json", f"submissions-{t}.json"))
        for f in sub["filings"].get("files", []):
            save(f"https://data.sec.gov/submissions/{f['name']}", f"submissions-{t}-{f['name']}")
    return 0


# ------------------------------------------------------------------------- the engine's tables
def norm(tag: str) -> str:
    """fnd.rs normalize_tag: suffix after ':' or '/', ASCII alnum lower, one prefix removed."""
    tail = re.split(r"[:/]", tag)[-1]
    s = re.sub(r"[^A-Za-z0-9]", "", tail).lower()
    for p in ("usgaap", "dei", "ifrsfull"):
        if s.startswith(p):
            return s[len(p):]
    return s


STATEMENT = {c: "B" for c in ("total_assets", "total_liabilities", "total_equity", "cash", "current_assets",
                              "current_liabilities", "deposits", "loans", "preferred_equity")}
STATEMENT.update({c: "I" for c in ("revenue", "operating_income", "net_income", "net_income_to_common")})
STATEMENT.update({c: "C" for c in ("operating_cash_flow", "capital_expenditure")})

# fnd.rs Concept::aliases, as-reported (used only for tag notes and a history cell's renamed tag)
ALIASES = {k: [norm(a) for a in v] for k, v in {
    "total_assets": ["Assets"], "total_liabilities": ["Liabilities"],
    "total_equity": ["StockholdersEquity", "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
                     "PartnersCapital", "EquityAttributableToOwnersOfParent", "Equity"],
    "cash": ["CashAndCashEquivalentsAtCarryingValue", "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
             "CashAndDueFromBanks", "CashAndCashEquivalents"],
    "current_assets": ["AssetsCurrent", "CurrentAssets"], "current_liabilities": ["LiabilitiesCurrent", "CurrentLiabilities"],
    "revenue": ["RevenueFromContractWithCustomerExcludingAssessedTax", "Revenues", "SalesRevenueNet",
                "SalesRevenueGoodsNet", "SalesRevenueServicesNet", "InterestAndDividendIncomeOperating",
                "RevenueFromContractWithCustomerIncludingAssessedTax", "RevenuesNetOfInterestExpense", "Revenue",
                "RevenueFromContractsWithCustomers"],
    "operating_income": ["OperatingIncomeLoss", "ProfitLossFromOperatingActivities"],
    "net_income": ["NetIncomeLoss", "ProfitLossAttributableToOwnersOfParent", "ProfitLoss",
                   "NetIncomeLossAvailableToCommonStockholdersBasic"],
    "operating_cash_flow": ["NetCashProvidedByUsedInOperatingActivities",
                            "NetCashProvidedByUsedInOperatingActivitiesContinuingOperations",
                            "CashFlowsFromUsedInOperatingActivities"],
    "capital_expenditure": ["PaymentsToAcquirePropertyPlantAndEquipment", "PaymentsToAcquireProductiveAssets",
                            "PaymentsForAdditionsToPropertyPlantAndEquipment",
                            "PurchaseOfPropertyPlantAndEquipmentClassifiedAsInvestingActivities",
                            "PaymentsToAcquireOtherPropertyPlantAndEquipment",
                            "PaymentsToAcquireLandUseRightsPropertyAndEquipment"],
    "deposits": ["Deposits", "DepositsLiabilities"],
    "loans": ["LoansAndLeasesReceivableNetReportedAmount",
              "FinancingReceivableExcludingAccruedInterestAfterAllowanceForCreditLoss"],
    "net_income_to_common": ["NetIncomeLossAvailableToCommonStockholdersBasic"],
    "preferred_equity": ["PreferredStockValue", "PreferredStockValueOutstanding"],
}.items()}

# fnd.rs BASES: (kind, inputs); ROA adds the prior position's total assets
BASES = {
    "asset_scale": ("scale", ["total_assets"]), "revenue_scale": ("scale", ["revenue"]),
    "net_margin": ("ratio", ["net_income", "revenue"]), "operating_margin": ("ratio", ["operating_income", "revenue"]),
    "operating_cash_margin": ("ratio", ["operating_cash_flow", "revenue"]),
    "liabilities_to_assets": ("ratio", ["total_liabilities", "total_assets"]),
    "equity_to_assets": ("ratio", ["total_equity", "total_assets"]),
    "current_ratio": ("ratio", ["current_assets", "current_liabilities"]),
    "cash_to_assets": ("ratio", ["cash", "total_assets"]),
    "capital_intensity": ("absratio", ["capital_expenditure", "revenue"]),
    "deposits_to_assets": ("ratio", ["deposits", "total_assets"]),
    "loans_to_assets": ("ratio", ["loans", "total_assets"]),
    "return_on_assets_state": ("roa", ["net_income", "total_assets"]),
}
FND02_BASES = ["revenue_scale", "net_margin", "operating_cash_margin", "liabilities_to_assets", "cash_to_assets"]
CURRENCY_FAILS = {"asset_scale", "revenue_scale"}  # a unit mismatch fails these (README)


def apply(kind: str, v: list[float]) -> float | None:
    try:
        if kind == "scale":
            return math.log(abs(v[0])) if v[0] else None
        if kind == "ratio":
            return v[0] / v[1]
        if kind == "absratio":
            return abs(v[0]) / abs(v[1])
        if kind == "roa":
            return v[0] / ((v[1] + v[2]) / 2.0)
    except ZeroDivisionError:
        return None
    raise ValueError(kind)


def close_enough(ours: float, theirs: float | None) -> bool:
    return theirs is not None and abs(ours - theirs) <= TOL * max(1.0, abs(theirs))


def close_of(d: str) -> datetime:
    y, m, dd = map(int, d.split("-"))
    return datetime(y, m, dd, 16, 0, tzinfo=NY).astimezone(timezone.utc)


def is_currency(u: str) -> bool:
    return len(u) == 3 and u.isalpha() and u.isupper()


# ----------------------------------------------------------------------------------- EDGAR side
class NotComparable(Exception):
    pass


class Company:
    def __init__(self, cache: Path, ticker: str):
        self.ticker = ticker
        cf = json.loads((cache / f"companyfacts-{ticker}.json").read_text())
        self.facts: dict[str, dict[str, list]] = defaultdict(lambda: defaultdict(list))
        for tax in ("us-gaap", "ifrs-full", "dei"):
            for name, d in cf["facts"].get(tax, {}).items():
                for unit, entries in d["units"].items():
                    for e in entries:
                        self.facts[norm(name)][unit].append((f"{tax}:{name}", e))
        self.filings: dict[str, tuple] = {}
        for p in sorted(cache.glob(f"submissions-{ticker}*.json")):
            j = json.loads(p.read_text())
            r = j["filings"]["recent"] if "filings" in j else j
            for i, accn in enumerate(r["accessionNumber"]):
                acc = datetime.fromisoformat(r["acceptanceDateTime"][i].replace("Z", "+00:00"))
                self.filings[accn] = (r["form"][i], r["reportDate"][i], acc)
        orig = [(rd, f) for f, rd, _ in self.filings.values() if f in PERIODIC and rd]
        self.quarterly = any(f in QUARTER_FORMS for _, f in orig)
        self.periods = sorted(set(x for x in orig if self.quarterly or x[1] in ANNUAL_FORMS))
        self.dates = [d for d, _ in self.periods]

    def accepted(self, accn: str, filed: str) -> datetime:
        f = self.filings.get(accn)
        if f:
            return f[2]
        # not in the submissions index: the end of its filing date, the latest it can have been
        y, m, d = map(int, filed.split("-"))
        return datetime(y, m, d, 23, 59, 59, tzinfo=NY).astimezone(timezone.utc)

    def options(self, tag: str, unit: str, kind: str, end: str) -> list[dict]:
        """Distinct values of the element at the period, first filed first, from periodic filings."""
        seen: dict[float, dict] = {}
        for qname, e in self.facts.get(tag, {}).get(unit, []):
            form = e.get("form", "")
            if e["end"] != end or form.removesuffix("/A") not in PERIODIC:
                continue
            if kind == "I":
                if "start" in e:
                    continue
            else:
                if "start" not in e:
                    continue
                days = (date.fromisoformat(e["end"]) - date.fromisoformat(e["start"])).days
                lo, hi = {"Q": (0, 120), "H": (150, 200), "N": (240, 300), "Y": (340, 380)}[kind]
                if not lo <= days <= hi:
                    continue
            acc = self.accepted(e["accn"], e["filed"])
            o = {"element": qname, "unit": unit, "end": end, "start": e.get("start"), "value": e["val"],
                 "accn": e["accn"], "form": form, "filed": e["filed"],
                 "accepted": acc.isoformat().replace("+00:00", "Z")}
            prev = seen.get(e["val"])
            if prev is None or o["accepted"] < prev["accepted"]:
                seen[e["val"]] = o
        return sorted(seen.values(), key=lambda o: (o["accepted"], o["value"]))

    def units_of(self, tag: str) -> list[str]:
        return list(self.facts.get(tag, {}).keys())

    def prev(self, end: str, k: int) -> list[tuple]:
        if end not in self.dates:
            raise NotComparable(f"{end} is not the period of a periodic report in EDGAR's index")
        i = self.dates.index(end)
        if i < k:
            raise NotComparable(f"fewer than {k} periodic reports before {end}")
        return self.periods[i - k:i]

    def quarter_ops(self, concept: str, end: str, q: int, annual: bool) -> list[tuple]:
        """[(sign, kind, end)] for one quarter of one concept (spec 06 §5.8 as fnd.rs builds it)."""
        st = STATEMENT[concept]
        if st == "B":
            return [(1, "I", end)]
        if annual:
            if q != 4:
                raise NotComparable("an annual filer's flow at a non-annual position")
            return [(1, "Y", end)]
        if q < 4 and (st == "I" or q == 1):
            return [(1, "Q", end)]
        if st == "I":
            prior = self.prev(end, 3)
            if any(f not in QUARTER_FORMS for _, f in prior):
                raise NotComparable(f"the three reports before {end} are not all quarterly: {prior}")
            return [(1, "Y", end)] + [(-1, "Q", d) for d, _ in prior]
        (p, f), = self.prev(end, 1)
        if f not in QUARTER_FORMS:
            raise NotComparable(f"the report before {end} is a {f}, not a quarter of the same year")
        return {2: [(1, "H", end), (-1, "Q", p)], 3: [(1, "N", end), (-1, "H", p)],
                4: [(1, "Y", end), (-1, "N", p)]}[q]

    def prior_end(self, end: str) -> str:
        (p, _), = self.prev(end, 1)
        return p


# ------------------------------------------------------------------------------------ a figure
class Comp:
    """One component: an element's quarter (sum of signed operands) or a stated constant."""

    def __init__(self, role, tag=None, ops=(), unit_class="money", concept=None, const=None):
        self.role, self.tag, self.ops, self.unit_class = role, tag, list(ops), unit_class
        self.concept, self.const = concept, const


def evaluate(co: Company, comps: list[Comp], fn, ours: float, stated_unit: str, t_close: datetime,
             unit_fails: bool) -> dict:
    """Verdict for one figure (README rules)."""
    money_units = [stated_unit] if stated_unit else []
    for c in comps:
        if c.unit_class == "money" and c.tag:
            money_units += [u for u in co.units_of(c.tag) if is_currency(u) and u not in money_units]
    tried = []
    for unit in money_units or [None]:
        res = _evaluate_unit(co, comps, fn, ours, unit, t_close)
        res["unit"] = unit
        tried.append(res)
        if res["verdict"] in ("PASS", "RESTATED", "LOOKAHEAD"):
            break
    first = tried[0]
    hit = tried[-1]
    if hit["verdict"] in ("PASS", "RESTATED", "LOOKAHEAD"):
        if hit["unit"] != stated_unit and stated_unit is not None:
            hit["verdict"] = "UNIT" if unit_fails else "UNIT_NOTED"
            hit["note"] = f"matches EDGAR in {hit['unit']}; the body states {stated_unit}"
        return hit
    # nothing reproduced: report against the stated unit if EDGAR has it, else the first unit it has
    have = [r for r in tried if r["verdict"] != "NOT_COMPARABLE"]
    out = have[0] if have else first
    if not have and len(tried) > 1:
        out["reason"] += f" (units tried: {', '.join(str(r['unit']) for r in tried)})"
    if have and len(tried) > 1:
        out["other_units"] = [{k: r[k] for k in ("unit", "edgar", "reason", "edgar_has_instead") if k in r}
                              for r in tried if r is not out]
    return out


def _evaluate_unit(co, comps, fn, ours, unit, t_close) -> dict:
    opts, layout, notes = [], [], []
    for c in comps:
        if c.const is not None:
            layout.append((c, []))
            continue
        u = unit if c.unit_class == "money" else c.unit_class
        tag = c.tag
        got = [co.options(tag, u, k, e) for _, k, e in c.ops]
        if not all(got):
            missing = [f"{tag} {k} {e}" for (_, k, e), g in zip(c.ops, got) if not g]
            out = {"verdict": "NOT_COMPARABLE", "reason": f"EDGAR has no {u} fact for: " + "; ".join(missing)}
            others = [a for a in ALIASES.get(c.concept, []) if a != tag
                      and all(co.options(a, u, k, e) for _, k, e in c.ops)]
            if others:  # diagnosis only: the declared rule uses the stated element
                out["edgar_has_instead"] = others
            if tag not in co.facts:
                out["reason"] += " (the element is not in companyfacts at all: an extension element?)"
            return out
        idx = []
        for o in got:
            idx.append(len(opts))
            opts.append(o)
        layout.append((c, idx))
        if c.concept in ALIASES and tag in ALIASES[c.concept]:
            first = got[0][0]
            k0, e0 = c.ops[0][1], c.ops[0][2]
            for a in ALIASES[c.concept][:ALIASES[c.concept].index(tag)]:
                if any(o["accn"] == first["accn"] for o in co.options(a, u, k0, e0)):
                    notes.append(f"tag: EDGAR's filing {first['accn']} also reports {a}, ranked before {tag} "
                                 "in the engine's alias order")
                    break

    def value(choice):
        vals = []
        for c, idx in layout:
            if c.const is not None:
                vals.append(c.const)
            else:
                vals.append(sum(sign * opts[i][j]["value"] for (sign, _, _), i, j in
                                zip(c.ops, idx, [choice[i] for i in idx])))
        return fn(vals)

    base = [0] * len(opts)
    edgar = value(base)
    facts = [{**opts[i][0], "sign": sign, "role": c.role} for c, idx in layout for (sign, _, _), i in zip(c.ops, idx)]
    out = {"edgar": edgar, "difference": None if edgar is None else ours - edgar,
           "scaled_difference": None if edgar is None else abs(ours - edgar) / max(1.0, abs(edgar)), "facts": facts}
    if notes:
        out["notes"] = notes
    if edgar is None:
        out.update(verdict="FAIL", reason="EDGAR's figure is undefined (zero denominator)")
        return out
    if close_enough(ours, edgar):
        out["verdict"] = "PASS"
        return out
    space = [range(len(o)) for o in opts]
    n = math.prod(len(o) for o in opts)
    best = None
    if 1 < n <= MAX_COMBOS:
        for choice in itertools.product(*space):
            if not any(choice):
                continue
            v = value(list(choice))
            if v is not None and close_enough(ours, v):
                used = [opts[i][j] for i, j in enumerate(choice) if j]
                latest = max(o["accepted"] for o in used)
                if best is None or latest < best[0]:
                    best = (latest, used, v)
    if best is None:
        out["verdict"] = "FAIL"
        if n > MAX_COMBOS:
            out["reason"] = f"{n} combinations of later filings not searched"
        return out
    latest, used, v = best
    lookahead = datetime.fromisoformat(latest.replace("Z", "+00:00")) > t_close
    out.update(verdict="LOOKAHEAD" if lookahead else "RESTATED", reproduced_with=used, reproduced=v)
    return out


# ------------------------------------------------------------------------------ stored bodies
def coords(body: dict) -> dict:
    return {c["coordinate_id"]: c for c in body.get("coordinates", [])}


def supported(c: dict | None) -> bool:
    return bool(c) and c["support"]["state"] == "SUPPORTED"


METHOD_Q = {"Q2_YTD_MINUS_Q1": 2, "Q3_YTD_MINUS_Q2_YTD": 3, "FY_MINUS_Q3_YTD": 4, "FY_MINUS_Q1_Q2_Q3": 4,
            "FY_DIRECT_ANNUAL_FILER": 4}


def position_q(label: str) -> int:
    m = re.fullmatch(r"FY\d{4}Q([1-4])", label or "")
    if not m:
        raise NotComparable(f"fiscal position {label!r}")
    return int(m.group(1))


def components_of(coord: dict) -> dict:
    """role -> (tag, unit, quarter method, first source period type) from a coordinate's basis."""
    out = {}
    for cb in coord.get("component_bases") or []:
        for comp in cb.get("components", []):
            sf = comp["source_facts"][0]
            out.setdefault(comp["component_role"], (sf["winning_normalized_source_tag"], sf["unit"],
                                                    comp["quarter_method"], sf["period_type"]))
    return out


def q_of_basis(basis: dict) -> int:
    _, _, method, pt = next(iter(basis.values()))
    if method in METHOD_Q:
        return METHOD_Q[method]
    return {"Q1": 1, "Q2": 2, "Q3": 3, "FY": 4}[pt]


def base_comps(co: Company, base: str, basis: dict, end: str, q: int, annual: bool) -> list[Comp]:
    kind, inputs = BASES[base]
    comps = []
    for concept in inputs:
        tag = basis[concept][0]
        comps.append(Comp(concept, tag, co.quarter_ops(concept, end, q, annual), concept=concept))
    if kind == "roa":
        tag = basis.get("prior_total_assets", basis["total_assets"])[0]
        comps.append(Comp("prior_total_assets", tag, [(1, "I", co.prior_end(end))], concept="total_assets"))
    return comps


class Report:
    def __init__(self):
        self.rows: dict[tuple, dict] = {}

    def add(self, d: str, sensor: str, company: str, role: str, scope: str, period: str, ours, res: dict,
            position: str | None = None, judged: bool = True):
        key = (sensor, company, role, scope, period, json.dumps(ours), res["verdict"])
        row = self.rows.get(key)
        if row is None:
            row = {"sensor": sensor, "company": company, "role": role, "scope": scope, "period": period,
                   "position": position, "ours": ours, **{k: v for k, v in res.items()}, "judged": judged,
                   "dates": []}
            if not judged:
                row["verdict"] = "INFO"
                row["edgar_verdict"] = res["verdict"]
            self.rows[key] = row
        row["dates"].append(d)


def run_fnd01(rep: Report, co: Company, d: str, body: dict, t_close: datetime, clock: list):
    cs = coords(body)
    pe = (body.get("private_evidence") or {}).get("bases", {})
    for base, (kind, _) in BASES.items():
        c = cs.get(base)
        if not supported(c):
            continue
        basis = components_of(c)
        annual = any(m == "FY_DIRECT_ANNUAL_FILER" for _, _, m, _ in basis.values()) or not co.quarterly
        stated_unit = next(iter(basis.values()))[1]
        fails = base in CURRENCY_FAILS
        figs = [("current", c["natural_date"], c["value"], pe.get(base, {}).get("current_position"))]
        hist = body.get("arrays", {}).get(f"{base}.history", {})
        figs += [("history", x["fiscal_period_end"], x["value"], x["fiscal_position"])
                 for x in hist.get("cells", []) if x.get("support") == "SUPPORTED"]
        for scope, end, ours, pos in figs:
            try:
                q = position_q(pos) if pos else q_of_basis(basis)
                comps = base_comps(co, base, basis, end, q, annual)
                res = evaluate(co, comps, lambda v, k=kind: apply(k, v), ours, stated_unit, t_close, fails)
            except NotComparable as e:
                res = {"verdict": "NOT_COMPARABLE", "reason": str(e)}
            rep.add(d, "CDD-FND01", co.ticker, base, scope, end, ours, res, pos)
        lf = cs.get(f"{base}.latest_filed_period_end")
        if supported(lf):
            clock.append((d, co.ticker, base, lf["value"]))


def run_fnd02(rep: Report, co: Company, d: str, body: dict, t_close: datetime):
    cs = coords(body)
    for base in FND02_BASES:
        kind = BASES[base][0]
        ar = cs.get(f"{base}.as_reported")
        if not supported(ar):
            continue
        basis = components_of(ar)
        annual = any(m == "FY_DIRECT_ANNUAL_FILER" for _, _, m, _ in basis.values()) or not co.quarterly
        stated_unit = next(iter(basis.values()))[1]
        for rid, judged in ((f"{base}.as_reported", True), (f"{base}.standardized", False)):
            c = cs.get(rid)
            if not supported(c):
                continue
            try:
                comps = base_comps(co, base, basis, ar["natural_date"], q_of_basis(basis), annual)
                res = evaluate(co, comps, lambda v, k=kind: apply(k, v), c["value"], stated_unit, t_close,
                               base in CURRENCY_FAILS)
            except NotComparable as e:
                res = {"verdict": "NOT_COMPARABLE", "reason": str(e)}
            rep.add(d, "CDD-FND02", co.ticker, rid, "current", ar["natural_date"], c["value"], res, judged=judged)


def shares_comp(cover_date: str) -> Comp:
    return Comp("cover_shares", "entitycommonstocksharesoutstanding", [(1, "I", cover_date)], unit_class="shares")


def run_capval(rep: Report, co: Company, d: str, body: dict, t_close: datetime):
    cs = coords(body)
    annual = not co.quarterly

    def put(role, period, ours, comps, fn, unit, extra=None, uncomparable=None):
        try:
            res = evaluate(co, comps, fn, ours, unit, t_close, True)
        except NotComparable as e:
            res = {"verdict": "NOT_COMPARABLE", "reason": str(e)}
        if uncomparable and res["verdict"] != "NOT_COMPARABLE":
            # README: the figure needs EDGAR's input; the rest is shown, not judged
            res = {"verdict": "NOT_COMPARABLE", "reason": uncomparable,
                   "diagnostic_with_stored_input": {k: res.get(k) for k in ("verdict", "edgar", "difference")}}
        if extra:
            res = {**res, **extra}
        rep.add(d, "CDD-CAPVAL01", co.ticker, role, "current", period, ours, res)

    def has_shares(cover: str) -> bool:
        return bool(co.options("entitycommonstocksharesoutstanding", "shares", "I", cover))

    c = cs.get("reported_shares_outstanding")
    if supported(c):
        put("reported_shares_outstanding", c["natural_date"], c["value"], [shares_comp(c["natural_date"])],
            lambda v: v[0], None)
    c = cs.get("public_float_value")
    if supported(c):
        put("public_float_value", c["natural_date"], c["value"],
            [Comp("public_float", "entitypublicfloat", [(1, "I", c["natural_date"])], unit_class="USD")],
            lambda v: v[0], None)

    mc = cs.get("daily_market_cap")
    mcb = (mc.get("component_bases") or [{}])[0] if supported(mc) else {}

    def cap_comps():
        """Market cap's parts: EDGAR's cover shares, or the stored count and why it is not comparable."""
        cover = mcb["shares_cover_date"]
        receipts = (mcb.get("ordinary_shares_per_receipt") or {}).get("value") or 1
        if mcb.get("splits_applied"):
            why = "a split was applied to the market cap's share count"
        elif not has_shares(cover):
            why = f"EDGAR has no non-dimensional cover share count at {cover} (a multi-class filer reports per class)"
        else:
            return [shares_comp(cover)], mcb["raw_close"] / receipts, None
        return [Comp("cover_shares", const=mcb["shares_outstanding"])], mcb["raw_close"] / receipts, why

    ey = cs.get("earnings_yield")
    if supported(ey) and mcb:
        t = ey["component_bases"][0]["trailing_net_income_to_common"]
        try:
            qcomps = []
            for qq in t["quarters"]:
                tags = {s["winning_normalized_source_tag"] for s in qq["source_facts"]}
                if len(tags) != 1:
                    raise NotComparable(f"quarter {qq['fiscal_position']} rests on several elements {sorted(tags)}")
                qn = position_q(qq["fiscal_position"])
                qcomps.append(Comp(qq["fiscal_position"], tags.pop(),
                                   co.quarter_ops("net_income", qq["fiscal_period_end"], qn, annual)))
                put("earnings_yield.quarter", qq["fiscal_period_end"], qq["value"], [qcomps[-1]],
                    lambda v: v[0], t["unit"], {"position": qq["fiscal_position"]})
            put("earnings_yield.trailing_net_income_to_common", t["fiscal_period_end"], t["value"], qcomps,
                lambda v: sum(v), t["unit"])
            sc, px, partial = cap_comps()
            n = len(qcomps)
            put("earnings_yield", d, ey["value"], qcomps + sc, lambda v: sum(v[:n]) / (px * v[n]), t["unit"],
                uncomparable=partial)
        except NotComparable as e:
            rep.add(d, "CDD-CAPVAL01", co.ticker, "earnings_yield", "current", d, ey["value"],
                    {"verdict": "NOT_COMPARABLE", "reason": str(e)})

    pb = cs.get("price_to_book")
    if supported(pb) and mcb:
        e = pb["component_bases"][0]["common_shareholders_equity"]
        end = e["fiscal_period_end"]
        eq_tag = e["total_equity"]["source_facts"][0]["winning_normalized_source_tag"]
        comps = [Comp("total_equity", eq_tag, [(1, "I", end)])]
        pref = e.get("preferred_equity")
        if pref and pref.get("source_facts"):
            ptag = pref["source_facts"][0]["winning_normalized_source_tag"]
            if pref["value"] == 0 and not co.options(ptag, e["unit"], "I", end):
                comps.append(Comp("preferred_equity", const=0.0))
            else:
                comps.append(Comp("preferred_equity", ptag, [(1, "I", end)]))
        else:
            comps.append(Comp("preferred_equity", const=0.0))
        put("price_to_book.common_shareholders_equity", end, e["value"], comps[:2], lambda v: v[0] - v[1], e["unit"])
        sc, px, partial = cap_comps()
        put("price_to_book", d, pb["value"], comps[:2] + sc, lambda v: px * v[2] / (v[0] - v[1]), e["unit"],
            uncomparable=partial)

    fr = cs.get("float_to_latest_reported_shares")
    if supported(fr):
        den = cs.get("float_to_latest_reported_shares.denominator_effective_date", {}).get("value")
        fs = cs.get("public_float_shares", {}).get("value")
        sb = ((body.get("private_evidence") or {}).get("split_basis") or {}).get("applied", {})
        splits = sb.get("float_to_latest_reported_shares") or {}
        if splits.get("numerator") or splits.get("denominator"):
            rep.add(d, "CDD-CAPVAL01", co.ticker, "float_to_latest_reported_shares", "current", den, fr["value"],
                    {"verdict": "NOT_COMPARABLE", "reason": "a split was applied to the ratio's shares"})
        elif den and fs:
            put("float_to_latest_reported_shares", den, fr["value"], [shares_comp(den)], lambda v: fs / v[0], None,
                {"partial": "the provider's float shares as stated; only the denominator is EDGAR's"})


def judge_clock(cos: dict, clock: list) -> list:
    out = {}
    for d, t, base, ours in clock:
        co = cos[t]
        tc = close_of(d)
        seen = [(rd, f, acc) for f, rd, acc in co.filings.values()
                if f in PERIODIC and rd and acc <= tc and (co.quarterly or f in ANNUAL_FORMS)]
        latest = max(seen)[0] if seen else None
        if latest == ours:
            v = "MATCH"
        elif latest is None or ours > latest:
            v = "NOT_COMPARABLE" if not co.quarterly else "LOOKAHEAD"
        else:
            v = "LAG"
        key = (t, d, ours, latest, v)
        out.setdefault(key, {"company": t, "date": d, "ours": ours, "edgar_latest_by_close": latest,
                             "verdict": v, "bases": []})["bases"].append(base)
        if v == "LAG":
            nxt = sorted((acc, rd, f) for f, rd, acc in co.filings.values() if f in PERIODIC and rd and rd > ours)
            out[key]["edgar_filing"] = [{"form": f, "period": rd, "accepted": acc.isoformat()} for acc, rd, f in nxt[:1]]
    return list(out.values())


FAILING = {"FAIL", "LOOKAHEAD", "UNIT"}


def compare(cache: str, bodies: str) -> int:
    cache_p = Path(cache)
    cos: dict[str, Company] = {}
    rep, clock, dates = Report(), [], set()
    with open(bodies) as fh:
        for line in fh:
            if not line.strip():
                continue
            d, mid, obj, text = line.rstrip("\n").split("\t", 3)
            dates.add(d)
            co = cos.get(obj) or cos.setdefault(obj, Company(cache_p, obj))
            body = json.loads(text)
            tc = close_of(d)
            if mid == "CDD-FND01":
                run_fnd01(rep, co, d, body, tc, clock)
            elif mid == "CDD-FND02":
                run_fnd02(rep, co, d, body, tc)
            elif mid == "CDD-CAPVAL01":
                run_capval(rep, co, d, body, tc)
    rows = sorted(rep.rows.values(), key=lambda r: (r["sensor"], r["company"], r["role"], r["scope"], r["period"]))
    clk = judge_clock(cos, clock)

    def tally(rs):
        by = defaultdict(Counter)
        for r in rs:
            by[f"{r['sensor']} {r['role']}"][r["verdict"]] += 1
        return {k: dict(v) for k, v in sorted(by.items())}

    current = [r for r in rows if r["scope"] == "current"]
    fetched = json.loads((cache_p / "fetched.json").read_text())
    summary = {
        "figures": len(rows),
        "verdicts": dict(Counter(r["verdict"] for r in rows)),
        "verdicts_current_readings": dict(Counter(r["verdict"] for r in current)),
        "by_sensor_role": tally(rows),
        "clock": dict(Counter(c["verdict"] for c in clk)),
        "pass": not any(r["verdict"] in FAILING for r in rows) and not any(c["verdict"] == "LOOKAHEAD" for c in clk),
        "criterion_5_latest_quarter": not any(r["verdict"] in FAILING for r in current),
        "pass_exact": sum(1 for r in rows if r["verdict"] == "PASS" and r.get("difference") == 0),
        "pass_within_tolerance_not_exact": [{k: r.get(k) for k in ("sensor", "company", "role", "scope", "period",
                                                                    "ours", "edgar", "scaled_difference")}
                                            for r in rows if r["verdict"] == "PASS" and r.get("difference")],
        "failing": [{k: r.get(k) for k in ("sensor", "company", "role", "scope", "period", "ours", "edgar",
                                            "difference", "verdict", "note", "reason")} for r in rows
                    if r["verdict"] in FAILING],
        "not_comparable": dict(Counter(f"{r['company']} {r['sensor']} {r.get('reason', '')[:120]}"
                                       for r in rows if r["verdict"] == "NOT_COMPARABLE")),
    }
    report = {
        "check": "SEC EDGAR companyfacts against the stored company figures (tools/external/README.md)",
        "tolerance": f"|ours - EDGAR| <= {TOL} * max(1, |EDGAR|)",
        "dates": sorted(dates),
        "sec_files": [{k: f[k] for k in ("url", "sha256", "fetched_at")} for f in fetched],
        "summary": summary,
        "clock": sorted(clk, key=lambda c: (c["company"], c["date"])),
        "figures": rows,
    }
    json.dump(report, sys.stdout, indent=1, default=str)
    print()
    return 0 if summary["pass"] else 1


# ------------------------------------------------------------- CDD-EVENTS01 filing times (README)
EVENTS_PERIODIC = {"10-K", "10-Q", "20-F", "40-F"}
EVENTS_SEEN: set[tuple[str, str]] = set()  # nothing compared before the rules (README)
WINDOW_DAYS = 366


def submissions(cache: Path, ticker: str) -> list[dict]:
    """Every filing in the cached submissions of one filer, acceptance as an aware UTC instant."""
    out = {}
    for p in sorted(cache.glob(f"submissions-{ticker}*.json")):
        j = json.loads(p.read_text())
        r = j["filings"]["recent"] if "filings" in j else j
        for i, accn in enumerate(r["accessionNumber"]):
            out[accn] = {"accn": accn, "form": r["form"][i], "report_date": r["reportDate"][i],
                         "filing_date": r["filingDate"][i], "items": r["items"][i],
                         "accepted": datetime.fromisoformat(r["acceptanceDateTime"][i].replace("Z", "+00:00"))}
    return list(out.values())


def _instant(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def _iso(t: datetime) -> str:
    return t.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def events_one(d: str, obj: str, body: dict, filings: list[dict]) -> list[dict]:
    tc = close_of(d)
    seen = (d, obj) in EVENTS_SEEN
    cs = {c["coordinate_id"]: c for c in body.get("coordinates", [])}
    out = []

    def row(kind: str, verdict: str, **kw) -> None:
        out.append({"date": d, "company": obj, "seen": seen, "item": kind, "verdict": verdict, **kw})

    eights = [f for f in filings if f["form"] == "8-K"]
    by_time = {f["accepted"].replace(microsecond=0): f for f in eights}
    # New York wall clock written as UTC: the reading the README's ZONE rule refuses
    by_ny_clock = {f["accepted"].astimezone(NY).replace(tzinfo=timezone.utc, microsecond=0): f for f in eights}

    def judge_8k(kind: str, value: str) -> None:
        t = _instant(value).replace(microsecond=0)
        if t > tc:
            row(kind, "LOOKAHEAD", ours=value, reason="accepted after T's close")
        elif t in by_time:
            f = by_time[t]
            row(kind, "MATCH", ours=value, accn=f["accn"], items=f["items"], item_2_02="2.02" in f["items"].split(","))
        elif t in by_ny_clock:
            f = by_ny_clock[t]
            row(kind, "FAIL", ours=value, reason="ZONE: equals the New York clock of an 8-K's acceptance",
                accn=f["accn"], edgar=_iso(f["accepted"]))
        else:
            near = min(eights, key=lambda f: abs((f["accepted"] - t).total_seconds()), default=None)
            row(kind, "FAIL", ours=value, reason="no original 8-K of the filer accepted at that second",
                nearest=near and {"accn": near["accn"], "accepted": _iso(near["accepted"]), "items": near["items"]})

    c = cs.get("sec_filing.earnings_release_8k_at")
    if c is None or c["support"]["state"] != "SUPPORTED" or not c.get("value"):
        row("earnings_release_8k_at", "NOT_COMPARABLE",
            reason=c and f"{c['support']['state']} {c['support'].get('reason_code')}" or "absent")
    else:
        judge_8k("earnings_release_8k_at", c["value"])
    h = body.get("arrays", {}).get("sec_filing.earnings_release_8k_at.history")
    if h is None or h.get("state") != "SUPPORTED":
        row("earnings_release_8k_at.history", "NOT_COMPARABLE",
            reason=h and f"{h.get('state')} {h.get('reason')}" or "absent")
    else:
        cells = h["cells"]
        for v in cells:
            judge_8k("earnings_release_8k_at.history", v)
        have = {_instant(v).replace(microsecond=0) for v in cells}
        start = date.fromisoformat(d) - timedelta(days=WINDOW_DAYS)
        for f in eights:
            ny = f["accepted"].astimezone(NY).date()
            if "2.02" in f["items"].split(",") and f["accepted"] <= tc and ny >= start \
                    and f["accepted"].replace(microsecond=0) not in have:
                row("earnings_release_8k_at.history", "FAIL",
                    reason="EDGAR item-2.02 8-K accepted by T's close, inside the window, missing from the history",
                    accn=f["accn"], edgar=_iso(f["accepted"]), items=f["items"],
                    near_window_start=(ny - start).days <= 3)

    keys = ("report_type", "period_end", "filing_date", "accepted_at")
    pc = {k: cs.get(f"sec_filing.{k}") for k in keys}
    if any(x is None or x["support"]["state"] != "SUPPORTED" for x in pc.values()):
        bad = {k: (x and x["support"]["state"]) for k, x in pc.items()}
        row("periodic_report", "NOT_COMPARABLE", reason=f"coordinates not all SUPPORTED: {bad}")
        return out
    ours = {k: pc[k]["value"] for k in keys}
    cands = [f for f in filings if f["form"] == ours["report_type"] and f["report_date"] == ours["period_end"]]
    acc = _instant(ours["accepted_at"]).replace(microsecond=0)
    exact = [f for f in cands if f["filing_date"] == ours["filing_date"] and f["accepted"].replace(microsecond=0) == acc]
    if exact:
        row("periodic_report", "MATCH", ours=ours, accn=exact[0]["accn"])
    else:
        ed = [{"accn": f["accn"], "filing_date": f["filing_date"], "accepted": _iso(f["accepted"])} for f in cands]
        if not cands:
            why = "no EDGAR filing of that form and period"
        elif ours["accepted_at"].endswith("T00:00:00Z"):
            why = "accepted_at is midnight UTC (a date stated as an instant); EDGAR's acceptance differs"
        else:
            why = "filing date or acceptance differs from EDGAR's"
        row("periodic_report", "FAIL", ours=ours, edgar=ed, reason=why)
    known = [f for f in filings if f["form"] in EVENTS_PERIODIC and f["report_date"] and f["accepted"] <= tc]
    latest = max(known, key=lambda f: f["accepted"], default=None)
    if acc > tc:
        row("periodic_report_clock", "LOOKAHEAD", ours=ours)
    elif latest is None:
        row("periodic_report_clock", "NOT_COMPARABLE", reason="no original periodic report in EDGAR by T's close")
    elif (latest["form"], latest["report_date"]) == (ours["report_type"], ours["period_end"]):
        row("periodic_report_clock", "MATCH", ours=ours)
    elif ours["report_type"] not in EVENTS_PERIODIC:
        row("periodic_report_clock", "NOT_COMPARABLE",
            reason=f"ours is a {ours['report_type']}, not a periodic XBRL report form",
            edgar_latest={"form": latest["form"], "period": latest["report_date"], "accepted": _iso(latest["accepted"])})
    elif latest["report_date"] > ours["period_end"]:
        row("periodic_report_clock", "LAG", ours=ours,
            edgar_latest={"form": latest["form"], "period": latest["report_date"], "accepted": _iso(latest["accepted"])})
    else:
        row("periodic_report_clock", "FAIL", ours=ours, reason="ours is not EDGAR's latest periodic report by T's close",
            edgar_latest={"form": latest["form"], "period": latest["report_date"], "accepted": _iso(latest["accepted"])})
    return out


def events(cache: str, bodies: str) -> int:
    cache_p = Path(cache)
    subs: dict[str, list] = {}
    rows, dates = [], set()
    with open(bodies) as fh:
        for line in fh:
            if not line.strip():
                continue
            d, mid, obj, text = line.rstrip("\n").split("\t", 3)
            if mid != "CDD-EVENTS01":
                continue
            dates.add(d)
            if obj not in subs:
                subs[obj] = submissions(cache_p, obj)
            rows += events_one(d, obj, json.loads(text), subs[obj])
    tally = defaultdict(Counter)
    for r in rows:
        tally[r["item"]][f"{r['verdict']} {'seen' if r['seen'] else 'blind'}"] += 1
    bad = [r for r in rows if r["verdict"] in ("FAIL", "LOOKAHEAD")]
    report = {
        "check": "EDGAR filing times against CDD-EVENTS01 (tools/external/README.md)",
        "dates": sorted(dates),
        "sec_files": json.loads((cache_p / "fetched.json").read_text()),
        "summary": {k: dict(v) for k, v in sorted(tally.items())},
        "pass": not bad, "pass_blind": not any(not r["seen"] for r in bad),
        "failures": bad,
        "rows": rows,
    }
    json.dump(report, sys.stdout, indent=1)
    print()
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "fetch":
        sys.exit(fetch(sys.argv[2], sys.argv[3:]))
    if cmd == "compare":
        sys.exit(compare(sys.argv[2], sys.argv[3]))
    if cmd == "events":
        sys.exit(events(sys.argv[2], sys.argv[3]))
    raise SystemExit(__doc__)
