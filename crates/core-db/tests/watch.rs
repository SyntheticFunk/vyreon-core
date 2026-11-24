//! The owner's channel (C-050): problems found from core's own rows (the engine's real calendar at
//! engine-v1, the slice's real revision header and bodies of 2025-12-10, a real faulted body, the
//! real refused day's evidence), each alerted once and resolved once; the deadlines across a
//! weekend, a Monday holiday, Good Friday and winter time; the digest's verdict in each state;
//! the 2,000-character limit. Run with `cargo test -- --ignored`.
use core_db::evidence::{self, SCHEMA};
use core_db::publish::publish_file;
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Write;
use test_support::slice::{BASE, DERIVED, bundle, inventories, listed_2025_12_10, parts};
use test_support::{ENGINE_FAULT_BODY_XLB, Scratch, WORKER_LOG_REFUSED_DAY};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";

/// A migrated database with the calendar, the universe and the slice's builds; returns the base build.
async fn ready(test: &str) -> (Scratch, Client, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for inventory in inventories() {
        builds.push(build::register(&mut client, &inventory).await.unwrap().engine_build);
    }
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    (scratch, client, builds.remove(0))
}

fn settings(from: Option<&str>) -> Settings {
    // the deadline hour of C-050 (10:00 MT): the mechanism is the same at any hour
    Settings { profile: PROFILE.into(), deadlines_from: from.map(str::to_owned), volume_bytes: None, deadline_hour: 10, daily: None, archive: None, refcheck_from: None, backup_days: None }
}

/// The slice's base revision header (2025-12-10) as the published revision of `day`, written as
/// core writes one, without its slots: the deadlines read only that a base revision is published.
async fn published(client: &Client, build: &str, day: &str, at: &str) {
    client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members)
             VALUES ($1::text::date, $2, 'VYREON-RESEARCH-PROFILE-S64-3.0.0', $3, '2026-11-08T13:49:56.940739Z', 'PUBLISHED', $4::text::timestamptz, $5)",
            &[&day, &PROFILE, &build, &at, &listed_2025_12_10()],
        )
        .await
        .unwrap();
}

async fn check(client: &mut Client, s: &Settings, now: &str) -> watch::Change {
    watch::check(client, s, now).await.unwrap()
}

async fn posts(client: &Client) -> Vec<String> {
    client.query("SELECT message FROM core.notice WHERE post ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

fn opened(v: &[&str]) -> watch::Change {
    watch::Change { opened: v.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

/// Labor Day week 2025 (Monday 2025-09-01 is no session) in summer time (UTC−6): Friday's
/// deadline is Saturday 10:00, Sunday's report is at risk at Saturday 12:00, Tuesday's deadline is
/// Wednesday; a session before the first deadline never alerts; each problem is posted once and
/// resolved once.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn deadlines_follow_the_calendar_over_a_weekend_and_a_holiday() {
    let (scratch, mut client, b) = ready("deadlines").await;
    let s = settings(Some("2025-08-25"));
    for day in ["2025-08-25", "2025-08-26", "2025-08-27", "2025-08-28"] {
        published(&client, &b, day, &format!("{day} 20:00-06")).await;
    }
    // 2025-08-22 (a Friday before the first deadline) is not published and never alerts
    assert_eq!(check(&mut client, &s, "2025-08-30 09:59:59-06").await, opened(&[]));
    assert_eq!(check(&mut client, &s, "2025-08-30 10:00:00-06").await, opened(&["Fri Aug 29 not published by its Sat 10:00 MT deadline"]));
    assert_eq!(check(&mut client, &s, "2025-08-30 11:59:00-06").await, opened(&[]), "no second alert for the same problem");
    assert_eq!(check(&mut client, &s, "2025-08-30 12:00:00-06").await, opened(&["Sunday's report at risk: Fri Aug 29 still not published at Sat 12:00 MT"]));
    // Monday is a holiday: nothing is due on Tuesday morning; Tuesday's deadline is Wednesday
    assert_eq!(check(&mut client, &s, "2025-09-02 10:30:00-06").await, opened(&[]));
    assert_eq!(check(&mut client, &s, "2025-09-03 09:59:00-06").await, opened(&[]));
    assert_eq!(check(&mut client, &s, "2025-09-03 10:00:00-06").await, opened(&["Tue Sep 2 not published by its Wed 10:00 MT deadline"]));
    let open = watch::open(&client, "2025-09-03 10:00:00-06").await.unwrap();
    assert_eq!(open.iter().map(|o| (o.kind.as_str(), o.key.as_str())).collect::<Vec<_>>(), [
        ("SESSION_LATE", "2025-08-29"),
        ("REPORT_AT_RISK", "2025-08-29"),
        ("SESSION_LATE", "2025-09-02")
    ]);
    assert_eq!(watch::age(open[0].age), "4 days");

    // both published: each problem resolved once
    published(&client, &b, "2025-08-29", "2025-09-03 10:20:00-06").await;
    published(&client, &b, "2025-09-02", "2025-09-03 10:25:00-06").await;
    let change = check(&mut client, &s, "2025-09-03 10:30:00-06").await;
    assert_eq!(change.resolved.len(), 3);
    assert_eq!(check(&mut client, &s, "2025-09-03 10:31:00-06").await, watch::Change::default());
    let said = posts(&client).await;
    assert_eq!(said.len(), 6, "three alerts, three resolutions: {said:?}");
    assert_eq!(said[0], "🔴 Fri Aug 29 not published by its Sat 10:00 MT deadline\nTo see more: `vyreon-core queue`");
    assert_eq!(said[3], "✅ Resolved: Fri Aug 29 not published by its Sat 10:00 MT deadline (it was open 4 days)");
    assert_eq!(said[5], "✅ Resolved: Tue Sep 2 not published by its Wed 10:00 MT deadline (it was open 30 min)");
    scratch.drop().await;
}

/// Good Friday 2025 (2025-04-18, no session): Thursday is the week's last session, due Friday 10:00
/// and Sunday's report at risk from Saturday 12:00. A Friday a week before is late but its Sunday
/// is past: no report-at-risk for it. In winter time (UTC−7) 10:00 MT is 17:00 UTC.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_weeks_last_session_carries_sundays_report_and_winter_time_is_followed() {
    let (scratch, mut client, b) = ready("good_friday").await;
    let s = settings(Some("2025-04-07"));
    for day in ["2025-04-07", "2025-04-08", "2025-04-09", "2025-04-10", "2025-04-14", "2025-04-15", "2025-04-16"] {
        published(&client, &b, day, &format!("{day} 20:00-06")).await;
    }
    assert_eq!(
        check(&mut client, &s, "2025-04-18 10:00:00-06").await,
        opened(&["Fri Apr 11 not published by its Sat 10:00 MT deadline", "Thu Apr 17 not published by its Fri 10:00 MT deadline"])
    );
    assert_eq!(check(&mut client, &s, "2025-04-19 12:00:00-06").await, opened(&["Sunday's report at risk: Thu Apr 17 still not published at Sat 12:00 MT"]));
    // the problem stays open past its Sunday until the session publishes
    assert_eq!(check(&mut client, &s, "2025-04-21 08:00:00-06").await, watch::Change::default());
    published(&client, &b, "2025-04-17", "2025-04-21 08:30:00-06").await;
    assert_eq!(check(&mut client, &s, "2025-04-21 09:00:00-06").await.resolved.len(), 2);

    // winter: Friday 2025-12-12 is due Saturday 10:00 MST, 17:00 UTC
    let s = settings(Some("2025-12-08"));
    for day in ["2025-12-08", "2025-12-09", "2025-12-10", "2025-12-11"] {
        published(&client, &b, day, &format!("{day} 20:00-07")).await;
    }
    assert_eq!(check(&mut client, &s, "2025-12-13 16:59:59+00").await.opened, Vec::<String>::new());
    assert_eq!(check(&mut client, &s, "2025-12-13 17:00:00+00").await, opened(&["Fri Dec 12 not published by its Sat 10:00 MT deadline"]));
    scratch.drop().await;
}

/// The real refused day's account (`job_evidence.rs`), gzipped with its real log as the worker sends it.
fn document(session: &str, message: &str) -> Vec<u8> {
    let account = json!({"schema": SCHEMA, "stage": format!("day {session}"), "message": message, "session": session,
                         "refusals": [{"input": "close_quotes", "session": session, "role": "T", "status": "missing", "blocking": true,
                                       "reason": "14016 published option attempts without a quote file"}]});
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(format!("{account}\n{WORKER_LOG_REFUSED_DAY}").as_bytes()).unwrap();
    gz.finish().unwrap()
}

/// One attempt of a one-day block that leaves its evidence and fails.
async fn failed_attempt(client: &mut Client, b: &str, message: &str) -> i64 {
    let claim = work::claim(client, Kind::DayBlock, b, "w1", 3600.0).await.unwrap().unwrap();
    let day = claim.sessions[0].clone();
    evidence::put(client, &claim.token, &evidence::check(document(&day, message)).unwrap()).await.unwrap();
    work::fail(client, &claim.token, &format!("day {day}"), message, "").await.unwrap();
    claim.job_id
}

/// A job out of attempts is a problem named by its evidence's stop message until it is retried;
/// provider errors on more than two attempts in half an hour are one, until two quiet hours.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_last_failure_and_a_provider_burst_are_problems_from_the_evidence() {
    let (scratch, mut client, b) = ready("failures").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2024-07-16", "2024-07-16", 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1", &[]).await.unwrap();
    let stop = "stop: 2024-07-16 not published: 2024-07-16 refused by the input plan (plan.json)";
    let job = failed_attempt(&mut client, &b, stop).await;
    let now = watch::now(&client).await.unwrap();
    let summary = format!("Day 2024-07-16 failed for the last time (job {job}): {stop}");
    assert_eq!(check(&mut client, &s, &now).await, opened(&[&summary]));
    assert_eq!(posts(&client).await, [format!("🔴 {summary}\nTo see more: `vyreon-core evidence show {job}`")]);
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    assert_eq!(check(&mut client, &s, &now).await.resolved, [summary]);
    work::cancel(&mut client, &[job]).await.unwrap();

    // the provider: the real transport error of the refused day's log (line 169), on three attempts
    let provider = r#"options 2024-07-16 GLD: Retryable("exit 3: == failed capture /work/h5-acq/GLD/options/2023-12-19/capture: option acquisition:Transport(\"HttpStatus(502)\")")"#;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-12", 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 5 WHERE state = 'QUEUED'", &[]).await.unwrap();
    failed_attempt(&mut client, &b, provider).await;
    failed_attempt(&mut client, &b, provider).await;
    let now = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &now).await, opened(&[]), "two attempts are not yet the provider");
    failed_attempt(&mut client, &b, provider).await;
    let now = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &now).await, opened(&["Provider (Intrinio) errors stopped 3 attempts in the last 30 minutes (HTTP 502)"]));
    // a worker goes on with the queued blocks (C-065: else the work, claimable with no lease, is a stall)
    work::claim(&mut client, Kind::DayBlock, &b, "w2", 4.0 * 3600.0).await.unwrap().unwrap();
    let in_an_hour = later_text(&client, 1).await;
    // still open within the quiet hours, and (C-050 amended 2026-11-20) restated without a post
    let still = watch::Change { restated: vec!["Provider (Intrinio) errors are still stopping attempts".into()], ..Default::default() };
    assert_eq!(check(&mut client, &s, &in_an_hour).await, still, "still open within the quiet hours");
    assert!(!posts(&client).await.iter().any(|p| p.contains("still stopping")), "a restatement is not posted");
    assert_eq!(check(&mut client, &s, &in_an_hour).await, watch::Change::default(), "restated once");
    let after_three = later_text(&client, 3).await;
    assert_eq!(check(&mut client, &s, &after_three).await.resolved.len(), 1);
    // the three were retried, not failed: handled, in the digest
    let digest = watch::digest(&client, &s, &after_three).await.unwrap();
    assert!(digest.starts_with("🟡 Running; handled: 3 retried attempts · "), "{digest}");
    scratch.drop().await;
}

