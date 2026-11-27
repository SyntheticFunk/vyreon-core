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

/// C-076 (phase 1, 2026-11-24: Intrinio answered 429 while the calendar minutes of the usage ledger
/// held 1,589-1,845 of 1,900): the ceiling holds over every rolling 60 s window, never a calendar minute.
/// A full ceiling used in the last seconds of one minute holds the next minute's first draws until those
/// requests are 60 s old at the provider; the ledger's minutes are where releases were booked.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_ceiling_holds_over_every_rolling_minute() {
    let (scratch, mut client) = migrated("budget_rolling").await;
    let budget = Budget { per_minute: 1900, ..Budget::default() };
    let mut ids = Vec::new();
    for w in ["w1", "w2", "w3", "w4"] {
        ids.push(granted(budget::draw(&mut client, budget, w, 600).await.unwrap()));
    }
    assert_eq!(ids.iter().map(|(_, n)| n).sum::<i32>(), 1900);
    for ((id, n), w) in ids.iter().zip(["w1", "w2", "w3", "w4"]) {
        budget::release(&mut client, *id, w, &json!({"options": n})).await.unwrap();
    }
    // used in the last ten seconds of the previous calendar minute: a new minute frees nothing
    let shift = |secs: i32| format!(
        "UPDATE core.budget_lease SET granted_at = granted_at - make_interval(secs => {secs}), released_at = released_at - make_interval(secs => {secs}),
                valid_until = valid_until - make_interval(secs => {secs})"
    );
    client.execute(&shift(10), &[]).await.unwrap();
    assert!(matches!(budget::draw(&mut client, budget, "w5", 1).await.unwrap(), Grant::Wait { .. }));
    // 50 s on: still inside a rolling minute of them (with their way to the provider)
    client.execute(&shift(50), &[]).await.unwrap();
    match budget::draw(&mut client, budget, "w5", 1).await.unwrap() {
        Grant::Wait { seconds } => assert!(seconds > 0.0 && seconds <= 6.0, "{seconds}"),
        other => panic!("{other:?}"),
    }
    // past it: the whole ceiling again
    client.execute(&shift(6), &[]).await.unwrap();
    assert_eq!(granted(budget::draw(&mut client, budget, "w5", 600).await.unwrap()).1, 600);
    scratch.drop().await;
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
    let span: f64 = client
        .query_one("SELECT EXTRACT(EPOCH FROM valid_until - granted_at)::float8 FROM core.budget_lease WHERE budget_lease_id = $1", &[&first])
        .await
        .unwrap()
        .get(0);
    assert!((span - 35.0).abs() < 0.01, "{span}");
    let (second, _) = granted(budget::draw(&mut client, budget, "w2", 600).await.unwrap());
    granted(budget::draw(&mut client, budget, "w3", 600).await.unwrap());
    assert_eq!(budget::committed(&client).await.unwrap(), 1800);
    match budget::draw(&mut client, budget, "w4", 100).await.unwrap() {
        // audit 2026-11-14, core.md 14: core's end of a lease is taken from the clock and allows
        // for the answer's way to the worker, which starts its 30 s on receipt
        Grant::Wait { seconds } => assert!((85.0..=95.0).contains(&seconds), "a 30 s lease counts for 30 + 5 + 60 s: {seconds}"),
        other => panic!("{other:?}"),
    }
    // a worker that used less gives the rest back at once
    assert_eq!(budget::release(&mut client, first, "w1", &json!({"daily": 64, "options": 86})).await.unwrap(), 150);
    assert_eq!(budget::committed(&client).await.unwrap(), 1350);
    assert_eq!(granted(budget::draw(&mut client, budget, "w4", 600).await.unwrap()).1, 450, "what is free, when less than asked");
    // a lease is released once, by its own worker
    assert!(budget::release(&mut client, first, "w1", &json!({"daily": 1})).await.is_err());
    assert!(budget::release(&mut client, second, "w1", &json!({"daily": 1})).await.is_err());
    // a worker that reports more than its lease is counted in full and reported (audit 2026-11-14,
    // core.md 13: it was counted at its grant, so 100 requests went unaccounted): nothing more is
    // granted until the overrun has aged out
    let over = budget::release(&mut client, second, "w2", &json!({"options": 700})).await.unwrap_err();
    assert!(over.contains("700 requests on a lease of 600"), "{over}");
    assert_eq!(budget::committed(&client).await.unwrap(), 1900);
    assert!(matches!(budget::draw(&mut client, budget, "w5", 1).await.unwrap(), Grant::Wait { .. }));
    // a lease stops counting a minute after its end; C-076: a released one's end is its release and the
    // requests' way to the provider (a request permitted just before the release still reaches it after)
    client.execute("UPDATE core.budget_lease SET released_at = released_at - interval '62 seconds', granted_at = granted_at - interval '2 minutes', valid_until = valid_until - interval '2 minutes' WHERE budget_lease_id = $1", &[&first]).await.unwrap();
    assert_eq!(budget::committed(&client).await.unwrap(), 1900, "62 s after its release: its last requests may be 57 s old at the provider");
    client.execute("UPDATE core.budget_lease SET released_at = released_at - interval '4 seconds' WHERE budget_lease_id = $1", &[&first]).await.unwrap();
    assert_eq!(budget::committed(&client).await.unwrap(), 1750);
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
    let refused = tape::session_done(&mut client, &claim.token, "2025-12-10", Some(METHOD)).await.unwrap_err();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.starts_with("63 of 64 members have no tape cache of method engine-v1 for 2025-12-10: SPY QQQ ")), "{refused}");
    for (key, _) in doc["members"].as_array().unwrap().iter().map(|m| (m["asset_key"].as_str().unwrap(), ())).filter(|(k, _)| *k != "AAPL") {
        tape::put(&client, key, "2025-12-10", METHOD, &Cache::Absent("NOT_CAPTURED: only AAPL's tape was fetched for this test".into())).await.unwrap();
    }
    assert!(tape::put(&client, "SPY", "2025-12-10", METHOD, &cache).await.is_err(), "an absence is not overwritten either");
    tape::session_done(&mut client, &claim.token, "2025-12-10", None).await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();

    // the session redone under a new method (audit 2026-11-14, core.md 11): the first method's
    // caches do not make it complete, whether the worker names the method or not
    const NEW: &str = "engine-v2";
    work::schedule(&mut client, Kind::Tape, "FREE_REPORT_64", &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let redo = work::claim(&mut client, Kind::Tape, &b, "tape-2", 3600.0).await.unwrap().unwrap();
    assert!(tape::put(&client, "AAPL", "2025-12-10", NEW, &cache).await.unwrap());
    for method in [Some(NEW), None] {
        let refused = tape::session_done(&mut client, &redo.token, "2025-12-10", method).await.unwrap_err();
        assert!(matches!(&refused, LeaseError::Refused(m) if m.starts_with("63 of 64 members have no tape cache of method engine-v2")), "{refused}");
    }
    for key in doc["members"].as_array().unwrap().iter().map(|m| m["asset_key"].as_str().unwrap()).filter(|k| *k != "AAPL") {
        tape::put(&client, key, "2025-12-10", NEW, &Cache::Absent("NOT_CAPTURED: only AAPL's tape was fetched for this test".into())).await.unwrap();
    }
    tape::session_done(&mut client, &redo.token, "2025-12-10", Some(NEW)).await.unwrap();
    work::finish(&mut client, &redo.token).await.unwrap();

    // a later day reads the session's caches back, byte for byte
    let stored = tape::of_session(&client, "2025-12-10", METHOD).await.unwrap();
    assert_eq!(stored.len(), 64);
    assert_eq!(stored.iter().find(|(k, _)| k == "AAPL").unwrap().1, cache);
    assert_eq!(stored.iter().filter(|(_, c)| matches!(c, Cache::Absent(_))).count(), 63);
    assert!(tape::of_session(&client, "2025-12-10", "another-method").await.unwrap().is_empty());
    scratch.drop().await;
}

