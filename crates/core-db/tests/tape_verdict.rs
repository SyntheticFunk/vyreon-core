//! C-058 (engine D-370 amended 2026-11-20): the tape worker's verdict on each tape cache it
//! stores, kept once beside the cache, served with it, and judged: a FAIL on an asset-session's
//! newest cache is a disagreement (REFCHECK_DISAGREES), none or NOT_RUN a cache not verified (the
//! digest and `show`). The cache is the real one (`test_support::TAPE_AAPL`, AAPL's of 2025-12-10).
//! The PASS and FAIL verdicts are real (D-102): the tape reader's on AAPL's cache of 2025-12-01 on
//! the cluster (`test_support::TAPE_CHECK_AAPL_PASS`, `_FAIL`), bound to this cache (asset,
//! session, method version and cache digest changed, nothing else). No real NOT_RUN exists on the
//! cluster: the NOT_RUN is the real PASS with only its outcome, reader digest and reason changed,
//! the reason the reference suite's own answer on this cache (it holds no raw tape:
//! `tools/reference/tests/test_tape_verdict.py` in the engine). Run with `cargo test -- --ignored`.
use core_contract::sha256_hex;
use core_db::refcheck;
use core_db::tape::{self, Cache};
use core_db::watch::{self, Settings};
use core_db::{migrate, work};
use serde_json::{Value, json};
use test_support::{Scratch, TAPE_AAPL, TAPE_CHECK_AAPL_FAIL, TAPE_CHECK_AAPL_PASS, tape_verdict_bound};

const S: &str = "2025-12-10";
const V1: &str = "engine-v1";
const LATER: &str = "2099-01-01T00:00:00Z";

fn verdict(method: &str, outcome: &str) -> Value {
    let real = if outcome == "FAIL" { TAPE_CHECK_AAPL_FAIL } else { TAPE_CHECK_AAPL_PASS };
    let mut v = tape_verdict_bound(real, "AAPL", S, method, &sha256_hex(TAPE_AAPL));
    if outcome == "NOT_RUN" {
        v["outcome"] = json!("NOT_RUN");
        v["reader_bins_sha256"] = Value::Null;
        v["why"] = json!("the reference reader failed: RawTapeAbsent: AAPL 2025-12-10: UNVERIFIABLE: raw tape absent: trades.raw (manifest: 2801962 bytes, sha256 758e3d0669b1a80a), quotes.raw (manifest: 2374468 bytes, sha256 b7017f531e964d1c) at intraday.py:545");
    }
    v
}

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

#[test]
fn a_verdict_is_checked_before_anything_is_stored() {
    let pass = verdict(V1, "PASS");
    let ok = tape::check_verdict(&bytes(&pass), "AAPL", S, V1).unwrap();
    assert_eq!((ok.outcome.as_str(), ok.differences, ok.why.as_deref()), ("PASS", 0, None));
    let fail = tape::check_verdict(&bytes(&verdict(V1, "FAIL")), "AAPL", S, V1).unwrap();
    // the real FAIL lists no bin differences: its reason names the field ("conditional_last_rejected
    // reader 38, cache 39")
    assert_eq!((fail.outcome.as_str(), fail.differences, fail.why.as_deref()), ("FAIL", 0, Some("the reference reader's bins differ from the engine's cache: conditional_last_rejected reader 38, cache 39")));
    assert!(tape::check_verdict(&bytes(&verdict(V1, "NOT_RUN")), "AAPL", S, V1).unwrap().reader_bins_sha256.is_none());
    let refused = |v: Value, asset: &str, says: &str| {
        let e = tape::check_verdict(&bytes(&v), asset, S, V1).unwrap_err();
        assert!(e.contains(says), "{e} should say {says}");
    };
    refused(pass.clone(), "MSFT", "asset");
    refused(verdict("engine-v2", "PASS"), "AAPL", "method_version");
    let with = |k: &str, x: Value| {
        let mut v = pass.clone();
        v[k] = x;
        v
    };
    refused(with("schema", json!("VYREON-TAPE-VERDICT@0")), "AAPL", "schema");
    refused(with("outcome", json!("MAYBE")), "AAPL", "outcome must be one of");
    refused(with("cache_sha256", json!("abc")), "AAPL", "packed cache's sha256");
    refused(with("reader_bins_sha256", Value::Null), "AAPL", "names the reader's bins");
    refused(with("differences", json!([[1, "volume", 1, 2]])), "AAPL", "PASS tape verdict lists differences");
    let mut v = verdict(V1, "FAIL");
    v["why"] = Value::Null;
    refused(v, "AAPL", "says why");
    let mut v = verdict(V1, "NOT_RUN");
    v["why"] = json!("the suite ended: token=abcdef0123456789");
    refused(v, "AAPL", "unredacted");
    assert!(tape::check_verdict(&vec![b' '; tape::MAX_VERDICT_BYTES + 1], "AAPL", S, V1).unwrap_err().contains("at most"));
}

