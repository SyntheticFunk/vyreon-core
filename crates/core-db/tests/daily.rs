//! Daily runs (C-051) on the engine's real calendar at engine-v1: a session is queued the morning
//! after (07:30 New York, winter and summer), a weekend and a holiday are never sessions, nothing
//! before the start setting is queued, a session already queued by hand or published is not queued
//! again, a session is never queued twice (a cancelled daily job stays cancelled), a daily job is
//! claimed before older backfill work, and a build that cannot be given work queues nothing and is
//! a problem. Run with `cargo test -- --ignored`.
use core_db::daily::{self, Daily, Queued};
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind};
use core_db::{build, migrate, universe};
use serde_json::json;
use test_support::Scratch;
use test_support::slice::{inventories, listed_2025_12_10};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";

/// A migrated database with the calendar, the universe and the slice's builds (base admitted);
/// returns (base, derived).
async fn ready(test: &str) -> (Scratch, Client, String, String) {
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
    build::admit(&client, &builds[0], &json!({"checklist": "daily test"})).await.unwrap();
    let derived = builds.pop().unwrap();
    (scratch, client, builds.pop().unwrap(), derived)
}

fn daily(build: &str, from: &str) -> Daily {
    Daily { profile: PROFILE.into(), build: build.into(), from: from.into() }
}

async fn queue(client: &mut Client, d: &Daily, now: &str) -> Vec<String> {
    daily::queue_due(client, d, now).await.unwrap().into_iter().map(|q: Queued| q.session).collect()
}

/// The kinds and priorities of the jobs covering `session`, in job order.
async fn jobs_of(client: &Client, session: &str) -> Vec<(String, i16, String)> {
    client
        .query(
            "SELECT kind, priority, state FROM core.job WHERE $1::text::date BETWEEN first_session AND last_session ORDER BY job_id",
            &[&session],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
}

fn daily_jobs() -> Vec<(String, i16, String)> {
    ["TAPE", "DAY_BLOCK", "ORDERED_PASS"].iter().map(|k| (k.to_string(), 1, "QUEUED".to_string())).collect()
}

/// The slice's base revision header as the published revision of `day` (as the watch's test).
async fn published(client: &Client, build: &str, day: &str) {
    client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members)
             VALUES ($1::text::date, $2, 'VYREON-RESEARCH-PROFILE-S64-3.0.0', $3, '2026-11-08T13:49:56.940739Z', 'PUBLISHED', now(), $4)",
            &[&day, &PROFILE, &build, &listed_2025_12_10()],
        )
        .await
        .unwrap();
}

/// December 2025 in winter time (New York UTC−5: 07:30 is 12:30 UTC): the morning after, never
/// before; a weekend; Christmas; the start setting; queued once.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_session_is_queued_the_morning_after_and_only_once() {
    let (scratch, mut client, b, _) = ready("daily_calendar").await;
    let d = daily(&b, "2025-12-10");
    // Wednesday the 10th is due Thursday 07:30 New York; the 9th is before the start
    assert_eq!(queue(&mut client, &d, "2025-12-11 12:29:59+00").await, Vec::<String>::new());
    assert_eq!(queue(&mut client, &d, "2025-12-11 12:30:00+00").await, ["2025-12-10"]);
    assert_eq!(jobs_of(&client, "2025-12-10").await, daily_jobs());
    assert_eq!(jobs_of(&client, "2025-12-09").await, []);
    // the same minute again, and a later one: nothing more
    assert_eq!(queue(&mut client, &d, "2025-12-11 12:30:00+00").await, Vec::<String>::new());
    assert_eq!(queue(&mut client, &d, "2025-12-11 23:00:00+00").await, Vec::<String>::new());
    // Friday the 12th is due Saturday; on Monday morning nothing of the weekend is queued, and
    // Monday's own session waits for Tuesday
    assert_eq!(queue(&mut client, &d, "2025-12-15 12:30:00+00").await, ["2025-12-11", "2025-12-12"]);
    assert_eq!(jobs_of(&client, "2025-12-13").await, []);
    // Christmas is no session: the 24th is due on the 25th, the 26th on the 27th (a Saturday)
    let n: i64 = client.query_one("SELECT count(*) FROM core.session WHERE session = '2025-12-25'", &[]).await.unwrap().get(0);
    assert_eq!(n, 0, "the engine's calendar has no 2025-12-25");
    let queued = queue(&mut client, &d, "2025-12-26 12:30:00+00").await;
    assert_eq!(queued.first().map(String::as_str), Some("2025-12-15"));
    assert_eq!(queued.last().map(String::as_str), Some("2025-12-24"));
    assert_eq!(queue(&mut client, &d, "2025-12-27 12:29:00+00").await, Vec::<String>::new());
    assert_eq!(queue(&mut client, &d, "2025-12-27 12:30:00+00").await, ["2025-12-26"]);

    // a daily job the operator cancels stays cancelled: the session is never queued again
    let jobs: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE first_session = '2025-12-26'", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    work::cancel(&mut client, &jobs).await.unwrap();
    assert_eq!(queue(&mut client, &d, "2025-12-29 12:00:00+00").await, Vec::<String>::new());
    assert_eq!(jobs_of(&client, "2025-12-26").await.iter().filter(|j| j.2 == "CANCELLED").count(), 3);
    let handled: i64 = client.query_one("SELECT count(*) FROM core.daily_session", &[]).await.unwrap().get(0);
    let sessions: i64 = client.query_one("SELECT count(*) FROM core.session WHERE session BETWEEN '2025-12-10' AND '2025-12-26'", &[]).await.unwrap().get(0);
    assert_eq!(handled, sessions, "every session from the start, once");

    // the log says what was queued, never posted
    let logged: i64 = client.query_one("SELECT count(*) FROM core.notice WHERE message LIKE 'Daily run: session %' AND NOT post", &[]).await.unwrap().get(0);
    assert_eq!(logged, sessions);
    scratch.drop().await;
}