/// C-068 (audit opsurface ops.md 12 and round 2, 17): a worker that dies after drawing, or whose
/// release never reaches core, leaves nothing in `request_usage`, and usage booked at release splits
/// a minute's requests across two. The ledger books each lease at its grant, states the grants never
/// released, and bounds each minute by its and the previous minute's leases.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_ledger_shows_the_grants_never_released() {
    let (scratch, mut client) = migrated("budget_ledger").await;
    let budget = Budget::default();
    let (first, _) = granted(budget::draw(&mut client, budget, "day-a", 600).await.unwrap());
    let (lost, _) = granted(budget::draw(&mut client, budget, "day-b", 600).await.unwrap());
    budget::release(&mut client, first, "day-a", &json!({"options": 150})).await.unwrap();
    // day-b's pod is killed: its lease is never released; once past its validity it is unreleased
    client.execute("UPDATE core.budget_lease SET valid_until = now() - interval '1 second' WHERE budget_lease_id = $1", &[&lost]).await.unwrap();
    assert_eq!(budget::unreleased(&client, 6.0).await.unwrap(), (1, 600));
    // both granted this minute (`granted_at` is the clock's; the test runs within one minute or
    // across two, so the bound is read where it is highest)
    let minutes = budget::ledger(&client, 6.0, 10).await.unwrap();
    let (released, unreleased): (i64, i64) = (minutes.iter().map(|m| m.released).sum(), minutes.iter().map(|m| m.unreleased).sum());
    assert_eq!((released, unreleased), (150, 600), "{minutes:?}");
    assert_eq!(minutes[0].bound, 750, "{minutes:?}");
    // round 2, 17: two leases of 100 used in one minute, released either side of its end, booked at
    // release fell 100 and 100 into two minutes; booked at grant, the minute after their grants is
    // bounded by both
    let at = |s: &str| format!("date_trunc('minute', now()) - interval '{s}'");
    for (grant, release) in [("10 minutes 40 seconds", "10 minutes 1 second"), ("10 minutes 30 seconds", "9 minutes 59 seconds")] {
        let (id, _) = granted(budget::draw(&mut client, budget, "day-c", 100).await.unwrap());
        client
            .execute(&format!("UPDATE core.budget_lease SET granted_at = {}, valid_until = {} + interval '35 seconds', used = 100, released_at = {} WHERE budget_lease_id = $1", at(grant), at(grant), at(release)), &[&id])
            .await
            .unwrap();
    }
    let minutes = budget::ledger(&client, 6.0, 10).await.unwrap();
    assert!(minutes.iter().any(|m| (m.released, m.unreleased, m.bound, m.workers) == (200, 0, 200, 1)), "the minute's 200 requests in one row: {minutes:?}");
    scratch.drop().await;
}

