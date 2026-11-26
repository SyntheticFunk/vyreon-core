//! C-073, C-091 (owner, 2026-11-24 and 2026-11-26: both Discord channels readable at a glance): the progress
//! post's shapes (a block started, a block finished, the morning's database line; nothing for a failure or a
//! release) and the alert's (a problem with its cause and effect, what needs the owner, the three-line digest), on
//! the real slice of 2025-12-10 and the engine's calendar. Run with `cargo test -- --ignored --nocapture` to see
//! each rendered.
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

/// C-091 (owner, 2026-11-26: "block queued, block finished, database progress"): the progress channel says a block
/// started (the nightly's apart), a block finished with the days it kept, and on the morning's heartbeat the
/// database's progress; a quiet half hour posts nothing; nothing else (no day lines, no history layer, no nightly
/// status, no "nothing broken").
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn progress_says_blocks_started_and_finished_and_the_databases_progress() {
    let (scratch, mut client, b, d) = ready("df_busy").await;
    // the current build is the slice's base build alone
    build::withdraw(&mut client, &d, "not this test's").await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-12", 30).await.unwrap();
    // a daily session's block
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
    // the morning's heartbeat (from 09:00 MT on the real clock) leads with the database's line
    let events = |post: String| post.lines().filter(|l| !l.starts_with("Database: ")).collect::<Vec<_>>().join("\n");
    let started = last_post(&client).await;
    eprintln!("STARTED:\n{started}\n");
    assert_eq!(events(started), "Nightly started: Nov 23, 2026\nStarted: Dec 10 – Dec 12, 2025 (3 days)\nRunning now: 1 block (Dec 10 – Dec 12, 2025) · the nightly (Nov 23, 2026)");
    // the block's days come in and it finishes
    for day in ["2025-12-10", "2025-12-11", "2025-12-12"] {
        publish_day(&mut client, &block, &b, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    let t1 = later(&client, &t0, 30).await;
    assert_eq!(progress::tick_as(&mut client, PROFILE, LABEL, &t1).await.unwrap().unwrap().posts, 1);
    let finished = last_post(&client).await;
    eprintln!("FINISHED:\n{finished}\n");
    assert_eq!(events(finished), "Finished: Dec 10 – Dec 12, 2025 · 3 days kept\nRunning now: the nightly (Nov 23, 2026)");
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
    // the morning's heartbeat: the database's progress, once a day
    let nine: String = client.query_one("SELECT ((date_trunc('day', $1::text::timestamptz AT TIME ZONE 'Etc/GMT+6') + interval '1 day 9 hours 1 minute') AT TIME ZONE 'Etc/GMT+6')::text", &[&t2]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &nine).await.unwrap();
    let heartbeat = last_post(&client).await;
    eprintln!("HEARTBEAT:\n{heartbeat}\n");
    // C-093: the nightly's 24-hour lease may or may not outlive the real clock's next 09:00
    assert!(heartbeat.starts_with("Database: 3 of 3 days kept (current build) · done\nRunning now: ") && heartbeat.lines().count() == 2, "{heartbeat}");
    let n: i64 = client.query_one(posts, &[]).await.unwrap().get(0);
    let later2 = later(&client, &nine, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &later2).await.unwrap();
    assert_eq!(client.query_one(posts, &[]).await.unwrap().get::<_, i64>(0), n, "one heartbeat a day");
    let all: Vec<String> = client.query("SELECT message FROM core.notice WHERE channel = 'progress'", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(all.iter().all(|p| !p.contains("nothing broken") && !p.contains("History") && !p.contains("New:") && !p.contains("day-7f2c")), "{all:?}");

    // the phase's total, and the finish at the build's measured rate once it has kept 3 days over 2 hours
    let phase = ("2025-12-01".to_owned(), "2025-12-31".to_owned());
    let total: i64 = client.query_one("SELECT count(*) FROM core.session WHERE session BETWEEN '2025-12-01' AND '2025-12-31'", &[]).await.unwrap().get(0);
    let now: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    assert_eq!(progress::database_line(&client, PROFILE, Some(&phase), &now).await.unwrap(), format!("Database: 3 of {total} days kept (current build)"), "not measurable yet: no date");
    client
        .execute(
            "UPDATE vyreon.day_revision SET published_at = now() - make_interval(hours => (4 - extract(day FROM represented_date)::int % 10))
             WHERE day_local",
            &[],
        )
        .await
        .unwrap();
    let finish: String = client
        .query_one("SELECT to_char((now() + make_interval(secs => ($1::bigint - 3) * 3600.0 / (3 / 4.0))) AT TIME ZONE 'Etc/GMT+6', 'Mon FMDD')", &[&total])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        progress::database_line(&client, PROFILE, Some(&phase), &now).await.unwrap(),
        format!("Database: 3 of {total} days kept (current build) · finish ~{finish} at the measured rate")
    );
    scratch.drop().await;
}

/// C-091 (owner, 2026-11-26): a failure is the alerts channel's, as a ⚠️ Problem with its cause, its effect on the
/// published numbers and "the coordinator is on it", no command; the progress channel says no failure. The owner's
/// own actions are 🔴 "Needs you" in one line; a resolution keeps its duration; the 09:00 digest is three lines.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failure_is_a_problem_with_its_cause_and_the_digest_is_three_lines() {
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
        refcheck_rerun_hours: None, phase: None,
    };
    let now = watch::now(&client).await.unwrap();
    watch::check(&mut client, &settings, &now).await.unwrap();
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE post AND channel = 'alerts' AND message LIKE '⚠️ Problem: Day%' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    eprintln!("ALERT:\n{alert}\n");
    assert_eq!(
        alert,
        "⚠️ Problem: Day Dec 9 failed for the last time: the macro capture ran past its window.\nPublished numbers affected: that day stays unpublished (readers see it missing, with its cause). The coordinator is on it."
    );
    let t1 = later(&client, &t0, 30).await;
    progress::tick_as(&mut client, PROFILE, LABEL, &t1).await.unwrap();
    let progress: Vec<String> = client.query("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    eprintln!("PROGRESS:\n{}\n", progress.join("\n"));
    assert!(progress.iter().all(|p| !p.contains("fail") && !p.contains('⚠') && !p.contains('🔴')), "{progress:?}");
    assert!(progress.iter().any(|p| p.contains("Started again: Dec 9 – Dec 10, 2025 (2 days left)")), "{progress:?}");
    // handled by itself, or the coordinator's: a problem; the owner's own action: one line
    assert_eq!(
        watch::posted_alert("PROVIDER_ERRORS", "Provider (Intrinio) errors stopped 3 attempts in the last 30 minutes (HTTP 502)", "vyreon-core evidence list"),
        "⚠️ Problem: Provider (Intrinio) errors stopped 3 attempts in the last 30 minutes (HTTP 502).\nPublished numbers not affected: the work is retried by itself. The coordinator is on it."
    );
    assert_eq!(
        watch::posted_alert("REFCHECK_DISAGREES", "Reference check disagrees with 7 published days: 404 comparisons", "vyreon-core refcheck list"),
        "⚠️ Problem: Reference check disagrees with 7 published days: 404 comparisons.\nPublished numbers affected: numbers may be wrong where the independent check disagrees; they stay published until judged. The coordinator is on it."
    );
    assert_eq!(
        watch::posted_alert("SPEND", "Spend since Nov 21 is $412.00, 82% of the $500.00 ceiling", "vyreon-core spend"),
        "🔴 Needs you: Spend since Nov 21 is $412.00, 82% of the $500.00 ceiling: decide whether to go on: raise the ceiling, or stop the workers."
    );
    assert_eq!(watch::posted_resolved("Work queue paused, nothing is handed out: release", 720), "✅ Resolved: Work queue paused, nothing is handed out: release (it was open 12 min)");
    // the 09:00 digest: verdict, database, what needs the owner
    let open = watch::open(&client, &now).await.unwrap();
    let digest = watch::digest_post(&mut client, &settings, &now, &open).await.unwrap();
    eprintln!("DIGEST:\n{digest}\n");
    let lines: Vec<&str> = digest.lines().collect();
    assert_eq!(lines.len(), 3, "{digest}");
    assert!(lines[0].starts_with("⚠️ 1 problem open, the coordinator is on it · "), "{digest}");
    assert_eq!(lines[1], "Database: 0 of 2 days kept (current build)");
    assert_eq!(lines[2], "Nothing needs you");
    assert_eq!(
        watch::digest_lines("Thu Nov 26", &["the spend is not known (the billing token is missing), so the ceiling is not watched".into()], 2, "Database: 20 of 597 days kept (current build)"),
        "🔴 Needs you · Thu Nov 26\nDatabase: 20 of 597 days kept (current build)\nNeeds you: the spend is not known (the billing token is missing), so the ceiling is not watched"
    );
    assert_eq!(watch::digest_lines("Thu Nov 26", &[], 0, "Database: 3 days kept (current build)"), "🟢 Running fine · Thu Nov 26\nDatabase: 3 days kept (current build)\nNothing needs you");
    scratch.drop().await;
}

/// C-091: a release is no progress line (its pause, hand-over, admission and resume stay in the log).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_release_posts_nothing_on_progress() {
    let (scratch, mut client, b, _) = ready("df_handover").await;
    let mut inv = with_day_local(inventories().remove(0));
    inv["engine_build"] = json!("c9c23ca5+aabbccddeeff");
    let new = build::register(&mut client, &inv).await.unwrap().engine_build;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-12", 2).await.unwrap();
    work::claim(&mut client, Kind::DayBlock, &b, "day-a", HOUR).await.unwrap().unwrap();
    let t0: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    progress::tick_as(&mut client, PROFILE, LABEL, &t0).await.unwrap();
    let before: i64 = client.query_one("SELECT count(*) FROM core.notice WHERE channel = 'progress'", &[]).await.unwrap().get(0);
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
    let morning: bool = client.query_one("SELECT extract(hour FROM ($1::text::timestamptz AT TIME ZONE 'Etc/GMT+6')) >= 9", &[&t1]).await.unwrap().get(0);
    let posts: Vec<String> = client.query("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(posts.iter().all(|p| !p.contains("release") && !p.contains("paused") && !p.contains("Queue")), "{posts:?}");
    if !morning {
        assert_eq!(posts.len() as i64, before, "{posts:?}");
    }
    scratch.drop().await;
}