async fn problems(client: &tokio_postgres::Client) -> Vec<(String, String)> {
    client
        .query("SELECT kind, summary FROM core.problem WHERE resolved_at IS NULL AND kind LIKE 'REFCHECK%' ORDER BY kind", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_verdict_is_kept_once_beside_its_cache_and_a_failure_is_a_disagreement_until_the_cache_is_redone() {
    let scratch = Scratch::new("tape_verdict").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let check = |v: &Value, method: &str| tape::check_verdict(&bytes(v), "AAPL", S, method).unwrap();

    // no cache, no verdict; a stated absence is no cache to judge either
    let e = tape::put_verdict(&client, "AAPL", S, V1, &check(&verdict(V1, "NOT_RUN"), V1)).await.unwrap_err();
    assert!(e.contains("no tape cache is stored to judge"), "{e}");
    tape::put(&client, "SPY", S, V1, &Cache::Absent("NOT_CAPTURED: only AAPL's tape was fetched for this test".into())).await.unwrap();
    let mut spy = verdict(V1, "NOT_RUN");
    spy["asset"] = json!("SPY");
    let e = tape::put_verdict(&client, "SPY", S, V1, &tape::check_verdict(&bytes(&spy), "SPY", S, V1).unwrap()).await.unwrap_err();
    assert!(e.contains("a stated absence, not a cache"), "{e}");

    // the real cache, and the suite's own NOT_RUN on it: kept once; a cache with no verdict, or a
    // NOT_RUN one, is not verified (the digest says so) and no problem
    assert!(tape::put(&client, "AAPL", S, V1, &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    let settings = Settings { profile: "FREE_REPORT_64".into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None, backup_days: None };
    let now: String = client.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0);
    assert_eq!(refcheck::tape_digest_line(&client, &now).await.unwrap().unwrap(), "Tape caches stored in the last 24 hours: 1 (0 verified by the tape reader, 0 disagree, 0 withheld whole, 1 not verified)");
    let not_run = verdict(V1, "NOT_RUN");
    assert!(tape::put_verdict(&client, "AAPL", S, V1, &check(&not_run, V1)).await.unwrap());
    // the same document again changes nothing; another is refused
    assert!(!tape::put_verdict(&client, "AAPL", S, V1, &check(&not_run, V1)).await.unwrap());
    let e = tape::put_verdict(&client, "AAPL", S, V1, &check(&verdict(V1, "PASS"), V1)).await.unwrap_err();
    assert!(e.contains("a verdict is already stored for this cache"), "{e}");
    assert_eq!(tape::verdicts_of_session(&client, S, V1).await.unwrap(), vec![("AAPL".to_owned(), not_run.clone())]);
    assert!(tape::verdicts_of_session(&client, S, "engine-v2").await.unwrap().is_empty());
    assert!(watch::check(&mut client, &settings, &now).await.unwrap().opened.iter().all(|l| !l.starts_with("Reference check")));
    let show = refcheck::tape_show(&client, S).await.unwrap();
    assert_eq!((show["caches"].as_i64(), show["counts"].clone()), (Some(2), json!({"NOT_RUN": 1, "stated absence": 1})));
    assert_eq!(show["not_passed"][0]["asset"], "AAPL");

    // the session redone under a new method: a FAIL on the newest cache is a disagreement to look at
    const V2: &str = "engine-v2";
    assert!(tape::put(&client, "AAPL", S, V2, &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    let mut other = verdict(V2, "FAIL");
    other["cache_sha256"] = json!("c".repeat(64));
    let e = tape::put_verdict(&client, "AAPL", S, V2, &check(&other, V2)).await.unwrap_err();
    assert!(e.contains("judged other bytes than the cache stored"), "{e}");
    assert!(tape::put_verdict(&client, "AAPL", S, V2, &check(&verdict(V2, "FAIL"), V2)).await.unwrap());
    let state = refcheck::state(&client, "FREE_REPORT_64", None, LATER).await.unwrap();
    assert_eq!((state.tape_failed, state.tape_failed_oldest.as_deref()), (1, Some(S)));
    let said = "Reference check: the tape reader disagrees with the engine's tape cache on 1 asset-session, the oldest 2025-12-10";
    assert_eq!(state.disagree_summary().as_deref(), Some(said));
    let now: String = client.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0);
    let opened: Vec<String> = watch::check(&mut client, &settings, &now).await.unwrap().opened.into_iter().filter(|l| l.starts_with("Reference check")).collect();
    assert_eq!(opened, [said]);
    assert_eq!(problems(&client).await, [("REFCHECK_DISAGREES".to_owned(), said.to_owned())]);
    let line = refcheck::tape_digest_line(&client, &now).await.unwrap().unwrap();
    assert_eq!(line, "Tape caches stored in the last 24 hours: 2 (0 verified by the tape reader, 1 disagree, 0 withheld whole, 1 not verified)");
    assert!(watch::digest(&mut client, &settings, &now).await.unwrap().contains(&line));
    let show = refcheck::tape_show(&client, S).await.unwrap();
    assert_eq!(show["counts"], json!({"FAIL": 1, "stated absence": 1}));
    // the real FAIL lists no bin differences (its reason names the field)
    assert_eq!((show["not_passed"][0]["differences"].clone(), show["not_passed"][0]["why"].as_str()), (json!(0), Some("the reference reader's bins differ from the engine's cache: conditional_last_rejected reader 38, cache 39")));

    // redone again (a fixed reader is a new method version) and passed: the disagreement resolves
    const V3: &str = "engine-v3";
    assert!(tape::put(&client, "AAPL", S, V3, &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    assert!(tape::put_verdict(&client, "AAPL", S, V3, &check(&verdict(V3, "PASS"), V3)).await.unwrap());
    assert_eq!(refcheck::state(&client, "FREE_REPORT_64", None, LATER).await.unwrap().tape_failed, 0);
    let now: String = client.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0);
    let resolved: Vec<String> = watch::check(&mut client, &settings, &now).await.unwrap().resolved.into_iter().filter(|l| l.starts_with("Reference check")).collect();
    assert_eq!(resolved, [said]);
    assert!(problems(&client).await.is_empty());
    assert_eq!(refcheck::tape_show(&client, S).await.unwrap()["counts"], json!({"PASS": 1, "stated absence": 1}));
    scratch.drop().await;
}