/// Audit 2026-11-22 opsurface (ops.md) 10, C-068: an earlier attempt left the provider's 502 as its
/// evidence, the final one stopped with no evidence (its worker out of disk); the alert named the
/// 502, the reader the final stop. Both now name the final lease's cause.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_final_failure_is_named_by_its_final_lease() {
    let (scratch, mut client, b) = ready("final_cause").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2024-07-16", "2024-07-16", 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 2", &[]).await.unwrap();
    let provider = r#"options 2024-07-16 GLD: Retryable("exit 3: == failed capture /work/h5-acq/GLD/options/2023-12-19/capture: option acquisition:Transport(\"HttpStatus(502)\")")"#;
    failed_attempt(&mut client, &b, provider).await;
    let last = work::claim(&mut client, Kind::DayBlock, &b, "w2", 3600.0).await.unwrap().unwrap();
    assert!(!work::fail(&mut client, &last.token, "day 2024-07-16", "worker ran out of disk", "").await.unwrap());
    let now = watch::now(&client).await.unwrap();
    let change = check(&mut client, &s, &now).await;
    assert_eq!(change.opened, [format!("Day 2024-07-16 failed for the last time (job {}): day 2024-07-16: worker ran out of disk", last.job_id)]);
    let reader: String = client.query_one("SELECT core.session_cause('2024-07-16', $1)->>'detail'", &[&PROFILE]).await.unwrap().get(0);
    assert!(reader.contains("worker ran out of disk") && !change.opened[0].contains("502"), "{reader}");
    scratch.drop().await;
}

/// Audit 2026-11-22 opsurface (ops.md) 9, C-068: after the provider refused the key (HTTP 401, the
/// engine's transport error as it writes one), an ordered pass that finished and a history revision
/// published cleared the alarm though neither calls the provider. Only a tape or a day block started
/// after the refusal that succeeds clears it now.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refused_key_is_cleared_only_by_work_that_calls_the_provider() {
    let (scratch, mut client, b) = ready("auth").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-11", "2025-12-11", 1).await.unwrap();
    let refused = r#"options 2025-12-11 GLD: Retryable("exit 3: == failed capture /work/h5-acq/GLD/options/2025-12-11/capture: option acquisition:Transport(\"HttpStatus(401)\")")"#;
    let job = failed_attempt(&mut client, &b, refused).await;
    let now = watch::now(&client).await.unwrap();
    let opened_now = check(&mut client, &s, &now).await.opened;
    assert!(opened_now.len() == 1 && opened_now[0].starts_with("The provider (Intrinio) refuses our API key: HTTP 401 on job"), "{opened_now:?}");
    work::cancel(&mut client, &[job]).await.unwrap();
    // an ordered pass runs and finishes, and a history revision is published: no provider call
    published(&client, &b, "2025-12-10", &now).await;
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    client.execute("UPDATE core.job SET state = 'DONE', next_session = NULL, finished_at = clock_timestamp() WHERE kind = 'ORDERED_PASS'", &[]).await.unwrap();
    history_published(&client, "2025-12-10", &later_text(&client, 0).await).await;
    let in_a_minute = later_text(&client, 0).await;
    assert_eq!(check(&mut client, &s, &in_a_minute).await.resolved, Vec::<String>::new(), "a pass proves nothing of the key");
    // round 2, 15: a tape started after the refusal that finds every cache held already finishes
    // without a request: not cleared; one that stores a cache (the real AAPL cache) clears it
    for (session, stores) in [("2025-12-12", false), ("2025-12-15", true)] {
        work::schedule(&mut client, Kind::Tape, PROFILE, &b, session, session, 1).await.unwrap();
        let tape = work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
        if stores {
            assert!(core_db::tape::put(&client, "AAPL", session, "engine-v1", &core_db::tape::Cache::Body(test_support::TAPE_AAPL.to_vec())).await.unwrap());
        }
        let tx = client.transaction().await.unwrap();
        let held = work::held(&tx, &tape.token).await.unwrap();
        work::session_done(&tx, &held, session).await.unwrap();
        tx.commit().await.unwrap();
        work::finish(&mut client, &tape.token).await.unwrap();
        let after = later_text(&client, 0).await;
        assert_eq!(check(&mut client, &s, &after).await.resolved.len(), usize::from(stores), "{session}");
    }
    scratch.drop().await;
}

