//! C-073 (owner, 2026-11-24: both Discord channels readable at a glance): the progress post's
//! shapes (a quiet half hour, a busy one, a failure, a hand-over) and the alert's (what happened,
//! what it means for the numbers, what to do), on the real slice of 2025-12-10 and the engine's
//! calendar. Run with `cargo test -- --ignored --nocapture` to see each rendered.
use core_db::publish::publish_for_lease;
use core_db::watch::{self, Settings};
use core_db::work::{self, Claim, Kind};
use core_db::{build, migrate, progress, universe};
use serde_json::json;
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, reading, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const LABEL: &str = "Phase 1";

async fn ready(test: &str) -> (Scratch, Client, String, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut inv = inventories();
    let derived = build::register(&mut client, &reading(inv.remove(1))).await.unwrap().engine_build;
    let base = build::register(&mut client, &with_day_local(inv.remove(0))).await.unwrap().engine_build;
    for b in [&base, &derived] {
        build::admit(&client, b, &json!({"checklist": "digest format"})).await.unwrap();
    }
    (scratch, client, base, derived)
}

async fn publish_day(client: &mut Client, claim: &Claim, build: &str, day: &str) {
    let (mut h, s, f) = parts(BASE);
    h.represented_date = day.into();
    let path = bundle(&format!("df-base-{day}"), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(build, day, base);
    let path = bundle(&format!("df-local-{day}"), &lh, &ls, &lf);
    publish_for_lease(client, &path, &claim.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::day_done(client, &claim.token, day).await.unwrap();
}

async fn last_post(client: &Client) -> String {
    client.query_one("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0)
}

async fn later(client: &Client, now: &str, minutes: i32) -> String {
    client.query_one("SELECT ($1::text::timestamptz + make_interval(mins => $2))::text", &[&now, &minutes]).await.unwrap().get(0)
}

/// A busy half hour, then a quiet one: the headline (days published of the backfill's, jobs
/// building, failures), "+N this half hour" and each new day in one line, the history passes and
/// the daily run in plain words; then one quiet line.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_busy_half_hour_then_a_quiet_one() {
    let (scratch, mut client, b, d) = ready("df_busy").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-12", 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, "2025-12-11", "2025-12-12", 1).await.unwrap();
    // a daily session's block, computing
    client
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, priority)
             VALUES ('DAY_BLOCK', $1, $2, '2026-11-23', '2026-11-23', '2026-11-23', 1)",
            &[&PROFILE, &b],
        )
        .await
        .unwrap();
    let daily = work::claim_as(&mut client, Kind::DayBlock, &b, "day-daily", 24.0 * HOUR, true).await.unwrap().unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "day-7f2c", 24.0 * HOUR).await.unwrap().unwrap();
    assert_eq!(daily.sessions, ["2026-11-23"]);
    let t0: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &t0).await.unwrap();
    // the block's first day comes in (the slice's day, published as 2025-12-10)
    publish_day(&mut client, &block, &b, "2025-12-10").await;
    let t1 = later(&client, &t0, 30).await;
    assert_eq!(progress::tick_as(&mut client, PROFILE, LABEL, &t1).await.unwrap().unwrap().posts, 1);
    let busy = last_post(&client).await;
    eprintln!("BUSY:\n{busy}\n");
    let lines: Vec<&str> = busy.lines().collect();
    // (owner, 2026-11-25) plain words: no mark when fine, "done", "computing", "nothing broken"
    assert_eq!(lines[0], "Phase 1: 1 of 3 days done (33.3%) · 1 computing · nothing broken", "{busy}");
    assert!(lines[1].starts_with("New: Dec 10 done in ") && lines[1].contains("% of readings"), "{busy}");
    assert_eq!(lines[2], "History layer (IV30 etc.): 0 of 2 — waiting for Dec 11 to be rebuilt (they go in date order)");
    assert!(lines[3].starts_with("Nightly (Nov 23): computing since "), "{busy}");
    assert_eq!(lines.len(), 4, "{busy}");
    assert!(!busy.contains("day-7f2c") && !busy.contains("job ") && !busy.contains("2025-"), "no pod, no job id, no ISO date: {busy}");
    // nothing new in the next half hour (before the morning's heartbeat): nothing posted
    let posts = "SELECT count(*) FROM core.notice WHERE channel = 'progress'";
    let before: i64 = client.query_one(posts, &[]).await.unwrap().get(0);
    let t2 = later(&client, &t1, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &t2).await.unwrap();
    let after: i64 = client.query_one(posts, &[]).await.unwrap().get(0);
    let morning: bool = client.query_one("SELECT extract(hour FROM ($1::text::timestamptz AT TIME ZONE 'Etc/GMT+6')) >= 9", &[&t2]).await.unwrap().get(0);
    if !morning {
        assert_eq!(after, before, "a quiet half hour posts nothing");
    }
    // the morning's heartbeat: the first window from 09:00 MT posts the state whatever happened, once a day
    let nine: String = client.query_one("SELECT ((date_trunc('day', $1::text::timestamptz AT TIME ZONE 'Etc/GMT+6') + interval '1 day 9 hours 1 minute') AT TIME ZONE 'Etc/GMT+6')::text", &[&t2]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &nine).await.unwrap();
    let heartbeat = last_post(&client).await;
    assert!(heartbeat.starts_with("Phase 1: 1 of 3 days done") && heartbeat.contains("History layer"), "{heartbeat}");
    let n: i64 = client.query_one(posts, &[]).await.unwrap().get(0);
    let later2 = later(&client, &nine, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &later2).await.unwrap();
    assert_eq!(client.query_one(posts, &[]).await.unwrap().get::<_, i64>(0), n, "one heartbeat a day");
    scratch.drop().await;
}

