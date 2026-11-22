//! C-059 (owner decision 2026-11-20): the tape caches stored before the verdicts are verified by a
//! sample of their sessions. A verify job's worker downloads a session's replay files again,
//! recomputes each member's cache and runs the tape reader on it; core compares the recomputed
//! cache's digest with the one it holds, keeps the verdict beside the stored cache only when they
//! are equal, and a difference is a problem to look at. The stored cache is the real one
//! (`test_support::TAPE_AAPL`, AAPL's of 2025-12-10). The checks and verdicts are real (D-102): the
//! cluster's verify-job checks of AAPL's cache of 2025-12-01 (`test_support::TAPE_CHECK_AAPL_PASS`,
//! `_FAIL`), bound to this cache (asset, session, method version and digests changed, nothing
//! else). Cases the cluster never met are that real check with only the field under test changed,
//! stated where used: a recomputation that differs (no DIFFER occurred: the cache with one byte
//! changed, and a reason), and one that compared nothing (no NOT_COMPARED occurred: no recomputed
//! digest, and a reason). Run with `cargo test -- --ignored`.
use core_contract::sha256_hex;
use core_db::tape::{self, Cache};
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, refcheck, universe};
use serde_json::{Value, json};
use test_support::slice::inventories;
use test_support::{Scratch, TAPE_AAPL, TAPE_CHECK_AAPL_FAIL, TAPE_CHECK_AAPL_PASS, tape_check_bound, tape_verdict_bound};

const S: &str = "2025-12-10";
const V1: &str = "engine-v1";
const P: &str = "FREE_REPORT_64";
const LATER: &str = "2099-01-01T00:00:00Z";

fn changed() -> String {
    let mut other = TAPE_AAPL.to_vec();
    *other.last_mut().unwrap() ^= 1;
    sha256_hex(&other)
}

/// The real check bound to `asset` and this cache, with the fields under test: the recomputed
/// digest, the reason and the verdict (a real one bound to that digest, or none).
fn check(asset: &str, recomputed: Option<&str>, why: Option<&str>, verdict: Option<Value>) -> Value {
    let mut c = tape_check_bound(TAPE_CHECK_AAPL_PASS, asset, S, V1, &sha256_hex(TAPE_AAPL));
    c["recomputed_sha256"] = json!(recomputed);
    c["why"] = json!(why);
    c["verdict"] = verdict.unwrap_or(Value::Null);
    c
}

/// The real PASS verdict, bound to the cache digest `cache`.
fn pass(cache: &str) -> Value {
    tape_verdict_bound(TAPE_CHECK_AAPL_PASS, "AAPL", S, V1, cache)
}

/// The real FAIL verdict (the reader of build 3b85b4f6), bound to the cache digest `cache`.
fn fail(cache: &str) -> Value {
    tape_verdict_bound(TAPE_CHECK_AAPL_FAIL, "AAPL", S, V1, cache)
}

/// The builds whose readers judged the frozen checks, registered and admitted at their recorded
/// times on the cluster (`core.engine_build`, read 2026-11-21): 3b85b4f6 (the reader of the FAIL,
/// 05:20:17.79Z) before cdf58be2 (the corrected reader of the PASS, 06:27:50.78Z). Their slot
/// inventory is the slice's (the order is what the verdicts need).
async fn reader_builds(client: &mut tokio_postgres::Client) {
    for (build, at) in [("3b85b4f6+96356aa79bd5", "2026-11-20 05:20:17.790723+00"), ("cdf58be2+96356aa79bd5", "2026-11-20 06:27:50.776257+00")] {
        let mut inventory = inventories()[0].clone();
        inventory["engine_build"] = json!(build);
        build::register(client, &inventory).await.unwrap();
        build::admit(client, build, &json!({"checklist": "tape reader order test"})).await.unwrap();
        client.execute("UPDATE core.engine_build SET registered_at = $2::text::timestamptz, admitted_at = $2::text::timestamptz WHERE engine_build = $1", &[&build, &at]).await.unwrap();
    }
}

fn parsed(v: &Value, asset: &str) -> tape::Check {
    tape::check_check(&serde_json::to_vec(v).unwrap(), asset, S, V1).unwrap()
}