async fn later_text(client: &Client, hours: i32) -> String {
    client.query_one("SELECT (now() + make_interval(hours => $1))::text", &[&hours]).await.unwrap().get(0)
}

/// The verdict in each state, the coverage of the newest session against the typical one from
/// the real bodies (one of them the engine's real faulted slot), and the digest and weekly check
/// queued once at 09:00 MT.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_digest_leads_with_its_verdict() {
    let (scratch, mut client, b) = ready("digest").await;
    let s = settings(None);
    // the slice's real base revision of the 10th, published for the 9th and the 10th
    for day in ["2025-12-09", "2025-12-10"] {
        let (mut h, sc, f) = parts(BASE);
        h.represented_date = day.into();
        let path = bundle(&format!("watch-{day}"), &h, &sc, &f);
        publish_file(&mut client, &path).await.unwrap();
        std::fs::remove_file(&path).unwrap();
    }
    // the engine's real faulted slot of the 10th (XLB's SAI-EXQ01) stored in the 10th's revision
    let body = ENGINE_FAULT_BODY_XLB.trim_end();
    let body_json: Value = serde_json::from_str(body).unwrap();
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             SELECT revision_id, 'SAI-EXQ01', 'XLB', represented_date, 'DELIVERED', 'engine-fault', 'engine-fault', $1, $2, $3::text::jsonb, '{}'
             FROM vyreon.day_revision WHERE represented_date = '2025-12-10'",
            &[&core_contract::sha256_hex(ENGINE_FAULT_BODY_XLB.as_bytes()), &body, &body_json.to_string()],
        )
        .await
        .unwrap();
    let now = watch::now(&client).await.unwrap();
    let today: String = client.query_one("SELECT to_char(now() AT TIME ZONE 'America/Edmonton', 'Dy Mon FMDD')", &[]).await.unwrap().get(0);

    // round x, 7: the tallies in one statement: counted the first time (and kept), read the next,
    // the same either way
    let dates = vec!["2025-12-09".to_owned(), "2025-12-10".to_owned()];
    let tallied = async |c: &Client| c.query_one("SELECT count(*) FROM core.revision_tally", &[]).await.unwrap().get::<_, i64>(0);
    assert_eq!(tallied(&client).await, 0);
    let counted = watch::session_tallies(&client, PROFILE, &dates).await.unwrap();
    assert_eq!(counted.len(), 2);
    assert_eq!((counted["2025-12-10"].engine_faults, counted["2025-12-09"].engine_faults), (1, 0));
    assert_eq!(tallied(&client).await, 2);
    assert_eq!(watch::session_tallies(&client, PROFILE, &dates).await.unwrap(), counted);

    // 🟢: nothing open, nothing handled
    check(&mut client, &s, &now).await;
    let green = watch::digest(&client, &s, &now).await.unwrap();
    let lines: Vec<&str> = green.lines().collect();
    assert_eq!(lines[0], format!("🟢 Running fine · {today}"));
    assert_eq!(lines[1], "Latest session stored: Wed Dec 10, 2025 · 80.0% of its measurements have a value (typical 100.0%) · 1 engine fault");
    assert_eq!(lines[2], "Sessions published in the last 24 hours: 2 (2025-12-09 to 2025-12-10)");
    assert_eq!(lines[3], "Provider gaps recorded in the last 24 hours: none");
    assert_eq!(lines[4], "Work queue: nothing waiting or running");
    assert!(lines[5].starts_with("Database size: ") && lines[5].ends_with("(its volume's size is not set, so it is not checked)"), "{}", lines[5]);
    assert_eq!(lines[6], "Publishing deadlines: not checked yet (no first session is set)");
    assert_eq!(*lines.last().unwrap(), "Open problems: none");
    assert!(green.chars().count() <= watch::LIMIT);

    // 🟡: an attempt retried (its lease lapsed), nothing open
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-12", "2025-12-12", 1).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", 3600.0).await.unwrap().unwrap();
    client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1", &[&claim.job_id]).await.unwrap();
    work::sweep(&mut client).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    check(&mut client, &s, &now).await;
    let yellow = watch::digest(&client, &s, &now).await.unwrap();
    assert!(yellow.starts_with(&format!("🟡 Running; handled: 1 retried attempt · {today}\n")), "{yellow}");
    assert!(yellow.contains("\nAttempts retried in the last 24 hours: 1\nWork queue: day blocks 1 waiting\nBackfill progress: 0 of 1 sessions done · no block running, so no estimate\n"), "{yellow}");

    // 🔴: the queue paused (as a refused bundle pauses it)
    let tx = client.transaction().await.unwrap();
    work::pause(&tx, &format!("job {} (w1): bundle refused: unreadable bundle", claim.job_id)).await.unwrap();
    tx.commit().await.unwrap();
    let now = watch::now(&client).await.unwrap();
    check(&mut client, &s, &now).await;
    let red = watch::digest(&client, &s, &now).await.unwrap();
    let paused = format!("Work queue paused, nothing is handed out: job {} (w1): bundle refused: unreadable bundle", claim.job_id);
    assert!(red.starts_with(&format!("🔴 Needs you: {paused} · {today}\n")), "{red}");
    assert!(red.ends_with(&format!("\nOpen problems (1):\n• {paused} · open 0 min · `vyreon-core queue`")), "{red}");

    // the digest at 09:00 MT, once; the weekly check on Saturdays
    let at = |t: &str| t.to_owned();
    for (t, digests) in [(at("2025-12-12 08:59:00-07"), 0), (at("2025-12-12 09:00:00-07"), 1), (at("2025-12-12 15:00:00-07"), 1), (at("2025-12-13 09:00:00-07"), 2)] {
        watch::tick(&mut client, &s, &t).await.unwrap();
        let n: i64 = client.query_one("SELECT count(*) FROM core.notice WHERE once LIKE 'digest %'", &[]).await.unwrap().get(0);
        assert_eq!(n, digests, "at {t}");
    }
    let weekly: String = client.query_one("SELECT message FROM core.notice WHERE once = 'weekly 2025-12-13'", &[]).await.unwrap().get(0);
    let lines: Vec<&str> = weekly.lines().collect();
    // the queue is still paused: red, though no deadline is set (round w, 12)
    assert_eq!(lines[0], format!("🔴 Weekly check: Sunday's report is not cleared while a problem is open: {paused}"));
    assert_eq!(lines[1..6], [
        "Mon Dec 8: not published",
        "Tue Dec 9: published · 100.0% of its measurements have a value · 0 engine faults · history pass not published",
        "Wed Dec 10: published · 80.0% of its measurements have a value · 1 engine fault · history pass not published",
        "Thu Dec 11: not published",
        "Fri Dec 12: not published"
    ]);
    let watched = watch::weekly(&client, &settings(Some("2025-12-08")), "2025-12-13 09:00:00-07").await.unwrap();
    // C-051: Friday's deadline (Saturday 10:00 here) is still ahead at 09:00: due, not missing
    assert!(watched.starts_with("🔴 Weekly check: Sunday's report at risk, 2 of 5 sessions not published (Mon Dec 8, Thu Dec 11)\n"), "{watched}");
    assert!(watched.contains("\nFri Dec 12: not published yet, due by Sat 10:00 MT\n"), "{watched}");
    scratch.drop().await;
}

/// The history pass of `day` (a derived revision on the day's base, not day-local), published at
/// `at`, written as core writes one, without its slots.
async fn history_published(client: &Client, day: &str, at: &str) {
    // the slice's derived revision header: its profile revision and build
    let (h, _, _) = parts(DERIVED);
    client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members, base_revision)
             SELECT $1::text::date, $2, $6, $3, '2026-11-08T13:49:56.940739Z', 'PUBLISHED', $4::text::timestamptz, $5, revision_id
             FROM vyreon.day_revision WHERE represented_date = $1::text::date AND profile_id = $2 AND base_revision IS NULL AND state = 'PUBLISHED'",
            &[&day, &PROFILE, &h.engine_build, &at, &listed_2025_12_10(), &h.profile_revision],
        )
        .await
        .unwrap();
}

