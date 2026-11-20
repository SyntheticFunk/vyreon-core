//! C-059 (owner decision 2026-11-20): the tape caches stored before the verdicts are verified by a
//! sample of their sessions. A verify job's worker downloads a session's replay files again,
//! recomputes each member's cache and runs the tape reader on it; core compares the recomputed
//! cache's digest with the one it holds, keeps the verdict beside the stored cache only when they
//! are equal, and a difference is a problem to look at. The stored cache is the real one
//! (`test_support::TAPE_AAPL`, AAPL's of 2025-12-10). The recomputed digests stand for what a verify
//! worker sends: the real cache's own (recomputed equal) and that cache with one byte changed (a
//! recomputation that differs); the PASS verdict stands for the tape reader's on a raw tape (none is
//! on this machine). Run with `cargo test -- --ignored`.
use core_contract::sha256_hex;
use core_db::tape::{self, Cache};
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, refcheck, universe};
use serde_json::{Value, json};
use test_support::slice::inventories;
use test_support::{Scratch, TAPE_AAPL};

const S: &str = "2025-12-10";
const V1: &str = "engine-v1";
const P: &str = "FREE_REPORT_64";
const LATER: &str = "2099-01-01T00:00:00Z";

fn changed() -> String {
    let mut other = TAPE_AAPL.to_vec();
    *other.last_mut().unwrap() ^= 1;
    sha256_hex(&other)
}

fn check(asset: &str, recomputed: Option<&str>, why: Option<&str>, verdict: Option<Value>) -> Value {
    json!({"schema": tape::CHECK_SCHEMA, "asset": asset, "session": S, "method_version": V1, "engine_build": "build-x",
           "recomputed_sha256": recomputed, "why": why, "tape_bytes": 30_056_637_434_i64, "stamped": [], "differences": [], "verdict": verdict})
}

fn pass(cache: &str) -> Value {
    json!({"schema": tape::VERDICT_SCHEMA, "asset": "AAPL", "session": S, "outcome": "PASS", "why": null, "differences": [],
           "reader_bins_sha256": "a".repeat(64), "method_version": V1, "cache_sha256": cache, "engine_build": "build-x", "versions": null})
}

fn parsed(v: &Value, asset: &str) -> tape::Check {
    tape::check_check(&serde_json::to_vec(v).unwrap(), asset, S, V1).unwrap()
}

