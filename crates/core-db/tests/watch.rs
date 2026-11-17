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
use test_support::slice::{BASE, bundle, inventories, listed_2025_12_10, parts};
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
    Settings { profile: PROFILE.into(), deadlines_from: from.map(str::to_owned), volume_bytes: None }
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
    watch::Change { opened: v.iter().map(|s| s.to_string()).collect(), resolved: vec![] }
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
    let in_an_hour = later_text(&client, 1).await;
    assert_eq!(check(&mut client, &s, &in_an_hour).await, watch::Change::default(), "still open within the quiet hours");
    let after_three = later_text(&client, 3).await;
    assert_eq!(check(&mut client, &s, &after_three).await.resolved.len(), 1);
    // the three were retried, not failed: handled, in the digest
    let digest = watch::digest(&client, &s, &after_three).await.unwrap();
    assert!(digest.starts_with("🟡 Running; handled: 3 retried attempts · "), "{digest}");
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
    assert!(yellow.contains("\nAttempts retried in the last 24 hours: 1\nWork queue: day blocks 1 waiting\nBackfill progress: 0 of 1 sessions done · expected to finish about "), "{yellow}");

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
    assert_eq!(lines[0], "🟡 Weekly check: this week's sessions have no publishing deadline yet, so Sunday's report is not checked");
    assert_eq!(lines[1..6], [
        "Mon Dec 8: not published",
        "Tue Dec 9: published · 100.0% of its measurements have a value · 0 engine faults",
        "Wed Dec 10: published · 80.0% of its measurements have a value · 1 engine fault",
        "Thu Dec 11: not published",
        "Fri Dec 12: not published"
    ]);
    let watched = watch::weekly(&client, &settings(Some("2025-12-08")), "2025-12-13 09:00:00-07").await.unwrap();
    assert!(watched.starts_with("🔴 Weekly check: Sunday's report at risk, 3 of 5 sessions not published (Mon Dec 8, Thu Dec 11, Fri Dec 12)\n"), "{watched}");
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