/// Audit 2026-11-17 round v, 6: the weekly check is green only when every session's base and
/// history pass are published and no problem is open. The week of 2025-12-08, Saturday after
/// Friday's deadline.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_weekly_check_needs_the_history_pass_and_no_open_problem() {
    let (scratch, mut client, b) = ready("weekly").await;
    let s = settings(Some("2025-12-08"));
    let days = ["2025-12-08", "2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];
    for day in days {
        published(&client, &b, day, &format!("{day} 20:00-07")).await;
    }
    for day in &days[..4] {
        history_published(&client, day, &format!("{day} 22:00-07")).await;
    }
    let saturday = "2025-12-13 11:00:00-07";
    check(&mut client, &s, saturday).await;
    let weekly = watch::weekly(&client, &s, saturday).await.unwrap();
    let lines: Vec<&str> = weekly.lines().collect();
    assert_eq!(lines[0], "🟡 Weekly check: Sunday's report waits for the history pass of 1 of 5 sessions (Fri Dec 12)");
    assert_eq!(lines[4..6], [
        "Thu Dec 11: published · no measurements · 0 engine faults · history pass published",
        "Fri Dec 12: published · no measurements · 0 engine faults · history pass not published"
    ]);
    history_published(&client, days[4], "2025-12-13 10:30:00-07").await;
    let weekly = watch::weekly(&client, &s, saturday).await.unwrap();
    assert!(weekly.starts_with("🟢 Weekly check: all 5 of this week's sessions published with their history pass; Sunday's report has what it needs\n"), "{weekly}");
    // a problem open (the queue paused): never green
    work::pause(&client, "a pass refused its bundle").await.unwrap();
    check(&mut client, &s, saturday).await;
    let weekly = watch::weekly(&client, &s, saturday).await.unwrap();
    assert!(
        weekly.starts_with("🔴 Weekly check: Sunday's report is not cleared while a problem is open: Work queue paused, nothing is handed out: a pass refused its bundle\n"),
        "{weekly}"
    );
    assert!(weekly.contains("\nOpen problems (1):\n"), "{weekly}");
    scratch.drop().await;
}

/// Audit 2026-11-17 round v, 7: a problem a publish opens while the 09:00 digest is composed is
/// in the digest, or its alert follows the digest; never a green digest after its alert. Here the
/// publish's transaction (which opens NOT_DERIVED with its alert) is still open when the watch's
/// minute starts: the watch waits for it and the digest says it is open.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_digest_never_says_nothing_is_open_after_an_alert() {
    let (scratch, mut publishing, _) = ready("digest_race").await;
    let s = settings(None);
    let tx = publishing.transaction().await.unwrap();
    watch::not_derived(&tx, &[("2025-12-11".into(), PROFILE.into(), false)], "A republish left").await.unwrap();
    let mut watching = scratch.client().await;
    let (s2, nine) = (s.clone(), "2025-12-12 09:30:00-07");
    let minute = tokio::spawn(async move {
        watch::tick(&mut watching, &s2, nine).await.unwrap();
        watching
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!minute.is_finished(), "the watch waits for the publish that is opening a problem");
    tx.commit().await.unwrap();
    let watching = minute.await.unwrap();
    let posts: Vec<(i64, String)> =
        watching.query("SELECT notice_id, message FROM core.notice WHERE post ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(posts[0].1.starts_with("🔴 A republish left 1 days that no job derives again: 2025-12-11"), "{posts:?}");
    assert!(posts[1].1.starts_with("🔴 Needs you: A republish left 1 days"), "the digest after the alert says it is open: {posts:?}");
    scratch.drop().await;
}

/// Audit 2026-11-17 round w, 12: the headline agrees with its lines. Saturday 09:00, Friday's base
/// published and its history pass not yet, before Friday's deadline (Saturday 10:00 here): due,
/// never "all published with their history pass". With no deadline set, an open problem is red.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_weekly_headline_agrees_with_its_lines() {
    let (scratch, mut client, b) = ready("weekly_lines").await;
    let s = settings(Some("2025-12-08"));
    let days = ["2025-12-08", "2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];
    for day in days {
        published(&client, &b, day, &format!("{day} 20:00-07")).await;
    }
    for day in &days[..4] {
        history_published(&client, day, &format!("{day} 22:00-07")).await;
    }
    let nine = "2025-12-13 09:00:00-07";
    check(&mut client, &s, nine).await;
    let weekly = watch::weekly(&client, &s, nine).await.unwrap();
    let lines: Vec<&str> = weekly.lines().collect();
    assert_eq!(lines[0], "🟢 Weekly check: on time; 4 of 5 sessions published with their history pass, 1 due later (Fri Dec 12)");
    assert_eq!(lines[5], "Fri Dec 12: published · no measurements · 0 engine faults · history pass not published yet, due by Sat 10:00 MT");
    // no deadline set: the week is not judged, but an open problem is still red
    work::pause(&client, "a pass refused its bundle").await.unwrap();
    check(&mut client, &settings(None), nine).await;
    let weekly = watch::weekly(&client, &settings(None), nine).await.unwrap();
    assert!(weekly.starts_with("🔴 Weekly check: Sunday's report is not cleared while a problem is open: Work queue paused"), "{weekly}");
    scratch.drop().await;
}

/// Audit 2026-11-17 round w, 13: a check that fails composes no digest from the stale problem
/// list; the next minute's check, once it succeeds, does. C-069: the check that could not run is a
/// problem on the channel itself (WATCH_FAILING), the tick is Err (so `/live` stops being told),
/// and it resolves once the check runs. (The check fails here because a problem cannot be written
/// for the minute: a trigger refuses the queue's pause, the way a constraint error would.)
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failed_check_posts_no_digest() {
    let (scratch, mut client, _) = ready("failed_check").await;
    let s = settings(None);
    work::pause(&client, "a check failed on 2025-12-11").await.unwrap();
    client
        .batch_execute(
            "CREATE FUNCTION core.refuse_pause() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN IF NEW.kind = 'QUEUE_PAUSED' THEN RAISE EXCEPTION 'problem refused (test)'; END IF; RETURN NEW; END $$;
             CREATE TRIGGER refuse_pause BEFORE INSERT ON core.problem FOR EACH ROW EXECUTE FUNCTION core.refuse_pause();",
        )
        .await
        .unwrap();
    let why = watch::tick(&mut client, &s, "2025-12-12 09:00:00-07").await.unwrap_err();
    assert!(why.contains("problem refused (test)"), "{why}");
    let said = posts(&client).await;
    assert_eq!(said.len(), 1, "the failed check is said, and no digest from a list the check could not bring up to date: {said:?}");
    assert!(said[0].starts_with("🔴 Watch failing: cannot run its checks, so no problem opens or resolves (problem refused (test))"), "{said:?}");
    client.batch_execute("DROP TRIGGER refuse_pause ON core.problem").await.unwrap();
    let change = watch::tick(&mut client, &s, "2025-12-12 09:01:00-07").await.unwrap();
    assert_eq!(change.resolved.len(), 1, "{change:?}");
    let said = posts(&client).await;
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(said[1].starts_with("🔴 Work queue paused"), "{said:?}");
    assert!(said[2].starts_with("✅ Resolved: Watch failing: cannot run its checks"), "{said:?}");
    assert!(said[3].starts_with("🔴 Needs you: Work queue paused") && !said[3].contains("Watch failing"), "the digest names what holds after the check: {said:?}");
    scratch.drop().await;
}

/// C-069 (audit 2026-11-23, Codex 8, Gemini 7): one part of the check that fails (a table it reads
/// renamed away for the minute: the queue's pause, which the stalls' part reads too) stops only itself. Its problem already open stays open (a part that
/// could not look resolves nothing), the part's failure is a WATCH_FAILING problem with its error,
/// the other parts still run, the digest is still posted and names it; once the part reads again
/// its failure resolves and its problem is judged again.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failing_part_of_the_check_stops_only_itself() {
    let (scratch, mut client, b) = ready("failing_part").await;
    let s = settings(Some("2025-12-10"));
    work::pause(&client, "checking the watch's parts").await.unwrap();
    watch::tick(&mut client, &s, "2025-12-12 08:00:00-07").await.unwrap();
    let kinds = |open: Vec<watch::Open>| open.into_iter().map(|o| (o.kind, o.key)).collect::<Vec<_>>();
    let before = kinds(watch::open(&client, "2025-12-12 08:00:00-07").await.unwrap());
    assert_eq!(before.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["SESSION_LATE", "QUEUE_PAUSED"], "{before:?}");
    // the queue's part cannot read its table; the deadlines' part still resolves what was published
    client.batch_execute("ALTER TABLE core.queue_control RENAME TO queue_control_away").await.unwrap();
    published(&client, &b, "2025-12-10", "2025-12-12 08:30:00-07").await;
    let change = watch::tick(&mut client, &s, "2025-12-12 09:00:00-07").await.unwrap();
    let missing = "relation \"core.queue_control\" does not exist";
    assert_eq!(change.failing, [format!("queue: {missing}"), format!("stalls: {missing}")], "{change:?}");
    assert_eq!(change.resolved, ["Wed Dec 10 not published by its Thu 10:00 MT deadline"], "{change:?}");
    let failing = format!("Watch failing: cannot check whether the work queue is paused ({missing})");
    let stalls = format!("Watch failing: cannot check for stalled work ({missing})");
    assert_eq!(change.opened, [failing.clone(), stalls.clone()], "{change:?}");
    let open = watch::open(&client, "2025-12-12 09:00:00-07").await.unwrap();
    assert_eq!(open.iter().map(|o| o.kind.as_str()).collect::<Vec<_>>(), ["QUEUE_PAUSED", "WATCH_FAILING", "WATCH_FAILING"], "the pause stays open, not resolved by a part that could not look");
    let said = posts(&client).await;
    let digest = said.last().unwrap();
    assert!(digest.starts_with("🔴 Needs you: Work queue paused") && digest.contains(&failing), "the digest is posted and names the failure: {said:?}");
    client.batch_execute("ALTER TABLE core.queue_control_away RENAME TO queue_control").await.unwrap();
    let change = watch::tick(&mut client, &s, "2025-12-12 09:01:00-07").await.unwrap();
    assert_eq!((change.resolved, change.opened.len(), change.failing.len()), (vec![failing, stalls], 0, 0));
    let open = watch::open(&client, "2025-12-12 09:01:00-07").await.unwrap();
    assert_eq!(open.iter().map(|o| o.kind.as_str()).collect::<Vec<_>>(), ["QUEUE_PAUSED"]);
    scratch.drop().await;
}