/// July 2025 in summer time (New York UTC−4: 07:30 is 11:30 UTC); Independence Day.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn summer_time_and_a_holiday_follow_new_york() {
    let (scratch, mut client, b, _) = ready("daily_summer").await;
    let d = daily(&b, "2025-07-03");
    assert_eq!(queue(&mut client, &d, "2025-07-04 11:29:59+00").await, Vec::<String>::new());
    assert_eq!(queue(&mut client, &d, "2025-07-04 11:30:00+00").await, ["2025-07-03"]);
    // the 4th is no session; Monday the 7th is due Tuesday
    assert_eq!(queue(&mut client, &d, "2025-07-08 11:29:00+00").await, Vec::<String>::new());
    assert_eq!(queue(&mut client, &d, "2025-07-08 11:30:00+00").await, ["2025-07-07"]);
    scratch.drop().await;
}

/// A session already queued by hand gets only the kinds it lacks; one already published gets
/// nothing; a daily job is claimed before the backfill's older work of its kind, and a daily-only
/// claim takes nothing else.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn work_queued_or_published_by_hand_is_not_queued_again() {
    let (scratch, mut client, b, _) = ready("daily_by_hand").await;
    let d = daily(&b, "2025-12-15");
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-15", "2025-12-15", 1).await.unwrap();
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    published(&client, &b, "2025-12-16").await;
    assert_eq!(queue(&mut client, &d, "2025-12-17 12:30:00+00").await, ["2025-12-15", "2025-12-16"]);
    assert_eq!(jobs_of(&client, "2025-12-15").await, [
        ("DAY_BLOCK".to_string(), 0, "QUEUED".to_string()),
        ("TAPE".to_string(), 1, "QUEUED".to_string()),
        ("ORDERED_PASS".to_string(), 1, "QUEUED".to_string())
    ]);
    assert_eq!(jobs_of(&client, "2025-12-16").await, []);
    let jobs: Vec<Vec<i64>> = client.query("SELECT jobs FROM core.daily_session ORDER BY session", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!((jobs[0].len(), jobs[1].len()), (2, 0));

    // the backfill's older tapes wait while the daily session's tape is claimed first
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-01", "2025-12-05", 1).await.unwrap();
    let claim = work::claim_as(&mut client, Kind::Tape, &b, "w1", 3600.0, true).await.unwrap().unwrap();
    assert_eq!(claim.sessions, ["2025-12-15"]);
    // round x, 13: the daily CronJob's worker takes only daily work, never the backfill's tapes
    assert!(work::claim_as(&mut client, Kind::Tape, &b, "daily", 3600.0, true).await.unwrap().is_none());
    let next = work::claim(&mut client, Kind::Tape, &b, "w2", 3600.0).await.unwrap().unwrap();
    assert_eq!(next.sessions, ["2025-12-01"], "then the earliest of the rest, as before");
    scratch.drop().await;
}