/// C-095 (the nightly of 2026-11-25: 28 assets' ACT01 capture lost to HTTP 429 with 1,540 to 1,770 requests
/// counted in the minute, eight backfill blocks drawing beside it). A refusal the worker reports is stored with
/// what core had committed, its endpoint without the query; it halves the ceiling, which comes back over five
/// minutes; and while the daily session's worker draws, a backfill worker is held to the ceiling less the reserve.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refusal_halves_the_ceiling_and_the_daily_session_keeps_a_reserve() {
    let (scratch, mut client) = migrated("budget_refusal").await;
    let budget = Budget { per_minute: 1900, daily_reserve: 600, ..Budget::default() };
    // the provider's own words of a refusal (Intrinio's 429 body and Retry-After, as the worker sends them)
    let report = json!({"category": "act01", "endpoint": "/options/trades/by_contract?api_key=SECRET", "status": 429,
                        "retry_after_seconds": 12, "message": "1 Minute Call Limit Reached", "headers": {"retry-after": "12"}});
    granted(budget::draw(&mut client, budget, "day-a", 600).await.unwrap());
    budget::refusal(&client, "day-a", &report).await.unwrap();
    let r = client.query_one("SELECT endpoint, status, retry_after_seconds, message, committed, headers::text FROM core.provider_refusal", &[]).await.unwrap();
    assert_eq!((r.get::<_, String>(0), r.get::<_, i32>(1), r.get::<_, Option<i32>>(2), r.get::<_, Option<String>>(3), r.get::<_, i32>(4)),
               ("/options/trades/by_contract".to_owned(), 429, Some(12), Some("1 Minute Call Limit Reached".to_owned()), 600));
    assert!(!r.get::<_, String>(5).contains("SECRET"));
    assert!(budget::refusal(&client, "day-a", &json!({"endpoint": "/x"})).await.is_err(), "no status");
    // just refused: the ceiling is 950, 600 are held, so 350 are free
    assert_eq!(granted(budget::draw(&mut client, budget, "day-b", 600).await.unwrap()).1, 350);
    assert!(matches!(budget::draw(&mut client, budget, "day-c", 1).await.unwrap(), Grant::Wait { .. }));
    // five minutes on, the ceiling is whole again: 1,900 less the 950 held
    client.execute("UPDATE core.provider_refusal SET at = at - interval '5 minutes'", &[]).await.unwrap();
    assert_eq!(granted(budget::draw(&mut client, budget, "day-c", 600).await.unwrap()).1, 600);
    assert_eq!(granted(budget::draw(&mut client, budget, "day-d", 600).await.unwrap()).1, 350);
    let listed = budget::refusals(&client, 1.0).await.unwrap();
    assert_eq!((listed.len(), listed[0].0.as_str(), listed[0].2, listed[0].4), (1, "/options/trades/by_contract", 1, 600));
    // the reserve: the daily session's worker holds a job of priority 1 and draws
    client.execute("DELETE FROM core.budget_lease", &[]).await.unwrap();
    let fixtures = test_support::fixtures();
    let calendar: Value = serde_json::from_str(test_support::CALENDAR).unwrap();
    work::load_calendar(&mut client, &calendar).await.unwrap();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "budget test"})).await.unwrap();
    work::schedule(&mut client, Kind::Tape, "FREE_REPORT_64", &b, "2025-12-10", "2025-12-11", 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE first_session = '2025-12-11'", &[]).await.unwrap();
    work::claim_as(&mut client, Kind::Tape, &b, "tape-daily-1", 3600.0, true).await.unwrap().unwrap();
    work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
    // the daily worker has not drawn yet: the backfill has the whole ceiling
    assert_eq!(granted(budget::draw(&mut client, budget, "tape-1", 600).await.unwrap()).1, 600);
    assert_eq!(granted(budget::draw(&mut client, budget, "tape-daily-1", 100).await.unwrap()).1, 100);
    // now it draws: the backfill is held to 1,300 (700 are held), the daily worker to 1,900
    assert_eq!(granted(budget::draw(&mut client, budget, "tape-1", 600).await.unwrap()).1, 600);
    assert!(matches!(budget::draw(&mut client, budget, "tape-1", 600).await.unwrap(), Grant::Wait { .. }), "1,300 are held");
    assert_eq!(granted(budget::draw(&mut client, budget, "tape-daily-1", 600).await.unwrap()).1, 600);
    scratch.drop().await;
}