/// Audit 2026-11-17 round w, 4: the digest counts measurement bodies outside the problems' lock.
/// While its counting is held (another session holds the measurements locked), a publish opens a
/// problem without waiting; the digest composed from the list before it is then not queued, and
/// the next minute's digest names it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_problem_opens_while_the_digest_counts() {
    let (scratch, client, b) = ready("digest_counting").await;
    let s = settings(None);
    // a published revision not yet counted: the digest's count reads its measurements
    published(&client, &b, "2025-12-10", "2025-12-11 20:00-07").await;
    let mut holder = scratch.client().await;
    let hold = holder.transaction().await.unwrap();
    hold.batch_execute("LOCK TABLE vyreon.measurement IN ACCESS EXCLUSIVE MODE").await.unwrap();
    let mut watching = scratch.client().await;
    let s2 = s.clone();
    let minute = tokio::spawn(async move {
        let r = watch::tick(&mut watching, &s2, "2025-12-12 09:30:00-07").await;
        (watching, r)
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!minute.is_finished(), "the digest is counting (held by the lock on the measurements)");
    // a publish opens a problem meanwhile: it does not wait for the digest
    let mut publishing = scratch.client().await;
    let tx = publishing.transaction().await.unwrap();
    let opened = tokio::time::timeout(std::time::Duration::from_secs(2), watch::not_derived(&tx, &[("2025-12-11".into(), PROFILE.into(), false)], "A republish left")).await;
    assert!(opened.is_ok(), "the problem opened while the digest was counting");
    tx.commit().await.unwrap();
    hold.rollback().await.unwrap();
    let (mut watching, r) = minute.await.unwrap();
    r.unwrap();
    let said = posts(&client).await;
    assert_eq!(said.len(), 1, "the digest composed before the problem opened is not queued: {said:?}");
    watch::tick(&mut watching, &s, "2025-12-12 09:31:00-07").await.unwrap();
    let said = posts(&client).await;
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].starts_with("🔴 Needs you: A republish left 1 days"), "{said:?}");
    scratch.drop().await;
}

/// Weeks of missed deadlines: one alert each, and a digest that stays under Discord's limit.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn many_open_problems_fit_one_post() {
    let (scratch, mut client, _) = ready("many").await;
    let s = settings(Some("2025-03-03"));
    let now = "2025-06-02 12:00:00-06";
    let change = check(&mut client, &s, now).await;
    let late: i64 = client.query_one("SELECT count(*) FROM core.session WHERE session BETWEEN '2025-03-03' AND '2025-05-30'", &[]).await.unwrap().get(0);
    assert_eq!(change.opened.len() as i64, late, "one per late session; no report-at-risk for weeks whose Sunday is past");
    assert_eq!(check(&mut client, &s, now).await, watch::Change::default());
    assert_eq!(posts(&client).await.len() as i64, late);
    let digest = watch::digest(&client, &s, now).await.unwrap();
    assert!(digest.chars().count() <= watch::LIMIT, "{}", digest.chars().count());
    assert!(digest.starts_with("🔴 Needs you: Mon Mar 3 not published by its Tue 10:00 MT deadline (and "), "{digest}");
    assert!(digest.contains(&format!("\nOpen problems ({late}):\n")));
    assert!(digest.ends_with(" more: `vyreon-core problems`"), "{digest}");
    scratch.drop().await;
}

/// `now` moved on by `minutes`, as the database reads it.
async fn minutes_after(client: &Client, now: &str, minutes: i32) -> String {
    client.query_one("SELECT ($1::text::timestamptz + make_interval(mins => $2))::text", &[&now, &minutes]).await.unwrap().get(0)
}

/// A check `minutes` after `now`.
async fn check_after(client: &mut Client, s: &Settings, now: &str, minutes: i32) -> watch::Change {
    let at = minutes_after(client, now, minutes).await;
    check(client, s, &at).await
}

/// C-065 (health.sh's stall): work claimable and no worker holding a lease for 15 minutes is one
/// problem, resolved when a worker claims; a lease that ends starts the clock again; when only
/// daily runs' jobs wait, 30 minutes (CronJob `daily` starts 15 minutes after the queueing).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn claimable_work_with_no_worker_for_a_quarter_hour_is_a_stall() {
    let (scratch, mut client, b) = ready("stall").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-08", "2025-12-10", 1).await.unwrap();
    let t0 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[]));
    assert_eq!(check_after(&mut client, &s, &t0, 14).await, opened(&[]));
    let since: String = client.query_one("SELECT to_char(since AT TIME ZONE 'America/Edmonton', 'Dy HH24:MI') FROM core.stall_seen WHERE kind = 'TAPE'", &[]).await.unwrap().get(0);
    let stalled = format!("Tape jobs stalled since {since} MT: 3 claimable and none leased, so no `tape` pod is running");
    assert_eq!(check_after(&mut client, &s, &t0, 15).await, opened(&[&stalled]));
    assert_eq!(posts(&client).await, [format!("🔴 {stalled}\nTo see more: `vyreon-core queue`")]);
    // a worker claims: resolved, and the clock is gone
    let claim = work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
    assert_eq!(check_after(&mut client, &s, &t0, 16).await.resolved, [stalled]);
    assert_eq!(client.query_one("SELECT count(*) FROM core.stall_seen", &[]).await.unwrap().get::<_, i64>(0), 0);

    // its job cancelled, the other two are daily runs' jobs: 30 minutes before a stall
    work::cancel(&mut client, &[claim.job_id]).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE state = 'QUEUED'", &[]).await.unwrap();
    let t1 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t1).await, opened(&[]));
    assert_eq!(check_after(&mut client, &s, &t1, 29).await, opened(&[]), "a daily run's start takes up to 30 minutes");
    let opened_now = check_after(&mut client, &s, &t1, 30).await.opened;
    assert_eq!(opened_now.len(), 1);
    assert!(opened_now[0].contains(": 2 claimable and none leased, so no `tape` pod"), "{opened_now:?}");
    // a paused queue hands nothing out: that is its own problem, not a stall
    work::pause(&client, "watch test").await.unwrap();
    let paused = check_after(&mut client, &s, &t1, 31).await;
    assert_eq!(paused.resolved.len(), 1);
    assert!(paused.opened[0].starts_with("Work queue paused"), "{paused:?}");
    scratch.drop().await;
}