/// A build that is not admitted (or was withdrawn) queues nothing, and is one problem until it can.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_build_that_cannot_work_queues_nothing_and_is_a_problem() {
    let (scratch, mut client, _, derived) = ready("daily_stopped").await;
    let d = daily(&derived, "2025-12-10");
    let refused = daily::queue_due(&mut client, &d, "2025-12-11 12:30:00+00").await.unwrap_err();
    assert_eq!(refused, format!("daily runs: build {derived} is not admitted; 1 due sessions are not queued"));
    let n: i64 = client.query_one("SELECT count(*) FROM core.job", &[]).await.unwrap().get(0);
    assert_eq!(n, 0);
    let s = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: Some(d.clone()), archive: None, refcheck_from: None, backup_days: None };
    let now = "2025-12-11 12:31:00+00";
    let stopped = format!("Daily runs stopped: build {derived} is not admitted, so no new session is queued");
    assert_eq!(watch::check(&mut client, &s, now).await.unwrap().opened, std::slice::from_ref(&stopped));
    // the minute's tick reports the refusal and still checks
    assert!(watch::tick(&mut client, &s, now).await.is_err());
    build::admit(&client, &derived, &json!({"checklist": "daily test"})).await.unwrap();
    let change = watch::tick(&mut client, &s, "2025-12-11 12:32:00+00").await.unwrap();
    assert_eq!(change.resolved, [stopped]);
    assert_eq!(jobs_of(&client, "2025-12-10").await, daily_jobs());
    // withdrawn: stopped again
    build::withdraw(&mut client, &derived, "daily test").await.unwrap();
    assert_eq!(watch::check(&mut client, &s, "2025-12-11 12:33:00+00").await.unwrap().opened.len(), 1);
    scratch.drop().await;
}

/// Audit round x, 11: a tick whose queueing failed (the store's partitions could not be made, the
/// build admitted) never queues a green digest: the session due and not queued is a problem first,
/// and the digest says it; once queueing works the problem resolves.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failed_queueing_is_a_problem_before_the_digest() {
    let (scratch, mut client, b, _) = ready("daily_queue_failed").await;
    let d = daily(&b, "2025-12-10");
    let s = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: Some(d), archive: None, refcheck_from: None, backup_days: None };
    client
        .batch_execute(
            "ALTER FUNCTION vyreon.ensure_months(date, date) RENAME TO ensure_months_kept;
             CREATE FUNCTION vyreon.ensure_months(first date, last date) RETURNS integer LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'partitions refused (test)'; END $$;",
        )
        .await
        .unwrap();
    // 09:00 Mountain on 2025-12-11 (UTC−7): the digest's minute
    let nine = "2025-12-11 16:00:00+00";
    let why = watch::tick(&mut client, &s, nine).await.unwrap_err();
    assert!(why.contains("partitions refused (test)"), "{why}");
    let stopped = "Daily runs stopped: a session due is not queued (the oldest 2025-12-10): queueing failed, the service's log says why";
    let open: Vec<String> = watch::open(&client, nine).await.unwrap().into_iter().map(|o| o.summary).collect();
    assert_eq!(open, [stopped]);
    let digest: String = client.query_one("SELECT message FROM core.notice WHERE once = 'digest 2025-12-11'", &[]).await.unwrap().get(0);
    assert!(digest.starts_with(&format!("🔴 Needs you: {stopped}")), "{digest}");
    // queueing works again: the session is queued and the problem resolves
    client.batch_execute("DROP FUNCTION vyreon.ensure_months(date, date); ALTER FUNCTION vyreon.ensure_months_kept(date, date) RENAME TO ensure_months;").await.unwrap();
    let change = watch::tick(&mut client, &s, "2025-12-11 16:01:00+00").await.unwrap();
    assert_eq!(change.resolved, [stopped]);
    assert_eq!(jobs_of(&client, "2025-12-10").await, daily_jobs());
    scratch.drop().await;
}

/// The deadline (C-050, at C-051's default hour) never falls before a session's inputs are
/// ready: over every session of the calendar, winter and summer, it is at least two measured
/// cluster days (5 h 50 min each) and a tape job after the session is queued.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_deadline_leaves_two_days_of_work_after_the_inputs_are_ready() {
    let (scratch, client, _, _) = ready("daily_deadline").await;
    let row = client
        .query_one(
            "SELECT min(deadline - ready)::text, max(deadline - ready)::text FROM (
                 SELECT ((session + 1) + make_time($1, 0, 0)) AT TIME ZONE 'America/Edmonton' AS deadline,
                        ((session + 1) + $2::text::time) AT TIME ZONE $3 AS ready
                 FROM core.session) x",
            &[&watch::DEADLINE_HOUR, &daily::READY_AT, &daily::READY_ZONE],
        )
        .await
        .unwrap();
    let (min, max): (String, String) = (row.get(0), row.get(1));
    // 14 h 30 min while Mountain time is two hours behind New York; 13 h 30 min in New York's
    // winters from November 2026, when Alberta stays on UTC−6 (the zone database PostgreSQL reads)
    assert_eq!((min.as_str(), max.as_str()), ("13:30:00", "14:30:00"));
    scratch.drop().await;
}
