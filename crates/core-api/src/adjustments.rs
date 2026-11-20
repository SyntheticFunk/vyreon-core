//! C-055: the OCC adjustment ledger core sends in every claim, beside the dated tickers. The
//! engine writes it to `<home>/h5-universe/adjustments.json` and reads it as
//! `VYREON-ADJUSTMENTS@1` (engine `crates/vyreon-cli/src/adjustments.rs`, `parse_ledger`; audit 9
//! finding 1, D-290..D-294): each member's dated OCC contract adjustments, so an adjustment older
//! than the engine's own detector lookback (21 sessions) still explains the provider's stale
//! pre-adjustment listings (EWJ's 2025-12-16 strike reduction withheld its open interest on 100
//! sessions without one).
//!
//! The ledger is committed data (`data/occ-adjustments.json`), compiled into the binary: every
//! claim a core build answers carries the same ledger, a change is a reviewed commit, and a new
//! database needs no load step. Each entry is in the engine's vocabulary and nothing else:
//!
//!   {"ex_date": "2025-12-05", "kind": "split", "ratio": 0.5, "evidence": "..."}       an n-for-1 forward split
//!                                                                                   (the provider's split_ratio, 1/n)
//!   {"ex_date": "2025-12-16", "kind": "other", "strike_offset": 0.16, "special_dividend": 0.157024,
//!    "evidence": "..."}                                                              every strike lowered by a
//!                                                                                   special distribution
//!   {"ex_date": "2024-01-24", "kind": "other", "root": "UNG1", "evidence": "..."}      positions moved to an adjusted
//!                                                                                   root (a changed deliverable)
//!
//! A strike offset states the special behind it (engine D-120 as amended 2026-11-20: the special
//! is not projected as a dividend, the rest of the payment is): `special_dividend`, OCC's exact
//! special per share from the memo, and `special_ex_date` where the cash went ex before the
//! strikes were reduced (FXI 2023: cash 06-07, strikes 06-08, memo #52545).
//!
//! Core adds its own rules to the engine's parser: every entry is backed by an OCC Information
//! Memo, cited by number and URL at the head of its evidence (no entry without a source, engine
//! D-102); a reverse or fractional split is never a `split` (the engine cannot place it: its
//! positions move to the adjusted root, given as `root`); the engine's `unknown` (a possible
//! adjustment) is never sent, since a sourced adjustment is certain; an asset's entries are in
//! ex-date order without repeats; every strike offset states its special, the offset is that
//! special rounded to the nearest penny (OCC's rule, as each memo states it), and its
//! `special_ex_date`, if any, is not after the strike reduction.
use core_contract::is_date;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// The engine's schema for the ledger (`vyreon_cli::adjustments::LEDGER_SCHEMA`).
pub const SCHEMA: &str = "VYREON-ADJUSTMENTS@1";

/// The committed ledger.
pub const LEDGER_FILE: &str = include_str!("../data/occ-adjustments.json");

/// Where an entry's memo is published (OCC's public Information Memo site).
pub const MEMO_URL: &str = "https://infomemo.theocc.com/infomemos?number=";

/// The memo number an entry's evidence opens with: `OCC Information Memo #N (<MEMO_URL>N)`.
fn memo(evidence: &str) -> Option<u32> {
    let rest = evidence.strip_prefix("OCC Information Memo #")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let n: u32 = digits.parse().ok()?;
    rest[digits.len()..].starts_with(&format!(" ({MEMO_URL}{n})")).then_some(n)
}