#[test]
fn a_check_is_checked_before_anything_is_stored() {
    let whole = sha256_hex(TAPE_AAPL);
    let ok = parsed(&check("AAPL", Some(&whole), None, Some(pass(&whole))), "AAPL");
    assert_eq!((ok.recomputed_sha256.as_deref(), ok.tape_bytes, ok.verdict.as_ref().map(|v| v.outcome.as_str())), (Some(whole.as_str()), Some(30_056_637_434), Some("PASS")));
    assert!(parsed(&check("MSFT", None, Some("its tapes were not captured: HTTP 500"), None), "MSFT").recomputed_sha256.is_none());
    let refused = |v: Value, says: &str| {
        let e = tape::check_check(&serde_json::to_vec(&v).unwrap(), "AAPL", S, V1).unwrap_err();
        assert!(e.contains(says), "{e} should say {says}");
    };
    let with = |k: &str, x: Value| {
        let mut v = check("AAPL", Some(&whole), None, None);
        v[k] = x;
        v
    };
    refused(with("schema", json!("VYREON-TAPE-CHECK@0")), "schema");
    refused(with("asset", json!("MSFT")), "asset");
    refused(with("method_version", json!("engine-v2")), "method_version");
    refused(with("recomputed_sha256", json!("abc")), "a sha256 or null");
    refused(with("recomputed_sha256", Value::Null), "says why");
    refused(with("tape_bytes", json!(-1)), "count of bytes");
    refused(with("verdict", json!("PASS")), "an object or null");
    refused(check("AAPL", Some(&whole), None, Some(pass(&changed()))), "judged another cache");
    let mut bad = pass(&whole);
    bad["outcome"] = json!("MAYBE");
    refused(check("AAPL", Some(&whole), None, Some(bad)), "outcome must be one of");
    refused(check("AAPL", None, Some("the capture failed: api_key=abcdef0123456789"), None), "unredacted");
    assert!(tape::check_check(&vec![b' '; tape::MAX_CHECK_BYTES + 1], "AAPL", S, V1).unwrap_err().contains("at most"));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_sample_check_keeps_the_verdict_only_beside_an_equal_cache_and_a_difference_is_a_problem() {
    let scratch = Scratch::new("tape_check").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let members: Vec<String> = doc["members"].as_array().unwrap().iter().map(|m| m["asset_key"].as_str().unwrap().to_owned()).collect();
    assert_eq!(members.len(), 64);
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "tape check test"})).await.unwrap();
    let settings = Settings { profile: P.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None };

    // nothing to verify before a cache is stored; a session that is not one is refused
    let e = work::schedule_verify(&mut client, P, &b, &[S]).await.unwrap_err();
    assert_eq!(e, "core holds no tape cache of 2025-12-10: nothing to verify");
    assert!(tape::put(&client, "AAPL", S, V1, &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    tape::put(&client, "SPY", S, V1, &Cache::Absent("TAPE_PROVIDER_REFUSED: only AAPL's tape is on this machine".into())).await.unwrap();
    assert!(work::schedule_verify(&mut client, P, &b, &["2025-12-13"]).await.unwrap_err().contains("not a trading session"));
    assert!(work::schedule_verify(&mut client, P, &b, &[]).await.is_err());
    // not while the session's own tape job is unfinished
    work::schedule(&mut client, Kind::Tape, P, &b, S, S, 1).await.unwrap();
    let e = work::schedule_verify(&mut client, P, &b, &[S]).await.unwrap_err();
    assert!(e.starts_with("2025-12-10 has unfinished tape job"), "{e}");
    let tape_job: i64 = client.query_one("SELECT job_id FROM core.job WHERE kind = 'TAPE'", &[]).await.unwrap().get(0);
    work::cancel(&mut client, &[tape_job]).await.unwrap();
    let jobs = work::schedule_verify(&mut client, P, &b, &[S, S]).await.unwrap();
    assert_eq!(jobs.len(), 1, "a session named twice is one job");

    // a verify job holds no day back, and the daily runs do not take it for the session's tape job
    work::schedule(&mut client, Kind::DayBlock, P, &b, S, "2025-12-11", 2).await.unwrap();
    let claimable: i64 = client.query_one("SELECT count(*) FROM core.claimable_job WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap().get(0);
    assert_eq!(claimable, 1);

    // the first sample: AAPL recomputed other bytes (DIFFER), SPY is a stated absence and the others
    // were not captured (NOT_COMPARED); the session is done only with every listed member checked
    let claim = work::claim(&mut client, Kind::Tape, &b, "verify-1", 3600.0).await.unwrap().unwrap();
    assert_eq!((claim.job_id, claim.scope.as_deref()), (jobs[0], Some(r#"{"verify": true}"#)));
    let refused = tape::session_done(&mut client, &claim.token, S, Some(V1)).await.unwrap_err();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.starts_with("64 members listed on 2025-12-10 have no check from verify job")), "{refused}");
    let differ = changed();
    let said = "manifest.json differs: tapes[2].stream_sha256";
    let c = tape::put_check(&mut client, &claim.token, S, "AAPL", V1, &parsed(&check("AAPL", Some(&differ), Some(said), Some(pass(&differ))), "AAPL")).await.unwrap();
    assert_eq!((c.outcome.as_str(), c.verdict_kept, c.why.as_deref(), c.new), ("DIFFER", false, Some(said), true));
    let again = tape::put_check(&mut client, &claim.token, S, "AAPL", V1, &parsed(&check("AAPL", Some(&sha256_hex(TAPE_AAPL)), None, None), "AAPL")).await.unwrap();
    assert_eq!((again.outcome.as_str(), again.new), ("DIFFER", false), "the first check stands");
    let spy = tape::put_check(&mut client, &claim.token, S, "SPY", V1, &parsed(&check("SPY", Some(&sha256_hex(TAPE_AAPL)), None, None), "SPY")).await.unwrap();
    assert_eq!((spy.outcome.as_str(), spy.why.as_deref()), ("NOT_COMPARED", Some("core holds a stated absence: TAPE_PROVIDER_REFUSED: only AAPL's tape is on this machine")));
    let msft = tape::put_check(&mut client, &claim.token, S, "MSFT", V1, &parsed(&check("MSFT", Some(&sha256_hex(TAPE_AAPL)), None, None), "MSFT")).await.unwrap();
    assert_eq!(msft.why.as_deref(), Some("core holds no tape cache of MSFT 2025-12-10 under engine-v1"));
    let e = tape::put_check(&mut client, &claim.token, S, "ZZZZ", V1, &parsed(&check("ZZZZ", None, Some("x"), None), "ZZZZ")).await.unwrap_err();
    assert!(matches!(&e, LeaseError::Refused(m) if m.contains("is not listed on 2025-12-10")), "{e}");
    for m in members.iter().filter(|m| !["AAPL", "SPY", "MSFT"].contains(&m.as_str())) {
        tape::put_check(&mut client, &claim.token, S, m, V1, &parsed(&check(m, None, Some("only AAPL's tape is on this machine"), None), m)).await.unwrap();
    }
    tape::session_done(&mut client, &claim.token, S, None).await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();
    assert!(tape::verdicts_of_session(&client, S, V1).await.unwrap().is_empty(), "no verdict beside a cache recomputed otherwise");
    assert_eq!(tape::of_session(&client, S, V1).await.unwrap().iter().find(|(k, _)| k == "AAPL").unwrap().1, Cache::Body(TAPE_AAPL.to_vec()), "the stored cache is untouched");

    // the difference is a problem to look at, and the digest states the sample
    let state = refcheck::state(&client, P, None, LATER).await.unwrap();
    assert_eq!((state.sample_differ, state.sample_differ_oldest.as_deref()), (1, Some(S)));
    let problem = "Reference check: the tape sample recomputed a cache other than the stored one on 1 asset-session, the oldest 2025-12-10 (vyreon-core tape checks)";
    let now: String = client.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0);
    let opened: Vec<String> = watch::check(&mut client, &settings, &now).await.unwrap().opened.into_iter().filter(|l| l.starts_with("Reference check")).collect();
    assert_eq!(opened, [problem]);
    let line = "Tape cache code verified by sample (C-059): 0 of 1 sessions passed (0 of 64 asset-sessions recomputed byte for byte with the tape reader agreeing; 1 differ from the stored cache, 63 not compared)";
    assert_eq!(tape::sample_line(&tape::sample(&client).await.unwrap()).as_deref(), Some(line));
    assert!(watch::digest(&client, &settings, &now).await.unwrap().contains(line));
    let sample = tape::sample(&client).await.unwrap();
    assert_eq!((sample[0]["tape_bytes"].as_i64(), sample[0]["not_passed"][0]["asset"].as_str()), (Some(30_056_637_434), Some("AAPL")));

    // the second sample recomputes AAPL byte for byte and the reader agrees: the verdict is kept
    // beside the stored cache, and the latest check no longer differs
    let jobs = work::schedule_verify(&mut client, P, &b, &[S]).await.unwrap();
    let claim = work::claim(&mut client, Kind::Tape, &b, "verify-2", 3600.0).await.unwrap().unwrap();
    assert_eq!(claim.job_id, jobs[0]);
    let whole = sha256_hex(TAPE_AAPL);
    let c = tape::put_check(&mut client, &claim.token, S, "AAPL", V1, &parsed(&check("AAPL", Some(&whole), None, Some(pass(&whole))), "AAPL")).await.unwrap();
    assert_eq!((c.outcome.as_str(), c.verdict_kept, c.why.as_deref()), ("MATCH", true, None));
    assert_eq!(tape::verdicts_of_session(&client, S, V1).await.unwrap(), vec![("AAPL".to_owned(), pass(&whole))]);
    for m in members.iter().filter(|m| *m != "AAPL") {
        tape::put_check(&mut client, &claim.token, S, m, V1, &parsed(&check(m, None, Some("only AAPL's tape is on this machine"), None), m)).await.unwrap();
    }
    tape::session_done(&mut client, &claim.token, S, None).await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();
    assert_eq!(refcheck::state(&client, P, None, LATER).await.unwrap().sample_differ, 0);
    let now: String = client.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0);
    let resolved: Vec<String> = watch::check(&mut client, &settings, &now).await.unwrap().resolved.into_iter().filter(|l| l.starts_with("Reference check")).collect();
    assert_eq!(resolved, [problem]);
    assert_eq!(
        tape::sample_line(&tape::sample(&client).await.unwrap()).as_deref(),
        Some("Tape cache code verified by sample (C-059): 0 of 1 sessions passed (1 of 64 asset-sessions recomputed byte for byte with the tape reader agreeing; 63 not compared)")
    );

    // a third finds the verdict already beside the cache: it stands; a verify job that fails for
    // the last time sets nothing aside and pauses nothing
    let jobs = work::schedule_verify(&mut client, P, &b, &[S]).await.unwrap();
    let claim = work::claim(&mut client, Kind::Tape, &b, "verify-3", 3600.0).await.unwrap().unwrap();
    let c = tape::put_check(&mut client, &claim.token, S, "AAPL", V1, &parsed(&check("AAPL", Some(&whole), None, Some(pass(&whole))), "AAPL")).await.unwrap();
    assert_eq!((c.outcome.as_str(), c.verdict_kept, c.why.as_deref()), ("MATCH", false, Some("a PASS verdict already stored beside the cache stands")));
    work::fail(&mut client, &claim.token, "tape 2025-12-10", "the replay listing failed", "").await.unwrap();
    for n in 2..=3 {
        let claim = work::claim(&mut client, Kind::Tape, &b, &format!("verify-3-{n}"), 3600.0).await.unwrap().unwrap();
        assert_eq!(claim.job_id, jobs[0]);
        work::fail(&mut client, &claim.token, "tape 2025-12-10", "the replay listing failed", "").await.unwrap();
    }
    let (state, paused): (String, bool) = client
        .query_one("SELECT state, (SELECT paused FROM core.queue_control) FROM core.job WHERE job_id = $1", &[&jobs[0]])
        .await
        .map(|r| (r.get(0), r.get(1)))
        .unwrap();
    assert_eq!((state.as_str(), paused), ("FAILED", false));
    scratch.drop().await;
}