/// Audit 2026-11-22 opsurface (ops.md) 8 and 5, C-068, and its round 2 (13, 14): one live lease
/// cleared the stall's clock for every kind, so a day block renewing its lease for a day hid tapes with no pod (their pods Pending
/// for a node, or failing before they claim), and work all waiting on something nothing would do was
/// no stall at all. A stall is now per kind, and the chain is one too: nothing claimable, nothing
/// leased, queued work of a running build left.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_stall_is_per_kind_and_a_waiting_chain_is_one() {
    let (scratch, mut client, b) = ready("stall_kind").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    // a block of the 8th runs; the tapes of the 10th and 11th wait with no pod
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-08", "2025-12-08", 1).await.unwrap();
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-10", "2025-12-11", 1).await.unwrap();
    work::claim(&mut client, Kind::DayBlock, &b, "day-1", 4.0 * 3600.0).await.unwrap().unwrap();
    let t0 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[]));
    let since: String = client.query_one("SELECT to_char(since AT TIME ZONE 'America/Edmonton', 'Dy HH24:MI') FROM core.stall_seen WHERE kind = 'TAPE'", &[]).await.unwrap().get(0);
    let tapes = format!("Tape jobs stalled since {since} MT: 2 claimable and none leased, so no `tape` pod is running");
    assert_eq!(check_after(&mut client, &s, &t0, 15).await, opened(&[&tapes]), "the block's lease hides no tape stall");
    // a tape pod claims: that kind resolves
    work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
    assert_eq!(check_after(&mut client, &s, &t0, 16).await.resolved, [tapes]);
    let rest: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE state IN ('QUEUED', 'LEASED')", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    work::cancel(&mut client, &rest).await.unwrap();

    // round 2, 14: a pass whose day no block computes (a missing day, or one not scheduled) holds
    // nothing: no chain
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &b, "2025-12-10", "2025-12-11", 1).await.unwrap();
    let t1 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t1).await, opened(&[]));
    assert_eq!(check_after(&mut client, &s, &t1, 20).await, opened(&[]), "nothing waits on a missing day's pass");
    // the chain: the days' block waits for a tape of a build since withdrawn (the claim's gate counts
    // it), a pass of a running build stands FAILED beside it (its own problem: the passes are not
    // counted, the block is)
    let w: String = client.query_one("SELECT engine_build FROM core.engine_build WHERE engine_build <> $1", &[&b]).await.unwrap().get(0);
    build::admit(&client, &w, &json!({"checklist": "watch test"})).await.unwrap();
    work::schedule(&mut client, Kind::Tape, PROFILE, &w, "2025-12-09", "2025-12-09", 1).await.unwrap();
    build::withdraw(&mut client, &w, "an image replaced, its tape left behind").await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 2).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &b, "2025-12-08", "2025-12-08", 1).await.unwrap();
    client.execute("UPDATE core.job SET state = 'FAILED' WHERE kind = 'ORDERED_PASS' AND first_session = '2025-12-08'", &[]).await.unwrap();
    let t2 = watch::now(&client).await.unwrap();
    let failed = check(&mut client, &s, &t2).await.opened;
    assert!(failed.len() == 1 && failed[0].starts_with("Ordered pass for 2025-12-08 failed for the last time"), "{failed:?}");
    let since: String = client.query_one("SELECT to_char(since AT TIME ZONE 'America/Edmonton', 'Dy HH24:MI') FROM core.stall_seen WHERE kind = 'CHAIN'", &[]).await.unwrap().get(0);
    let chain = format!(
        "Work stalled since {since} MT: 1 jobs queued (1 day blocks), none claimable and none leased: they wait for work that is not running (a withdrawn build's? `build hand-over`)"
    );
    assert_eq!(check_after(&mut client, &s, &t2, 15).await, opened(&[&chain]));
    // a paused queue is its own problem, never a chain
    work::pause(&client, "watch test").await.unwrap();
    let paused = check_after(&mut client, &s, &t2, 60).await;
    assert_eq!(paused.resolved, [chain]);
    assert!(paused.opened.len() == 1 && paused.opened[0].starts_with("Work queue paused"), "{paused:?}");
    work::resume(&client).await.unwrap();

    // round 2, 13: twelve healthy backfill blocks hid a daily block with no pod (its own ScaledJob
    // broken): a daily session's blocks are their own group, 30 minutes as before
    let rest: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE state IN ('QUEUED', 'LEASED', 'FAILED')", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    work::cancel(&mut client, &rest).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-05", "2025-12-05", 1).await.unwrap();
    work::claim(&mut client, Kind::DayBlock, &b, "day-backfill", 4.0 * 3600.0).await.unwrap().unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-08", "2025-12-08", 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE kind = 'DAY_BLOCK' AND state = 'QUEUED'", &[]).await.unwrap();
    let t3 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t3).await.opened, Vec::<String>::new(), "(the cancelled pass and the pause resolve)");
    assert_eq!(check_after(&mut client, &s, &t3, 29).await, opened(&[]));
    let daily = check_after(&mut client, &s, &t3, 30).await.opened;
    assert!(daily.len() == 1 && daily[0].starts_with("Daily day blocks stalled since ") && daily[0].ends_with("1 claimable and none leased, so no `day-block-daily` pod is running"), "{daily:?}");
    scratch.drop().await;
}

/// C-068 (audit opsurface ops.md 2) and its round 2 (2, 3): the watch reads the server's WAL and
/// archiver, and the backups only from the plugin's own record (`core.backup_evidence`, which core
/// keeps from the Kubernetes API). On the test server (a superuser: `pg_monitor`'s functions open to
/// it; archiving off) the volume's use counts every database and the WAL. With backups watched: no
/// record is a problem (no evidence), a record with a recent recoverable base backup is none, a
/// Cluster without the plugin's archiver is `WAL_ARCHIVING`, a failed read is unverified again.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_backups_are_judged_on_the_plugins_record_alone() {
    let (scratch, mut client, _) = ready("storage").await;
    let now = watch::now(&client).await.unwrap();
    let st = watch::storage(&client, &now).await.unwrap();
    let this: i64 = client.query_one("SELECT pg_database_size(current_database())", &[]).await.unwrap().get(0);
    assert!(st.readable && st.data > this && st.wal.is_some_and(|w| w >= 16 << 20), "{st:?}");
    let mut s = settings(None);
    s.volume_bytes = Some(250 << 30);
    assert_eq!(check(&mut client, &s, &now).await, opened(&[]), "{st:?}");
    s.backup_days = Some(8);
    let never = "Backups unverified: core has never read the backup plugin's record (VYREON_CORE_PG_CLUSTER, its Role: README B0)";
    assert_eq!(check(&mut client, &s, &now).await, opened(&[never]));
    // the plugin's record: archiving through it, a base backup three days ago
    let three_days: String = client.query_one("SELECT to_char((now() - interval '3 days') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')", &[]).await.unwrap().get(0);
    let cluster = json!({"spec": {"plugins": [{"name": watch::BARMAN_PLUGIN, "isWALArchiver": true, "parameters": {"barmanObjectName": "vyreon-backup"}}]}});
    let store = json!({"status": {"serverRecoveryWindow": {"vyreon-pg": {"firstRecoverabilityPoint": three_days, "lastSuccessfulBackupTime": three_days}}}});
    let good = watch::BackupEvidence::from_objects("vyreon-pg", &cluster, Some(&store));
    watch::put_backup_evidence(&client, Ok(&good)).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &now).await.resolved, [never]);
    let digest = watch::digest(&client, &s, &now).await.unwrap();
    assert!(digest.contains(" of 250.0 GiB (") && digest.contains("newest recoverable base backup 3 days ago (the plugin's record)"), "{digest}");
    // the plugin taken out of the Cluster: WAL goes nowhere, whatever PostgreSQL counts as archived
    let bare = watch::BackupEvidence::from_objects("vyreon-pg", &json!({"spec": {}}), None);
    watch::put_backup_evidence(&client, Ok(&bare)).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    let off = check(&mut client, &s, &now).await.opened;
    assert!(off.iter().any(|o| o.starts_with("WAL archiving is off")) && off.iter().any(|o| o.starts_with("No recoverable base backup")), "{off:?}");
    // a read that fails: unverified, never the last good record
    watch::put_backup_evidence(&client, Err("GET objectstores/vyreon-backup: 403 Forbidden")).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    let change = check(&mut client, &s, &now).await;
    assert!(change.restated.iter().any(|r| r.starts_with("Backups unverified: core cannot read the backup plugin's record: GET objectstores/vyreon-backup: 403")), "{change:?}");
    assert!(change.resolved.is_empty(), "nothing is resolved without evidence: {change:?}");
    scratch.drop().await;
}

