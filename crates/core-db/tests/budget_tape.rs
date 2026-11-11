//! The request budget, and tape caches with a real one (`fixtures/tape-cache-AAPL-2025-12-10.tar.gz`:
//! AAPL's reduced tape of 2025-12-10 from the droplet, packed as its four files). Run with
//! `cargo test -- --ignored`.
use test_support::Scratch;
use test_support::slice::inventories;
use core_db::budget::{self, Budget, Grant};
use core_db::tape::{self, Cache};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use serde_json::{Value, json};
use tokio_postgres::Client;

const TAPE: &[u8] = test_support::TAPE_AAPL;

async fn migrated(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    (scratch, client)
}

fn granted(grant: Grant) -> (i64, i32) {
    match grant {
        Grant::Granted { budget_lease_id, requests, .. } => (budget_lease_id, requests),
        Grant::Wait { seconds } => panic!("told to wait {seconds} s"),
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_budget_never_commits_more_than_its_ceiling() {
    let (scratch, mut client) = migrated("budget").await;
    let budget = Budget::default();
    assert_eq!((budget.per_minute, budget.max_lease), (1800, 600));
    // three workers starting cold take all of it; the fourth waits for the first lease to age out
    let (first, n) = granted(budget::draw(&mut client, budget, "w1", 5000).await.unwrap());
    assert_eq!(n, 600, "no lease is larger than the maximum");
    let (second, _) = granted(budget::draw(&mut client, budget, "w2", 600).await.unwrap());
    granted(budget::draw(&mut client, budget, "w3", 600).await.unwrap());
    assert_eq!(budget::committed(&client).await.unwrap(), 1800);
    match budget::draw(&mut client, budget, "w4", 100).await.unwrap() {
        Grant::Wait { seconds } => assert!((80.0..=90.0).contains(&seconds), "a 30 s lease counts for 90 s: {seconds}"),
        other => panic!("{other:?}"),
    }
    // a worker that used less gives the rest back at once
    assert_eq!(budget::release(&mut client, first, "w1", &json!({"daily": 64, "options": 86})).await.unwrap(), 150);
    assert_eq!(budget::committed(&client).await.unwrap(), 1350);
    assert_eq!(granted(budget::draw(&mut client, budget, "w4", 600).await.unwrap()).1, 450, "what is free, when less than asked");
    // a lease is released once, by its own worker
    assert!(budget::release(&mut client, first, "w1", &json!({"daily": 1})).await.is_err());
    assert!(budget::release(&mut client, second, "w1", &json!({"daily": 1})).await.is_err());
    // a worker that reports more than its lease is counted in full and reported
    let over = budget::release(&mut client, second, "w2", &json!({"options": 700})).await.unwrap_err();
    assert!(over.contains("700 requests on a lease of 600"), "{over}");
    assert_eq!(budget::committed(&client).await.unwrap(), 1800);
    // a lease stops counting a minute after its end
    client.execute("UPDATE core.budget_lease SET released_at = released_at - interval '61 seconds', granted_at = granted_at - interval '2 minutes', valid_until = valid_until - interval '2 minutes' WHERE budget_lease_id = $1", &[&first]).await.unwrap();
    assert_eq!(budget::committed(&client).await.unwrap(), 1650);
    // what was used is kept by worker and category
    let usage: Vec<(String, String, i64)> = client
        .query("SELECT worker, category, requests FROM core.request_usage ORDER BY 1, 2", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let row = |w: &str, c: &str, n: i64| (w.to_owned(), c.to_owned(), n);
    assert_eq!(usage, [row("w1", "daily", 64), row("w1", "options", 86), row("w2", "options", 700)]);
    assert!(budget::draw(&mut client, budget, "w5", 0).await.is_err());
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_tape_cache_is_written_once_and_a_tape_job_needs_every_listed_member() {
    let (scratch, mut client) = migrated("tape").await;
    let fixtures = test_support::fixtures();
    let calendar: Value = serde_json::from_str(test_support::CALENDAR).unwrap();
    work::load_calendar(&mut client, &calendar).await.unwrap();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "tape test"})).await.unwrap();
    const METHOD: &str = "engine-v1";

    // the real cache is stored as sent, once
    let cache = Cache::Body(TAPE.to_vec());
    assert!(tape::put(&client, "AAPL", "2025-12-10", METHOD, &cache).await.unwrap());
    assert!(!tape::put(&client, "AAPL", "2025-12-10", METHOD, &cache).await.unwrap(), "the same bytes again change nothing");
    let mut other = TAPE.to_vec();
    *other.last_mut().unwrap() ^= 1;
    let refused = tape::put(&client, "AAPL", "2025-12-10", METHOD, &Cache::Body(other)).await.unwrap_err();
    assert!(refused.contains("already stored with other content"), "{refused}");
    assert!(tape::put(&client, "AAPL", "2025-12-13", METHOD, &cache).await.is_err(), "a Saturday is not a session");
    assert!(tape::put(&client, "AAPL", "2025-12-11", METHOD, &Cache::Body(Vec::new())).await.is_err());

    // a tape job is done only when every member listed on the session has a cache or a stated absence
    work::schedule(&mut client, Kind::Tape, "FREE_REPORT_64", &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let claim = work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
    let refused = tape::session_done(&mut client, &claim.token, "2025-12-10").await.unwrap_err();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.starts_with("63 of 64 members have no tape cache for 2025-12-10: SPY QQQ ")), "{refused}");
    for (key, _) in doc["members"].as_array().unwrap().iter().map(|m| (m["asset_key"].as_str().unwrap(), ())).filter(|(k, _)| *k != "AAPL") {
        tape::put(&client, key, "2025-12-10", METHOD, &Cache::Absent("NOT_CAPTURED: only AAPL's tape was fetched for this test".into())).await.unwrap();
    }
    assert!(tape::put(&client, "SPY", "2025-12-10", METHOD, &cache).await.is_err(), "an absence is not overwritten either");
    tape::session_done(&mut client, &claim.token, "2025-12-10").await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();

    // a later day reads the session's caches back, byte for byte
    let stored = tape::of_session(&client, "2025-12-10", METHOD).await.unwrap();
    assert_eq!(stored.len(), 64);
    assert_eq!(stored.iter().find(|(k, _)| k == "AAPL").unwrap().1, cache);
    assert_eq!(stored.iter().filter(|(_, c)| matches!(c, Cache::Absent(_))).count(), 63);
    assert!(tape::of_session(&client, "2025-12-10", "another-method").await.unwrap().is_empty());
    scratch.drop().await;
}
