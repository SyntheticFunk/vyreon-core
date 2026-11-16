//! Failure evidence (C-049) with a real worker's log of a refused day
//! (`test_support::WORKER_LOG_REFUSED_DAY`) as the excerpt. Run the database test with
//! `cargo test -- --ignored`.
use core_db::evidence::{self, SCHEMA};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Write;
use test_support::slice::inventories;
use test_support::{Scratch, WORKER_LOG_REFUSED_DAY};

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// A document as the worker sends it: the account's line, then the excerpt.
fn document(account: &Value, excerpt: &str) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(format!("{account}\n{excerpt}").as_bytes()).unwrap();
    gz.finish().unwrap()
}

/// The real refused day's account: its stop and the plan's refusing entry as the engine writes it.
fn account(session: &str) -> Value {
    json!({
        "schema": SCHEMA, "stage": format!("day {session}"),
        "message": format!("stop: {session} not published: {session} refused by the input plan (plan.json)"),
        "session": session,
        "refusals": [{"input": "close_quotes", "session": session, "role": "T", "status": "missing", "blocking": true,
                      "reason": "14016 published option attempts without a quote file"}],
    })
}

#[test]
fn a_document_is_checked_and_a_credential_left_in_it_is_refused() {
    let checked = evidence::check(document(&account("2024-07-16"), WORKER_LOG_REFUSED_DAY)).unwrap();
    assert_eq!((checked.stage.as_str(), checked.session.as_deref(), checked.excerpt_bytes), ("day 2024-07-16", Some("2024-07-16"), WORKER_LOG_REFUSED_DAY.len() as u64));
    assert_eq!(checked.refusals.as_ref().and_then(Value::as_array).map(Vec::len), Some(1));
    // the real log, with a provider URL carrying a key and a connection string with a password
    // put into it: refused, whichever it is
    let leaked = |line: &str| WORKER_LOG_REFUSED_DAY.replacen("   exit Some(1)\n", &format!("   exit Some(1)\n{line}\n"), 1);
    for line in [
        "GET https://api-v2.intrinio.com/options/prices/eod?api_key=OmRlYWRiZWVm&page_size=250 failed: HTTP 502",
        "cannot connect: postgresql://vyreon_ro:hunter2pass@vyreon-pg-rw:5432/vyreon",
    ] {
        let refused = evidence::check(document(&account("2024-07-16"), &leaked(line))).unwrap_err();
        assert!(refused.contains("nothing was stored"), "{refused}");
    }
    let redacted = leaked("GET https://api-v2.intrinio.com/options/prices/eod?api_key=[redacted]&page_size=250 failed: HTTP 502");
    assert!(evidence::check(document(&account("2024-07-16"), &redacted)).is_ok());
    // what is not an account
    for bad in [json!({"stage": "day", "message": "x"}), json!({"schema": SCHEMA, "stage": "", "message": "x"}), json!({"schema": SCHEMA, "stage": "day", "message": "x", "session": "yesterday"})] {
        assert!(evidence::check(document(&bad, "")).is_err(), "{bad}");
    }
    let whole = document(&account("2024-07-16"), WORKER_LOG_REFUSED_DAY);
    assert!(evidence::check(whole[..whole.len() - 4].to_vec()).is_err(), "the gzip is read to its end");
    assert!(evidence::check(Vec::new()).is_err());
    assert!(evidence::check(vec![0u8; evidence::MAX_DOCUMENT_BYTES + 1]).unwrap_err().contains("at most"));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn each_attempt_leaves_its_evidence_once_and_it_is_read_back_whole() {
    let scratch = Scratch::new("evidence").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let inventory = inventories().into_iter().next().unwrap();
    let base = build::register(&mut client, &inventory).await.unwrap().engine_build;
    build::admit(&client, &base, &json!({"checklist": "evidence test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base, "2024-07-15", "2024-07-17", 30).await.unwrap();
    let first = work::claim(&mut client, Kind::DayBlock, &base, "w1", HOUR).await.unwrap().unwrap();
    let real = || evidence::check(document(&account("2024-07-16"), WORKER_LOG_REFUSED_DAY)).unwrap();

    // a session outside the job's, and a token that names no lease
    let outside = evidence::check(document(&account("2024-07-18"), "")).unwrap();
    assert!(matches!(evidence::put(&mut client, &first.token, &outside).await, Err(LeaseError::Refused(ref m)) if m.contains("not a session of job")));
    assert_eq!(evidence::put(&mut client, "not-a-lease", &real()).await, Err(LeaseError::Gone));

    // sent before the worker's fail: kept; sent again (its answer lost): the first stays
    assert!(evidence::put(&mut client, &first.token, &real()).await.unwrap());
    assert!(!evidence::put(&mut client, &first.token, &evidence::check(document(&account("2024-07-15"), "other")).unwrap()).await.unwrap());
    assert!(work::fail(&mut client, &first.token, "day 2024-07-16", "refused by the input plan", "").await.unwrap());

    // the next attempt's worker lost its lease and stopped: its ended lease still leaves evidence
    let second = work::claim(&mut client, Kind::DayBlock, &base, "w2", HOUR).await.unwrap().unwrap();
    work::cancel(&mut client, &[second.job_id]).await.unwrap();
    let mut stopped = account("2024-07-16");
    stopped["stage"] = json!("day 2024-07-16 (lease lost)");
    stopped["refusals"] = Value::Null;
    assert!(evidence::put(&mut client, &second.token, &evidence::check(document(&stopped, "")).unwrap()).await.unwrap());

    // listed newest first; shown whole, the latest attempt by default
    let listed = evidence::list(&client, Some(first.job_id), 10).await.unwrap();
    let seen: Vec<(i64, &str, Option<i64>, i64)> = listed.iter().map(|r| (r["attempt"].as_i64().unwrap(), r["worker"].as_str().unwrap(), r["refusals"].as_i64(), r["excerpt_bytes"].as_i64().unwrap())).collect();
    assert_eq!(seen, [(2, "w2", None, 0), (1, "w1", Some(1), WORKER_LOG_REFUSED_DAY.len() as i64)]);
    assert_eq!(evidence::list(&client, None, 10).await.unwrap().len(), 2);
    let (latest, excerpt) = evidence::show(&client, first.job_id, None).await.unwrap().unwrap();
    assert_eq!((latest["attempt"].as_i64(), latest["stage"].as_str(), excerpt.as_str()), (Some(2), Some("day 2024-07-16 (lease lost)"), ""));
    let (one, excerpt) = evidence::show(&client, first.job_id, Some(1)).await.unwrap().unwrap();
    assert_eq!(excerpt, WORKER_LOG_REFUSED_DAY);
    assert_eq!((one["kind"].as_str(), one["session"].as_str(), one["first_session"].as_str(), one["last_session"].as_str()), (Some("DAY_BLOCK"), Some("2024-07-16"), Some("2024-07-15"), Some("2024-07-17")));
    assert_eq!(one["refusals"], account("2024-07-16")["refusals"]);
    assert_eq!(one["engine_build"].as_str(), Some(base.as_str()));
    assert!(evidence::show(&client, first.job_id, Some(3)).await.unwrap().is_none());
    // the stored document is checked against its digest when it is read
    client.execute("UPDATE core.job_evidence SET document = document || '\\x00'::bytea WHERE attempt = 1", &[]).await.unwrap();
    assert!(evidence::show(&client, first.job_id, Some(1)).await.is_err());
    drop(client);
    scratch.drop().await;
}