/// Audit round 2 of core-watch, 7: the stall's clock survived a pause and resume the watch did not
/// see (a restart over an overnight pause), so the resumed queue read as stalled at once. The
/// clock now starts no earlier than the resume (the queue's `changed_at`, set here to when the
/// resume happens on the test's clock, as `resume` stamps it).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_stall_counts_from_when_the_queue_was_resumed() {
    let (scratch, mut client, b) = ready("stall_resume").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-09", "2025-12-10", 1).await.unwrap();
    let t0 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[]));
    // the watch away: the queue paused, then resumed 20 minutes on
    work::pause(&client, "watch test").await.unwrap();
    work::resume(&client).await.unwrap();
    let resumed = minutes_after(&client, &t0, 20).await;
    client.execute("UPDATE core.queue_control SET changed_at = $1::text::timestamptz", &[&resumed]).await.unwrap();
    assert_eq!(check_after(&mut client, &s, &t0, 30).await, opened(&[]), "10 minutes since the resume, not 30");
    let since: String = client.query_one("SELECT to_char(changed_at AT TIME ZONE 'America/Edmonton', 'Dy HH24:MI') FROM core.queue_control", &[]).await.unwrap().get(0);
    let stalled = format!("Tape jobs stalled since {since} MT: 2 claimable and none leased, so no `tape` pod is running");
    assert_eq!(check_after(&mut client, &s, &t0, 35).await, opened(&[&stalled]));
    // a live lease: resolved, the claimability view not read
    work::claim(&mut client, Kind::Tape, &b, "tape-1", 3600.0).await.unwrap().unwrap();
    assert_eq!(check_after(&mut client, &s, &t0, 36).await.resolved, [stalled]);
    scratch.drop().await;
}

/// C-065 (health.sh's "pods that found nothing to claim"): three claims in ten minutes that found
/// nothing while a job no other claim held stayed claimable are one problem; it resolves when no
/// such claim recurs. Audit round 2 of core-watch: claims that found only work other claims were
/// taking keep no miss (6); misses whose job was leased after are none, even with another job of
/// the same kind and build queued since (6); equal counts name the same work every time (16).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn workers_leaving_while_work_waits_are_a_problem() {
    let (scratch, mut client, b) = ready("claims_empty").await;
    build::admit(&client, &b, &json!({"checklist": "watch test"})).await.unwrap();
    let s = settings(None);
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-09", "2025-12-10", 1).await.unwrap();
    // both jobs held as claims in flight hold them: no miss at all
    let mut holder = scratch.client().await;
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' FOR UPDATE", &[]).await.unwrap();
    for pod in ["tape-a", "tape-b", "tape-c"] {
        assert!(work::claim(&mut client, Kind::Tape, &b, pod, 3600.0).await.unwrap().is_none());
    }
    let t0 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[]), "work others were taking is no miss");
    drop(tx);
    // both jobs held by a lock no claim takes, which still stops a claim: claims free of others
    // that could not lease them
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' FOR KEY SHARE", &[]).await.unwrap();
    for pod in ["tape-1", "tape-2"] {
        assert!(work::claim(&mut client, Kind::Tape, &b, pod, 3600.0).await.unwrap().is_none());
    }
    let t0 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[]), "two are not yet a problem");
    assert!(work::claim(&mut client, Kind::Tape, &b, "tape-3", 3600.0).await.unwrap().is_none());
    let t0 = watch::now(&client).await.unwrap();
    let short = |b: &str| if b.chars().count() > 16 { format!("{}…", b.chars().take(12).collect::<String>()) } else { b.to_string() };
    let summary = format!("Workers left with nothing while work waited: 3 claims found nothing in the last 10 min, 2 tape jobs of build {} still claimable", short(&b));
    assert_eq!(check(&mut client, &s, &t0).await, opened(&[&summary]));
    drop(tx);
    // ten minutes without one: resolved
    assert_eq!(check_after(&mut client, &s, &t0, 11).await.resolved, [summary]);

    // misses whose job was leased a moment later: not a problem, though another tape job of the
    // build was queued since
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' FOR KEY SHARE", &[]).await.unwrap();
    for pod in ["tape-4", "tape-5", "tape-6"] {
        assert!(work::claim(&mut client, Kind::Tape, &b, pod, 3600.0).await.unwrap().is_none());
    }
    drop(tx);
    work::claim(&mut client, Kind::Tape, &b, "tape-7", 3600.0).await.unwrap().unwrap();
    work::claim(&mut client, Kind::Tape, &b, "tape-8", 3600.0).await.unwrap().unwrap();
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-11", "2025-12-11", 1).await.unwrap();
    let t1 = watch::now(&client).await.unwrap();
    assert_eq!(check(&mut client, &s, &t1).await, opened(&[]), "the job queued since is not the work they missed");

    // the second build's tape jobs missed as often as the first's: the summary names the same
    // build every time (the first by kind, then build)
    let other: String = client.query_one("SELECT engine_build FROM core.engine_build WHERE engine_build <> $1", &[&b]).await.unwrap().get(0);
    build::admit(&client, &other, &json!({"checklist": "watch test"})).await.unwrap();
    work::schedule(&mut client, Kind::Tape, PROFILE, &other, "2025-12-12", "2025-12-12", 1).await.unwrap();
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' AND state = 'QUEUED' FOR KEY SHARE", &[]).await.unwrap();
    for pod in ["tape-9", "tape-10"] {
        assert!(work::claim(&mut client, Kind::Tape, &b, pod, 3600.0).await.unwrap().is_none());
        assert!(work::claim(&mut client, Kind::Tape, &other, pod, 3600.0).await.unwrap().is_none());
    }
    let t2 = watch::now(&client).await.unwrap();
    let first = if b < other { &b } else { &other };
    let named = format!("Workers left with nothing while work waited: 4 claims found nothing in the last 10 min, 1 tape jobs of build {} still claimable", short(first));
    assert_eq!(check(&mut client, &s, &t2).await, opened(&[&named]));
    drop(tx);
    let t1 = minutes_after(&client, &t2, 11).await;
    check(&mut client, &s, &t1).await;
    // a week later the misses are gone
    let week = minutes_after(&client, &t1, 7 * 24 * 60 + 1).await;
    check(&mut client, &s, &week).await;
    assert_eq!(client.query_one("SELECT count(*) FROM core.claim_miss", &[]).await.unwrap().get::<_, i64>(0), 0);
    scratch.drop().await;
}