#[test]
fn a_check_is_checked_before_anything_is_stored() {
    let whole = sha256_hex(TAPE_AAPL);
    let ok = parsed(&check("AAPL", Some(&whole), None, Some(pass(&whole))), "AAPL");
    assert_eq!((ok.recomputed_sha256.as_deref(), ok.tape_bytes, ok.verdict.as_ref().map(|v| v.outcome.as_str())), (Some(whole.as_str()), Some(19_150_689_784), Some("PASS")));
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
    let settings = Settings { profile: P.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None, backup_days: None };

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
    // no DIFFER occurred on the cluster: the real check with the field under test changed (the
    // recomputed digest is the cache's with one byte changed) and a reason
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
    // round cc, 7: three members checked of the 64 listed: the rest are counted not checked
    let sample = tape::sample(&client).await.unwrap();
    assert_eq!((sample[0]["listed"].as_i64(), sample[0]["checked"].as_i64(), sample[0]["unchecked"].as_i64(), &sample[0]["passed"]), (Some(64), Some(3), Some(61), &json!(false)));
    assert!(tape::sample_line(&sample).unwrap().ends_with("(0 of 64 asset-sessions recomputed byte for byte with the tape reader agreeing; 1 differ from the stored cache, 2 not compared, 61 not checked)"));
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
    assert_eq!((sample[0]["tape_bytes"].as_i64(), sample[0]["not_passed"][0]["asset"].as_str()), (Some(19_150_689_784), Some("AAPL")));

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
    work::cancel(&mut client, &[jobs[0]]).await.unwrap();

    // round cc: one verify job a step, AAPL's check alone, the job then cancelled
    async fn verify_aapl(client: &mut tokio_postgres::Client, b: &str, method: &str, v: Value) -> tape::Checked {
        let jobs = work::schedule_verify(client, P, b, &[S]).await.unwrap();
        let claim = work::claim(client, Kind::Tape, b, "verify-cc", 3600.0).await.unwrap().unwrap();
        let checked = tape::put_check(client, &claim.token, S, "AAPL", method, &tape::check_check(&serde_json::to_vec(&v).unwrap(), "AAPL", S, method).unwrap()).await.unwrap();
        work::cancel(client, &jobs).await.unwrap();
        checked
    }
    reader_builds(&mut client).await;
    let disagree = |s: &refcheck::State| (s.sample_differ, s.tape_failed);
    // 8: a DIFFER, then a check that compared nothing (the provider refused the session): the
    // difference stands until a later comparison matches
    assert_eq!(verify_aapl(&mut client, &b, V1, check("AAPL", Some(&differ), Some(said), None)).await.outcome, "DIFFER");
    assert_eq!(disagree(&refcheck::state(&client, P, None, LATER).await.unwrap()), (1, 0));
    assert_eq!(verify_aapl(&mut client, &b, V1, check("AAPL", None, Some("its tapes were not captured: HTTP 500"), None)).await.outcome, "NOT_COMPARED");
    assert_eq!(disagree(&refcheck::state(&client, P, None, LATER).await.unwrap()), (1, 0), "a check that compared nothing resolves nothing");
    // 6 (C-058 amended 2026-11-21): recomputed byte for byte and judged FAIL beside the PASS kept
    // earlier: the verdict there stands, and the disagreement is the latest reader's to say. The
    // real FAIL is the earlier reader's (3b85b4f6), sent after the corrected reader's PASS: it
    // supersedes nothing
    let c = verify_aapl(&mut client, &b, V1, check("AAPL", Some(&whole), None, Some(fail(&whole)))).await;
    assert_eq!((c.outcome.as_str(), c.verdict_kept), ("MATCH", false));
    assert_eq!(disagree(&refcheck::state(&client, P, None, LATER).await.unwrap()), (0, 0), "an earlier reader's FAIL, whenever it comes");
    // a later reader FAILS the same bytes (no such verdict exists: the real FAIL with only its
    // reader's build changed, to one admitted after cdf58be2's): raised
    let mut later = inventories()[0].clone();
    later["engine_build"] = json!("e9c216f7+aabffe6e62a2");
    build::register(&mut client, &later).await.unwrap();
    build::admit(&client, "e9c216f7+aabffe6e62a2", &json!({"checklist": "tape reader order test"})).await.unwrap();
    let mut by_later = fail(&whole);
    by_later["engine_build"] = json!("e9c216f7+aabffe6e62a2");
    let c = verify_aapl(&mut client, &b, V1, check("AAPL", Some(&whole), None, Some(by_later))).await;
    assert_eq!((c.outcome.as_str(), c.verdict_kept), ("MATCH", false));
    let state = refcheck::state(&client, P, None, LATER).await.unwrap();
    assert_eq!(disagree(&state), (0, 1));
    assert_eq!(state.disagree_summary().unwrap(), "Reference check: the tape reader disagrees with the engine's tape cache on 1 asset-session, the oldest 2025-12-10");
    // 10: the sample counts that FAIL as a MATCH the reader did not agree with, and the DIFFER's
    // verdict-less check under no verdict count
    let line = tape::sample_line(&tape::sample(&client).await.unwrap()).unwrap();
    assert!(line.ends_with("(0 of 64 asset-sessions recomputed byte for byte with the tape reader agreeing; 1 recomputed equal, the tape reader not agreeing, 63 not compared)"), "{line}");
    assert_eq!(refcheck::tape_show(&client, S).await.unwrap()["counts"], json!({"FAIL": 1, "stated absence": 1}), "show says the current verdict");
    // 8: a newer method version's cache supersedes the one checked: neither its FAIL nor a later
    // DIFFER of the superseded cache is raised
    assert!(tape::put(&client, "AAPL", S, "engine-v2", &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    assert_eq!(disagree(&refcheck::state(&client, P, None, LATER).await.unwrap()), (0, 0));
    assert_eq!(verify_aapl(&mut client, &b, V1, check("AAPL", Some(&differ), Some(said), None)).await.outcome, "DIFFER");
    assert_eq!(disagree(&refcheck::state(&client, P, None, LATER).await.unwrap()), (0, 0), "a check of a superseded cache");
    scratch.drop().await;
}

/// C-058 amended 2026-11-21, on the cluster's sequence of 2026-11-20 for AAPL's cache of
/// 2025-12-01 (bound to this cache): the reader of 3b85b4f6 FAILED it (kept beside the cache), the
/// corrected reader of cdf58be2 then PASSED the same recomputed bytes (not kept: the first stands).
/// The disagreement opens on the first and resolves on the second; for one reader, the verdict
/// stored last counts.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_old_readers_fail_is_superseded_by_a_fixed_readers_pass_on_the_same_bytes() {
    let scratch = Scratch::new("tape_reader_order").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "tape reader order test"})).await.unwrap();
    reader_builds(&mut client).await;
    let settings = Settings { profile: P.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None, backup_days: None };
    assert!(tape::put(&client, "AAPL", S, V1, &Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    let whole = sha256_hex(TAPE_AAPL);
    let send = async |client: &mut tokio_postgres::Client, real: &str, change: &dyn Fn(&mut Value)| {
        let jobs = work::schedule_verify(client, P, &b, &[S]).await.unwrap();
        let claim = work::claim(client, Kind::Tape, &b, "verify", 3600.0).await.unwrap().unwrap();
        let mut c = test_support::tape_check_bound(real, "AAPL", S, V1, &whole);
        change(&mut c);
        let checked = tape::put_check(client, &claim.token, S, "AAPL", V1, &parsed(&c, "AAPL")).await.unwrap();
        work::cancel(client, &jobs).await.unwrap();
        checked
    };
    let opened = "Reference check: the tape reader disagrees with the engine's tape cache on 1 asset-session, the oldest 2025-12-10";
    let ours = |v: Vec<String>| v.into_iter().filter(|l| l.starts_with("Reference check")).collect::<Vec<_>>();
    let minute = async |c: &tokio_postgres::Client| -> String { c.query_one("SELECT (now() + interval '1 minute')::text", &[]).await.unwrap().get(0) };

    // the old reader's FAIL, kept beside the cache: a disagreement
    let c = send(&mut client, TAPE_CHECK_AAPL_FAIL, &|_| ()).await;
    assert_eq!((c.outcome.as_str(), c.verdict_kept), ("MATCH", true));
    let now = minute(&client).await;
    assert_eq!(ours(watch::check(&mut client, &settings, &now).await.unwrap().opened), [opened]);
    // the corrected reader's PASS on the same bytes: the kept FAIL stands as evidence, the
    // disagreement resolves, and `refcheck show` says PASS
    let c = send(&mut client, TAPE_CHECK_AAPL_PASS, &|_| ()).await;
    assert_eq!((c.outcome.as_str(), c.verdict_kept, c.why.as_deref()), ("MATCH", false, Some("a FAIL verdict already stored beside the cache stands")));
    let state = refcheck::state(&client, P, None, LATER).await.unwrap();
    assert_eq!((state.tape_failed, state.disagree_summary()), (0, None));
    let now = minute(&client).await;
    assert_eq!(ours(watch::check(&mut client, &settings, &now).await.unwrap().resolved), [opened]);
    assert_eq!(refcheck::tape_show(&client, S).await.unwrap()["counts"], json!({"PASS": 1}));
    let kept: String = client.query_one("SELECT outcome FROM core.tape_verdict WHERE session = $1::text::date AND asset_key = 'AAPL'", &[&S]).await.unwrap().get(0);
    assert_eq!(kept, "FAIL", "the kept verdict is unchanged");
    // the day workers read the current verdict: the fixed reader's PASS, as it sent it
    let served = tape::verdicts_of_session(&client, S, V1).await.unwrap();
    assert_eq!(served, vec![("AAPL".to_owned(), test_support::tape_verdict_bound(TAPE_CHECK_AAPL_PASS, "AAPL", S, V1, &whole))]);
    // the same reader again, later (no such verdict exists: the real PASS with only its outcome and
    // reason changed): the verdict stored last counts
    let c = send(&mut client, TAPE_CHECK_AAPL_PASS, &|c| {
        c["verdict"]["outcome"] = json!("FAIL");
        c["verdict"]["why"] = json!("the reference reader's bins differ from the engine's cache: conditional_last_rejected reader 38, cache 39");
    })
    .await;
    assert_eq!((c.outcome.as_str(), c.verdict_kept), ("MATCH", false));
    assert_eq!(refcheck::state(&client, P, None, LATER).await.unwrap().tape_failed, 1);
    scratch.drop().await;
}