/// A failure is named and makes the headline red once its alert is open; the alert says what
/// happened, what it means for the numbers and what to do; a retried block is named.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failure_is_named_and_its_alert_says_what_to_do() {
    let (scratch, mut client, b, _) = ready("df_failure").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-10", 30).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 2", &[]).await.unwrap();
    let t0: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &t0).await.unwrap();
    for worker in ["day-a", "day-b"] {
        let claim = work::claim(&mut client, Kind::DayBlock, &b, worker, HOUR).await.unwrap().unwrap();
        work::fail(&mut client, &claim.token, "day 2025-12-09", "stop: 2025-12-09 not published: the macro capture ran past its window", "").await.unwrap();
    }
    let settings = Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None, spend: None, recompute: None,
        refcheck_rerun_hours: None,
    };
    let now = watch::now(&client).await.unwrap();
    watch::check(&mut client, &settings, &now).await.unwrap();
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE post AND channel = 'alerts' AND message LIKE '🔴 Day%' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    eprintln!("ALERT:\n{alert}\n");
    let lines: Vec<&str> = alert.lines().collect();
    // (owner, 2026-11-25) what happened, whether published numbers are affected, what to do; no job id,
    // no ISO date, no code
    assert_eq!(lines[0], "🔴 Day Dec 9 failed for the last time: the macro capture ran past its window", "{alert}");
    assert!(
        lines[1].starts_with("Published numbers affected: that day stays unpublished (readers see it missing, with its cause). To do: after a fix, retry the failed work (`vyreon-core queue retry-failed`); why it failed: `vyreon-core evidence show "),
        "{alert}"
    );
    assert_eq!(lines.len(), 2, "{alert}");
    let t1 = later(&client, &t0, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &t1).await.unwrap();
    let post = {
        let p: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
        p
    };
    eprintln!("FAILURE:\n{post}\n");
    let lines: Vec<&str> = post.lines().collect();
    // (owner, 2026-11-25) 🔴 only with an alert open; the retry a thing to look at; the failure in plain words
    assert_eq!(lines[0], "🔴 Phase 1: 0 of 2 days done (0%) · 0 computing · 1 failed", "{post}");
    assert!(post.contains("⚠ Needs a look: 1 block restarted after a failure (Dec 9)"), "the retry is named: {post}");
    assert!(post.lines().any(|l| l.ends_with("⚠ Dec 9 could not be computed after its tries: set aside, the rest goes on")), "the failure is named: {post}");
    // an alert that needs nothing of the owner says so
    let provider = watch::posted_alert("PROVIDER_ERRORS", "Provider (Intrinio) errors stopped 3 attempts in the last 30 minutes (HTTP 502)", "vyreon-core evidence list");
    assert_eq!(
        provider,
        "🔴 Provider (Intrinio) errors stopped 3 attempts in the last 30 minutes (HTTP 502)\nPublished numbers not affected: the work is retried by itself. Nothing to do: it is handled automatically."
    );
    // a resolution keeps its duration
    assert_eq!(watch::posted_resolved("Work queue paused, nothing is handed out: release", 720), "✅ Resolved: Work queue paused, nothing is handed out: release (it was open 12 min)");
    scratch.drop().await;
}

/// A hand-over reads as one short line per step: the pause, the withdrawal, the hand-over, the
/// admission and the resume.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_hand_over_is_a_few_short_lines() {
    let (scratch, mut client, b, _) = ready("df_handover").await;
    let mut inv = with_day_local(inventories().remove(0));
    inv["engine_build"] = json!("c9c23ca5+aabbccddeeff");
    let new = build::register(&mut client, &inv).await.unwrap().engine_build;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-12", 2).await.unwrap();
    work::claim(&mut client, Kind::DayBlock, &b, "day-a", HOUR).await.unwrap().unwrap();
    let t0: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &t0).await.unwrap();
    let tx = client.transaction().await.unwrap();
    work::pause(&tx, "release c9c23ca5: D-461 rulings").await.unwrap();
    tx.commit().await.unwrap();
    let preview = build::hand_over(&mut client, &b, &new, "D-461 rulings", true, None).await.unwrap();
    build::hand_over(&mut client, &b, &new, "D-461 rulings", false, Some(&preview.token)).await.unwrap();
    build::admit(&client, &new, &json!({"tests": "x", "checks": "y"})).await.unwrap();
    let tx = client.transaction().await.unwrap();
    work::resume(&tx).await.unwrap();
    tx.commit().await.unwrap();
    let t1 = later(&client, &t0, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &t1).await.unwrap();
    let post = last_post(&client).await;
    eprintln!("HAND-OVER:\n{post}\n");
    // (owner, 2026-11-25) each step in plain words, no build, no code, no admission line
    let steps: Vec<&str> = post.lines().skip(1).filter(|l| l.len() > 6 && l.as_bytes()[2] == b':').map(|l| &l[6..]).collect();
    assert!(steps.contains(&"Queue paused: a release is switching versions"), "{post}");
    assert!(steps.contains(&"A release started: 1 block of the old version finishes its current day, then the new version takes over"), "{post}");
    assert!(steps.contains(&"Queue resumed"), "{post}");
    assert!(!post.contains("c9c23ca5") && !post.contains("D-461") && !post.contains("admitted"), "{post}");
    assert!(post.lines().any(|l| l.starts_with("Release: 1 block of the old version finishing its current day")), "{post}");
    let _ = &b;
    assert!(post.lines().all(|l| l.chars().count() < 160), "short lines: {post}");
    scratch.drop().await;
}