/// Checks a ledger document against the engine's parser and core's rules above.
pub fn check(doc: &Value) -> Result<(), String> {
    if doc["schema"] != SCHEMA {
        return Err(format!("not {SCHEMA}"));
    }
    let assets = doc["assets"].as_object().ok_or("no assets object")?;
    for (asset, entries) in assets {
        let entries = entries.as_array().filter(|l| !l.is_empty()).ok_or_else(|| format!("{asset}: not a list of entries"))?;
        let mut seen = BTreeSet::new();
        let mut last = String::new();
        for e in entries {
            let e = e.as_object().ok_or_else(|| format!("{asset}: an entry that is not an object"))?;
            let ex = e.get("ex_date").and_then(Value::as_str).filter(|d| is_date(d)).ok_or_else(|| format!("{asset}: an entry without an ex_date"))?;
            let at = format!("{asset} {ex}");
            if let Some(k) = e.keys().find(|k| !["ex_date", "kind", "ratio", "root", "strike_offset", "special_dividend", "special_ex_date", "evidence"].contains(&k.as_str())) {
                return Err(format!("{at}: a field the engine does not read ({k})"));
            }
            let evidence = e.get("evidence").and_then(Value::as_str).unwrap_or_default();
            memo(evidence).ok_or_else(|| format!("{at}: the evidence does not open with its OCC Information Memo and URL"))?;
            let num = |k: &str| e.get(k).map(|v| v.as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or_else(|| format!("{at}: {k} is not a positive number")));
            let image = match (e.get("kind").and_then(Value::as_str), num("ratio").transpose()?, e.get("root"), num("strike_offset").transpose()?) {
                (Some("split"), Some(r), None, None) => {
                    // the engine's `split::forward_split`
                    let n = (1.0 / r).round();
                    if r >= 1.0 || n < 2.0 || ((1.0 / r) - n).abs() > 1e-6 * n {
                        return Err(format!("{at}: ratio {r} is not a whole forward split (a reverse or fractional split is the adjusted root)"));
                    }
                    format!("split:{n}")
                }
                (Some("other"), None, Some(root), None) => {
                    let root = root.as_str().filter(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
                    format!("root:{}", root.ok_or_else(|| format!("{at}: not an option root"))?)
                }
                (Some("other"), None, None, Some(d)) => {
                    let special = num("special_dividend").transpose()?.ok_or_else(|| format!("{at}: a strike offset without its special_dividend (OCC's special per share)"))?;
                    if ((special * 100.0).round() / 100.0 - d).abs() > 1e-9 {
                        return Err(format!("{at}: strike offset {d} is not the special {special} rounded to the nearest penny"));
                    }
                    if let Some(when) = e.get("special_ex_date") {
                        when.as_str().filter(|w| is_date(w) && *w <= ex).ok_or_else(|| format!("{at}: special_ex_date is not a date on or before the ex_date"))?;
                    }
                    format!("shift:{d}")
                }
                _ => return Err(format!("{at}: not a split, an adjusted root or a strike offset")),
            };
            if e.get("strike_offset").is_none() && (e.contains_key("special_dividend") || e.contains_key("special_ex_date")) {
                return Err(format!("{at}: a special dividend on an entry that is no strike offset"));
            }
            if ex < last.as_str() || !seen.insert((ex.to_owned(), image)) {
                return Err(format!("{at}: out of ex-date order or repeated"));
            }
            last = ex.to_owned();
        }
    }
    Ok(())
}

/// The committed ledger, checked once; a ledger that fails its check answers every claim with
/// the error (the tests check it before a build ships).
pub fn ledger() -> Result<&'static Value, String> {
    static LEDGER: OnceLock<Result<Value, String>> = OnceLock::new();
    LEDGER
        .get_or_init(|| {
            let doc: Value = serde_json::from_str(LEDGER_FILE).map_err(|e| format!("occ-adjustments.json: {e}"))?;
            check(&doc).map_err(|e| format!("occ-adjustments.json: {e}"))?;
            Ok(doc)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The claim's `adjustments`: the ledger's entries of the given asset keys (the claim's members),
/// as the engine's `assets` object. An object, empty when none has an entry.
pub fn for_members<'a>(members: impl IntoIterator<Item = &'a str>) -> Result<Value, String> {
    let assets = &ledger()?["assets"];
    let mut out = Map::new();
    for key in members {
        if let Some(entries) = assets.get(key) {
            out.insert(key.to_owned(), entries.clone());
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn committed() -> Value {
        serde_json::from_str(LEDGER_FILE).unwrap()
    }

    /// The committed ledger passes, names only members of the engine's 64-asset profile, and
    /// holds the two strike reductions that motivated it (EWJ withheld its open interest on 100
    /// sessions without the first; engine D-290 names the second).
    #[test]
    fn the_committed_ledger_is_well_formed_and_names_members() {
        let doc = ledger().unwrap();
        let members = test_support::slice::listed_2025_12_10();
        for asset in doc["assets"].as_object().unwrap().keys() {
            assert!(members.contains(asset), "{asset} is not a member");
        }
        let entry = |asset: &str, ex: &str| doc["assets"][asset].as_array().unwrap().iter().find(|e| e["ex_date"] == ex).cloned().unwrap();
        assert_eq!((entry("EWJ", "2025-12-16")["kind"].as_str(), entry("EWJ", "2025-12-16")["strike_offset"].as_f64()), (Some("other"), Some(0.16)));
        assert_eq!(entry("EMB", "2024-12-30")["strike_offset"].as_f64(), Some(0.25));
        assert_eq!(entry("XLE", "2025-12-05")["ratio"].as_f64(), Some(0.5));
        assert_eq!(entry("UNG", "2024-01-24")["root"], "UNG1");
        assert_eq!(memo(entry("EWJ", "2025-12-16")["evidence"].as_str().unwrap()), Some(57919));
        // the specials behind the four strike offsets, OCC's amounts (memos #57919, #52545, #53847, #55816)
        let special = |asset: &str, ex: &str| (entry(asset, ex)["special_dividend"].as_f64(), entry(asset, ex)["special_ex_date"].as_str().map(str::to_owned));
        assert_eq!(special("EWJ", "2025-12-16"), (Some(0.157024), None));
        assert_eq!(special("FXI", "2023-06-08"), (Some(0.148983), Some("2023-06-07".to_owned())));
        assert_eq!(special("QQQ", "2023-12-27"), (Some(0.21584), None));
        assert_eq!(special("EMB", "2024-12-30"), (Some(0.254515), None));
        let offsets = doc["assets"].as_object().unwrap().values().flat_map(|l| l.as_array().unwrap()).filter(|e| e.get("strike_offset").is_some()).count();
        assert_eq!(offsets, 4);
    }

    /// Each of core's rules refuses a real entry altered to break it.
    #[test]
    fn an_entry_that_breaks_a_rule_is_refused() {
        let altered = |asset: &str, ex: &str, change: &dyn Fn(&mut Map<String, Value>)| {
            let mut doc = committed();
            let e = doc["assets"][asset].as_array_mut().unwrap().iter_mut().find(|e| e["ex_date"] == ex).unwrap();
            change(e.as_object_mut().unwrap());
            check(&doc).unwrap_err()
        };
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("evidence".into(), json!("strike shift"));
        })
        .contains("OCC Information Memo"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("evidence".into(), json!("OCC Information Memo #57919 (https://infomemo.theocc.com/infomemos?number=57918)"));
        })
        .contains("OCC Information Memo"));
        // UNG's 1-for-4 reverse split as the provider states it (split_ratio 4.0)
        assert!(altered("UNG", "2024-01-24", &|e| {
            e.remove("root");
            e.insert("kind".into(), json!("split"));
            e.insert("ratio".into(), json!(4.0));
        })
        .contains("not a whole forward split"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.remove("strike_offset");
            e.insert("kind".into(), json!("unknown"));
        })
        .contains("not a split"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("root".into(), json!("EWJ1"));
        })
        .contains("not a split"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("strike_offset".into(), json!(-0.16));
        })
        .contains("not a positive number"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("memo".into(), json!(57919));
        })
        .contains("does not read"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("ex_date".into(), json!("2025-12-1"));
        })
        .contains("without an ex_date"));
        // a strike offset without its special, or one that is not the special to the penny
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.remove("special_dividend");
        })
        .contains("without its special_dividend"));
        assert!(altered("EWJ", "2025-12-16", &|e| {
            e.insert("special_dividend".into(), json!(0.16));
            e.insert("strike_offset".into(), json!(0.17));
        })
        .contains("nearest penny"));
        assert!(altered("FXI", "2023-06-08", &|e| {
            e.insert("special_ex_date".into(), json!("2023-06-09"));
        })
        .contains("special_ex_date"));
        assert!(altered("XLE", "2025-12-05", &|e| {
            e.insert("special_dividend".into(), json!(0.1));
        })
        .contains("no strike offset"));
        let mut doc = committed();
        let xle = doc["assets"]["XLE"].as_array_mut().unwrap();
        xle.push(xle[0].clone());
        assert!(check(&doc).unwrap_err().contains("repeated"));
        let mut doc = committed();
        doc["schema"] = json!("VYREON-ADJUSTMENTS@2");
        assert!(check(&doc).is_err());
    }

    /// A claim carries only its members' entries.
    #[test]
    fn a_claim_carries_its_members_entries() {
        let some = for_members(["EWJ", "AAPL", "XLE"]).unwrap();
        assert_eq!(some.as_object().unwrap().keys().collect::<Vec<_>>(), ["EWJ", "XLE"]);
        assert_eq!(some["EWJ"], ledger().unwrap()["assets"]["EWJ"]);
        assert_eq!(for_members([]).unwrap(), json!({}));
    }
}