/// C-071 (triage 2026-11-23 r2, C4 = G1, BLOCKS the ordered passes): the passes publish a history
/// revision for each of phase 1's 597 sessions (2024-07-09..2026-11-20) within hours, and the 09:00
/// digest counts every revision of the last 24 hours not counted yet. It counts them in batches of
/// `TALLY_BATCH`, each committed, for at most `TALLY_BUDGET` a minute, so no statement grows with
/// the passes, and a core killed mid-count (here: its connection terminated while a batch waits)
/// loses only that batch: the next minute goes on from there and counts each revision once.
///
/// The slice's real base and derived revisions of 2025-12-10 are published, the engine's real
/// faulted XLB body added to the derived one; each other session has a bare base header and a
/// history revision holding the derived revision's three bodies re-dated to its session (every
/// "2025-12-10" in a body replaced by the session's date: a stated edit, so each body's own
/// engine-fault result id names its day as the engine writes it).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_digest_counts_the_ordered_passes_in_committed_batches_and_survives_a_kill() {
    let (scratch, mut client, _) = ready("tally_batches").await;
    client.query_one("SELECT vyreon.ensure_months('2024-07-01', '2026-11-30')", &[]).await.unwrap();
    let s = settings(None);
    let mut revision = Vec::new();
    for (name, (h, sc, f)) in [("tally-base", parts(BASE)), ("tally-derived", parts(DERIVED))] {
        let path = bundle(name, &h, &sc, &f);
        revision.push(core_db::publish::publish_file_vouched(&mut client, &path).await.unwrap().revision_id);
        std::fs::remove_file(&path).unwrap();
    }
    let (base, derived) = (revision[0], revision[1]);
    let body = ENGINE_FAULT_BODY_XLB.trim_end();
    let body_json: Value = serde_json::from_str(body).unwrap();
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             VALUES ($1, 'SAI-EXQ01', 'XLB', '2025-12-10', 'DELIVERED', 'engine-fault', 'engine-fault', $2, $3, $4::text::jsonb, '{}')",
            &[&derived, &core_contract::sha256_hex(ENGINE_FAULT_BODY_XLB.as_bytes()), &body, &body_json.to_string()],
        )
        .await
        .unwrap();
    // Friday 2026-11-20 09:30 MT: the passes published over the last 15 hours, the bases days before
    let now = "2026-11-20 09:30:00-07";
    client.execute("UPDATE vyreon.day_revision SET published_at = '2026-11-20 09:00:00-07'", &[]).await.unwrap();
    client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members)
             SELECT s.session, b.profile_id, b.profile_revision, b.engine_build, b.snapshot_cutoff, 'PUBLISHED', '2026-11-17 20:00:00-07', b.listed_members
             FROM core.session s, vyreon.day_revision b
             WHERE b.revision_id = $1 AND s.session BETWEEN '2024-07-09' AND '2026-11-20' AND s.session <> '2025-12-10' ORDER BY s.session",
            &[&base],
        )
        .await
        .unwrap();
    let passes = client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members, base_revision)
             SELECT b.represented_date, h.profile_id, h.profile_revision, h.engine_build, h.snapshot_cutoff, 'PUBLISHED',
                    $2::text::timestamptz - make_interval(secs => 90 * row_number() OVER (ORDER BY b.represented_date DESC)), h.listed_members, b.revision_id
             FROM vyreon.day_revision b, vyreon.day_revision h
             WHERE h.revision_id = $1 AND b.base_revision IS NULL AND b.represented_date <> '2025-12-10' ORDER BY b.represented_date",
            &[&derived, &now],
        )
        .await
        .unwrap();
    assert_eq!(passes, 596, "phase 1's sessions less the slice's own");
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             SELECT d.revision_id, m.measurement_id, m.object_id, d.represented_date, m.slot_state, m.science_version, m.method_version,
                    encode(sha256(convert_to(replace(m.body_text, '2025-12-10', d.represented_date::text) || E'\n', 'UTF8')), 'hex'),
                    replace(m.body_text, '2025-12-10', d.represented_date::text), replace(m.body::text, '2025-12-10', d.represented_date::text)::jsonb, m.receipt
             FROM vyreon.day_revision d JOIN vyreon.measurement m ON m.revision_id = $1
             WHERE d.base_revision IS NOT NULL AND d.revision_id <> $1",
            &[&derived],
        )
        .await
        .unwrap();
    let fresh: Vec<i64> = client
        .query("SELECT revision_id FROM vyreon.day_revision WHERE published_at > $1::text::timestamptz - interval '24 hours' ORDER BY revision_id", &[&now])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(fresh.len(), 598, "596 passes and the slice's two revisions");
    let tallied = async |c: &Client| c.query_one("SELECT count(*) FROM core.revision_tally", &[]).await.unwrap().get::<_, i64>(0);

    // a core killed mid-count: its third batch waits on a revision another transaction holds, and
    // its connection is terminated there; the two batches before stay counted, the third is gone
    let held = fresh[2 * watch::TALLY_BATCH + 3];
    let mut other = scratch.client().await;
    let tx = other.transaction().await.unwrap();
    tx.execute("INSERT INTO core.revision_tally (revision_id, slots, withheld, provider_withheld, engine_faults) VALUES ($1, 0, 0, 0, 0)", &[&held]).await.unwrap();
    let killed = scratch.client().await;
    let pid: i32 = killed.query_one("SELECT pg_backend_pid()", &[]).await.unwrap().get(0);
    let counting = tokio::spawn(async move { watch::tally_recent(&killed, PROFILE, now, None).await });
    let waits = async || {
        client
            .query_one("SELECT count(*) FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock'", &[&pid])
            .await
            .unwrap()
            .get::<_, i64>(0)
    };
    for _ in 0..600 {
        if waits().await == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(waits().await, 1, "the third batch waits on the held revision");
    assert!(client.query_one("SELECT pg_terminate_backend($1)", &[&pid]).await.unwrap().get::<_, bool>(0));
    assert!(counting.await.unwrap().is_err(), "the killed count fails");
    tx.rollback().await.unwrap();
    assert_eq!(tallied(&client).await, 2 * watch::TALLY_BATCH as i64, "two batches committed, the killed one rolled back");
    let first: Vec<(i64, String)> = client
        .query("SELECT revision_id, counted_at::text FROM core.revision_tally ORDER BY 1", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(first.iter().map(|(r, _)| *r).collect::<Vec<_>>(), fresh[..2 * watch::TALLY_BATCH]);

    // a minute whose budget is spent counts one more batch and composes nothing yet
    assert!(!watch::tally_recent(&client, PROFILE, now, Some(std::time::Instant::now())).await.unwrap());
    assert_eq!(tallied(&client).await, 3 * watch::TALLY_BATCH as i64);

    // the 09:30 minute goes on from there, counts the rest and queues the digest
    watch::tick(&mut client, &s, now).await.unwrap();
    let digest: String = client.query_one("SELECT message FROM core.notice WHERE once = 'digest 2026-11-20'", &[]).await.unwrap().get(0);
    let counted = client
        .query(
            "SELECT d.revision_id, d.base_revision IS NULL, t.slots, t.withheld, t.provider_withheld, t.engine_faults, t.counted_at::text
             FROM vyreon.day_revision d JOIN core.revision_tally t USING (revision_id) WHERE d.revision_id = ANY($1) ORDER BY 1",
            &[&fresh],
        )
        .await
        .unwrap();
    assert_eq!(counted.len(), 598, "every revision of the last 24 hours counted, each once");
    for r in &counted {
        let (id, is_base): (i64, bool) = (r.get(0), r.get(1));
        let t: (i32, i32, i32, i32) = (r.get(2), r.get(3), r.get(4), r.get(5));
        // the base: four delivered slots; each history revision: two delivered and the engine's
        // faulted slot, withheld whole for its engine fault (never the held row's zeros)
        assert_eq!(t, if is_base { (4, 0, 0, 0) } else { (3, 1, 0, 1) }, "revision {id}");
    }
    // what the killed count committed was not counted again
    let kept: Vec<(i64, String)> = counted.iter().filter(|r| first.iter().any(|(id, _)| *id == r.get::<_, i64>(0))).map(|r| (r.get(0), r.get(6))).collect();
    assert_eq!(kept, first);
    let lines: Vec<&str> = digest.lines().collect();
    assert_eq!(lines[1], "Latest session stored: Fri Nov 20 · 66.7% of its measurements have a value (typical 66.7%) · 1 engine fault", "{digest}");
    assert_eq!(lines[2], "Sessions published in the last 24 hours: 1 (2025-12-10)", "{digest}");
    assert_eq!(lines[3], "Provider gaps recorded in the last 24 hours: none", "{digest}");
    scratch.drop().await;
}
