//! C-077: a worker release drains the old build's running day blocks instead of ending them. The
//! hand-over carries every queued job as before, and the days after a running block's current day; the
//! block finishes that day under its lease (published, done, its reference check with core) and its
//! lease then ends; a block that does not finish within the bound is stopped and its day carried. Real
//! sessions (the engine's calendar), the real slice of 2025-12-10 published as each day. Run with
//! `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::refcheck::{self, SCHEMA};
use core_db::work::{self, Claim, Kind, LeaseError};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Write;
use test_support::Scratch;
use test_support::slice::{BASE, DERIVED, bundle, day_local_parts, inventories, parts, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAYS: [&str; 5] = ["2025-12-08", "2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];

async fn ready(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    (scratch, client)
}

/// A build named `name` publishing every profile of the slice (one image, every layer), registered.
async fn registered(client: &mut Client, name: &str) -> String {
    let slots: Vec<Value> = inventories().iter().flat_map(|i| i["slots"].as_array().unwrap().clone()).collect();
    build::register(client, &with_day_local(json!({"engine_build": name, "slots": slots}))).await.unwrap();
    name.to_owned()
}

async fn admit(client: &Client, build: &str) {
    build::admit(client, build, &json!({"checklist": "drain test", "by": "cargo test"})).await.unwrap();
}

fn unique(name: &str) -> String {
    static SENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!("{name}-{}", SENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// `day`'s base and day-local revisions under the block's lease, as a day block sends them; not yet done.
async fn publish_day(client: &mut Client, build: &str, claim: &Claim, day: &str) -> Result<(i64, i64), String> {
    let (mut h, s, f) = parts(BASE);
    (h.represented_date, h.engine_build) = (day.into(), build.into());
    let path = bundle(&unique(&format!("c077-{day}-{}", claim.job_id)), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await;
    std::fs::remove_file(&path).unwrap();
    let base = base.map_err(|e| e.to_string())?.revision_id;
    let (h, s, f) = day_local_parts(build, day, base);
    let path = bundle(&unique(&format!("c077-local-{day}")), &h, &s, &f);
    let local = publish_for_lease(client, &path, &claim.token).await;
    std::fs::remove_file(&path).unwrap();
    Ok((base, local.map_err(|e| e.to_string())?.revision_id))
}

/// The day's reference check as its worker reports it (one that timed out: the account alone).
fn check(day: &str, base: i64, local: i64) -> refcheck::Checked {
    let a = json!({
        "schema": SCHEMA, "session": day, "outcome": "TIMEOUT", "why": "its process group was killed",
        "wall_seconds": 10.0, "suite_seconds": 9.0, "cpu_seconds": null, "cpu_complete": false, "jobs": 6, "deadline_seconds": 7200,
        "revisions": [base, local], "counts": null, "summary": null,
    });
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(a.to_string().as_bytes()).unwrap();
    refcheck::check(gz.finish().unwrap(), day).unwrap()
}

/// Runs `build`'s claimable ordered pass, if any; returns its day.
async fn run_pass(client: &mut Client, build: &str) -> Option<String> {
    let pass = work::claim(client, Kind::OrderedPass, build, "pass", HOUR).await.unwrap()?;
    let day = pass.sessions[0].clone();
    let base: i64 = client
        .query_one(
            "SELECT revision_id FROM vyreon.day_revision WHERE represented_date = $1::text::date AND state = 'PUBLISHED' AND base_revision IS NULL",
            &[&day],
        )
        .await
        .unwrap()
        .get(0);
    let (mut h, s, f) = parts(DERIVED);
    (h.represented_date, h.base_revision, h.engine_build) = (day.clone(), Some(base), build.into());
    let path = bundle(&unique(&format!("c077-derive-{day}")), &h, &s, &f);
    publish_for_lease(client, &path, &pass.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::finish(client, &pass.token).await.unwrap();
    Some(day)
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn job(client: &Client, job: i64) -> (String, String, String, Option<String>) {
    let r = client.query_one("SELECT state, first_session::text, last_session::text, next_session::text FROM core.job WHERE job_id = $1", &[&job]).await.unwrap();
    (r.get(0), r.get(1), r.get(2), r.get(3))
}

async fn notices(client: &Client) -> Vec<String> {
    client.query("SELECT message FROM core.notice ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

/// The release of 2026-11-24 deleted 8 running blocks' first days (hours each). Now: two blocks of the old
/// build run when the release hands over, one on the second day of three (its first published), one on
/// the first of two. The hand-over withdraws the old build and carries the passes and each block's days
/// after its current one at once; both leases go on. The first finishes its day: done says it was the
/// last, its reference check ends its lease and the job is DONE; a further publish under it is refused
/// as gone. The second never finishes: at the bound it is stopped and its day carried to the successor.
/// The successor computes the rest, and the passes run in date order once each day is published.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_release_lets_running_blocks_finish_their_day() {
    let (scratch, mut client) = ready("c077_drain").await;
    let old = registered(&mut client, "slice-old-image").await;
    admit(&client, &old).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[0], DAYS[4], 3).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &old, DAYS[0], DAYS[4], 1).await.unwrap();
    let a = work::claim(&mut client, Kind::DayBlock, &old, "pod-a", HOUR).await.unwrap().unwrap();
    let b = work::claim(&mut client, Kind::DayBlock, &old, "pod-b", HOUR).await.unwrap().unwrap();
    assert_eq!((a.sessions.len(), b.sessions.len()), (3, 2));
    publish_day(&mut client, &old, &a, DAYS[0]).await.unwrap();
    assert!(!work::day_done(&mut client, &a.token, DAYS[0]).await.unwrap(), "days remain in the block");

    // the release: the successor registered, the hand-over previewed and done
    let new = registered(&mut client, "slice-new-image").await;
    let preview = build::hand_over(&mut client, &old, &new, "release 3", true, None).await.unwrap();
    let draining: Vec<(i64, &str, &str)> = preview.draining.iter().map(|d| (d.job, d.worker.as_str(), d.session.as_str())).collect();
    assert_eq!(draining, [(a.job_id, "pod-a", DAYS[1]), (b.job_id, "pod-b", DAYS[3])]);
    let carried: Vec<(String, String, String, String)> = preview.carried.iter().map(|c| (c.kind.clone(), c.state.clone(), c.first.clone(), c.last.clone())).collect();
    let mut expected = vec![
        ("DAY_BLOCK".to_owned(), "DRAINING".to_owned(), DAYS[2].to_owned(), DAYS[2].to_owned()),
        ("DAY_BLOCK".to_owned(), "DRAINING".to_owned(), DAYS[4].to_owned(), DAYS[4].to_owned()),
    ];
    expected.extend(DAYS.iter().map(|d| ("ORDERED_PASS".to_owned(), "QUEUED".to_owned(), d.to_string(), d.to_string())));
    assert_eq!(carried, expected);
    assert_eq!(job(&client, a.job_id).await.2, DAYS[2], "a preview changes nothing");
    let handed = build::hand_over(&mut client, &old, &new, "release 3", false, Some(&preview.token)).await.unwrap();
    assert!(handed.withdrew && handed.hand_over_id.is_some());
    // both blocks still hold their leases, cut to their current day; the old build claims nothing more
    assert_eq!(job(&client, a.job_id).await, ("LEASED".into(), DAYS[0].into(), DAYS[1].into(), Some(DAYS[1].into())));
    assert_eq!(job(&client, b.job_id).await, ("LEASED".into(), DAYS[3].into(), DAYS[3].into(), Some(DAYS[3].into())));
    assert!(work::heartbeat(&mut client, &a.token, HOUR).await.is_ok() && work::heartbeat(&mut client, &b.token, HOUR).await.is_ok());
    let live = build::drains(&client).await.unwrap();
    assert_eq!(live.iter().map(|d| d["worker"].as_str().unwrap()).collect::<Vec<_>>(), ["pod-a", "pod-b"]);
    assert!(notices(&client).await.iter().any(|n| n.contains("2 running day blocks finishing their day first")));
    let refused = build::hand_over(&mut client, &old, &new, "again", true, None).await.unwrap_err();
    assert!(refused.contains("no unfinished work"), "{refused}");
    admit(&client, &new).await;

    // the successor starts on the carried days at once
    let n1 = work::claim(&mut client, Kind::DayBlock, &new, "pod-c", HOUR).await.unwrap().unwrap();
    assert_eq!(n1.sessions, [DAYS[2]]);
    // the first pass runs; the second waits for the draining day
    assert_eq!(run_pass(&mut client, &new).await.as_deref(), Some(DAYS[0]));
    assert_eq!(run_pass(&mut client, &new).await, None, "the pass of a day a draining block computes waits for it");

    // block a finishes its day: its last; the lease lives on for the day's check
    let (base, local) = publish_day(&mut client, &old, &a, DAYS[1]).await.unwrap();
    assert!(work::day_done(&mut client, &a.token, DAYS[1]).await.unwrap(), "the draining block's day is its last");
    assert!(work::heartbeat(&mut client, &a.token, HOUR).await.is_ok());
    assert_eq!(run_pass(&mut client, &new).await.as_deref(), Some(DAYS[1]), "the day is published: its pass runs");
    refcheck::put(&mut client, &a.token, &check(DAYS[1], base, local)).await.unwrap();
    assert_eq!(job(&client, a.job_id).await.0, "DONE");
    assert!(matches!(work::heartbeat(&mut client, &a.token, HOUR).await, Err(LeaseError::Gone)), "its lease ended with the check");
    // a worker of an engine before C-077 goes on to its next day: refused, nothing written
    let gone = publish_day(&mut client, &old, &a, DAYS[2]).await.unwrap_err();
    assert!(gone.to_lowercase().contains("lease"), "{gone}");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE represented_date = '2025-12-10'").await, 0);

    // block b never finishes: at the bound it is stopped and its day is the successor's
    work::sweep(&mut client).await.unwrap();
    assert_eq!(job(&client, b.job_id).await.0, "LEASED", "within its bound it runs on");
    client.execute("UPDATE core.drain SET until = now() - interval '1 second' WHERE lease_id = (SELECT lease_id FROM core.lease WHERE job_id = $1)", &[&b.job_id]).await.unwrap();
    work::sweep(&mut client).await.unwrap();
    assert_eq!(job(&client, b.job_id).await.0, "CANCELLED");
    assert!(matches!(work::heartbeat(&mut client, &b.token, HOUR).await, Err(LeaseError::Gone)));
    let carried: (i64, String) = {
        let r = client
            .query_one("SELECT d.carried_job, j.engine_build FROM core.drain d JOIN core.job j ON j.job_id = d.carried_job WHERE d.job_id = $1", &[&b.job_id])
            .await
            .unwrap();
        (r.get(0), r.get(1))
    };
    assert_eq!(carried.1, new);
    assert_eq!(job(&client, carried.0).await, ("QUEUED".into(), DAYS[3].into(), DAYS[3].into(), Some(DAYS[3].into())));
    assert_eq!(count(&client, "SELECT count(*) FROM core.drain WHERE settled_at IS NULL").await, 0, "both drains settled");
    let log = notices(&client).await;
    assert!(log.iter().any(|n| n.contains(&format!("Draining day block job {} of build {old} finished {}", a.job_id, DAYS[1]))), "{log:?}");
    assert!(log.iter().any(|n| n.contains(&format!("Draining day block job {} stopped at its bound", b.job_id))), "{log:?}");
    assert!(log.iter().any(|n| n.contains(&format!("the day is build {new}'s, job {}", carried.0))), "{log:?}");
    assert!(build::drains(&client).await.unwrap().is_empty());

    // the successor computes the rest; the passes run in date order
    publish_day(&mut client, &new, &n1, DAYS[2]).await.unwrap();
    work::day_done(&mut client, &n1.token, DAYS[2]).await.unwrap();
    work::finish(&mut client, &n1.token).await.unwrap();
    while let Some(block) = work::claim(&mut client, Kind::DayBlock, &new, "pod-d", HOUR).await.unwrap() {
        for day in block.sessions.clone() {
            publish_day(&mut client, &new, &block, &day).await.unwrap();
            work::day_done(&mut client, &block.token, &day).await.unwrap();
        }
        work::finish(&mut client, &block.token).await.unwrap();
    }
    let mut ran = Vec::new();
    while let Some(day) = run_pass(&mut client, &new).await {
        ran.push(day);
    }
    assert_eq!(ran, &DAYS[2..]);
    scratch.drop().await;
}

/// A running block is drained only where the successor reads what its day publishes (C-064): one
/// whose successor does not publish the old build's day-local profile is stopped as before, its whole
/// remainder carried and restarted.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_block_whose_day_the_successor_cannot_read_is_stopped_as_before() {
    let (scratch, mut client) = ready("c077_no_drain").await;
    let old = registered(&mut client, "slice-old-image").await;
    admit(&client, &old).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[0], DAYS[1], 2).await.unwrap();
    let a = work::claim(&mut client, Kind::DayBlock, &old, "pod-a", HOUR).await.unwrap().unwrap();
    // the successor publishes the base profile only
    let mut base_only = inventories()[0].clone();
    base_only["engine_build"] = json!("slice-new-base-only");
    let new = build::register(&mut client, &base_only).await.unwrap().engine_build;
    let preview = build::hand_over(&mut client, &old, &new, "release 3", true, None).await.unwrap();
    assert!(preview.draining.is_empty());
    let handed = build::hand_over(&mut client, &old, &new, "release 3", false, Some(&preview.token)).await.unwrap();
    assert_eq!(handed.carried.iter().map(|c| (c.state.as_str(), c.first.as_str(), c.last.as_str())).collect::<Vec<_>>(), [("QUEUED", DAYS[0], DAYS[1])]);
    assert!(matches!(work::heartbeat(&mut client, &a.token, HOUR).await, Err(LeaseError::Gone)), "stopped by the withdrawal");
    assert_eq!(count(&client, "SELECT count(*) FROM core.drain").await, 0);
    scratch.drop().await;
}

async fn open_problems(client: &Client, kind: &str) -> Vec<String> {
    client.query("SELECT key FROM core.problem WHERE kind = $1 AND resolved_at IS NULL ORDER BY key", &[&kind]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

fn watch_settings() -> core_db::watch::Settings {
    core_db::watch::Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: core_db::watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None,
        spend: None,
        recompute: None,
        refcheck_rerun_hours: None,
    }
}

/// Release-3 audit, Codex 4, Gemini 5: `next_session` moves on at `day_done`, before the day's archive and
/// reference check. A block whose worker reports the check of a done day finishes that day (not the next),
/// its later days carried; a block whose every day is done (its next session none) drains too, its lease
/// kept for the check, never ended by the withdrawal. Meanwhile the new build's day blocks wait for the drains'
/// slots: no stall while they drain within their bound (said on the progress channel), STALLED once past it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_hand_over_lets_a_done_days_archive_and_check_finish() {
    let (scratch, mut client) = ready("c077_post_work").await;
    let old = registered(&mut client, "slice-old-image").await;
    admit(&client, &old).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[0], DAYS[2], 3).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[3], DAYS[3], 1).await.unwrap();
    let a = work::claim(&mut client, Kind::DayBlock, &old, "pod-a", HOUR).await.unwrap().unwrap();
    let b = work::claim(&mut client, Kind::DayBlock, &old, "pod-b", HOUR).await.unwrap().unwrap();
    let (base, local) = publish_day(&mut client, &old, &a, DAYS[0]).await.unwrap();
    assert!(!work::day_done(&mut client, &a.token, DAYS[0]).await.unwrap());
    work::report_step(&mut client, &a.token, &format!("reference check {}", DAYS[0]), None, None).await.unwrap();
    publish_day(&mut client, &old, &b, DAYS[3]).await.unwrap();
    assert!(work::day_done(&mut client, &b.token, DAYS[3]).await.unwrap());
    let new = registered(&mut client, "slice-new-image").await;
    let preview = build::hand_over(&mut client, &old, &new, "release 3", true, None).await.unwrap();
    let handed = build::hand_over(&mut client, &old, &new, "release 3", false, Some(&preview.token)).await.unwrap();
    let draining: Vec<(i64, &str)> = handed.draining.iter().map(|d| (d.job, d.session.as_str())).collect();
    assert_eq!(draining, [(a.job_id, DAYS[0]), (b.job_id, DAYS[3])]);
    assert_eq!(job(&client, a.job_id).await, ("LEASED".into(), DAYS[0].into(), DAYS[0].into(), None), "cut to the done day");
    let carried: Vec<(String, String, String)> = handed.carried.iter().filter(|c| c.kind == "DAY_BLOCK").map(|c| (c.state.clone(), c.first.clone(), c.last.clone())).collect();
    assert_eq!(carried, [("DRAINING".to_owned(), DAYS[1].to_owned(), DAYS[2].to_owned())]);
    assert!(work::heartbeat(&mut client, &a.token, HOUR).await.is_ok() && work::heartbeat(&mut client, &b.token, HOUR).await.is_ok());
    admit(&client, &new).await;
    // the new build's blocks claimable and no pod of it claims: while the old build's blocks drain within
    // their bound their slots explain it (live, 2026-11-25: a false STALLED with day-block's 8 slots all
    // draining); the progress channel says so. Gemini 14: once a drain is past its bound, it no longer does
    let s = watch_settings();
    // (release-4 audit, Codex 7) the slot is the pod's ScaledJob, named in its pod's name: day-block pods
    // that claimed daily (priority 1) blocks hold day-block's slots, not the daily one's
    client.execute("UPDATE core.job SET priority = 1 WHERE job_id = ANY($1)", &[&vec![a.job_id, b.job_id]]).await.unwrap();
    client.execute("UPDATE core.lease SET worker = 'day-block-' || lease_id || '-pod' WHERE job_id = ANY($1)", &[&vec![a.job_id, b.job_id]]).await.unwrap();
    let t0: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    core_db::watch::check(&mut client, &s, &t0).await.unwrap();
    let at = |m: i32| format!("SELECT ($1::text::timestamptz + make_interval(mins => {m}))::text");
    let t1: String = client.query_one(&at(20), &[&t0]).await.unwrap().get(0);
    client.execute("UPDATE core.lease SET expires_at = $1::text::timestamptz + interval '1 hour' WHERE ended_at IS NULL", &[&t1]).await.unwrap();
    core_db::watch::check(&mut client, &s, &t1).await.unwrap();
    assert!(open_problems(&client, "STALLED").await.is_empty(), "the drains hold the slots: no stall");
    let post = core_db::progress::preview(&mut client, PROFILE, &t1).await.unwrap().join("\n");
    assert!(post.contains("Draining: 2 blocks of build slice-old-image finishing their day (2 with the day done, its check to come), the first bound "), "{post}");
    assert!(post.contains("the new build's blocks start as they end"), "{post}");
    // the half hours go by with nothing published: each window's post still says the blocks drain (release-4
    // audit, Codex 5: a quiet window's one line dropped it)
    core_db::progress::tick(&mut client, PROFILE, &t1).await.unwrap();
    let t1b: String = client.query_one(&at(51), &[&t0]).await.unwrap().get(0);
    core_db::progress::tick(&mut client, PROFILE, &t1b).await.unwrap();
    let last: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(last.contains("Draining: 2 blocks"), "{last}");
    // one drain past its bound (the sweep did not stop it) beside one within it: the kind is not excused
    // (release-4 audit, Codex 6); after the stall's minutes, STALLED
    client.execute("UPDATE core.drain SET until = $1::text::timestamptz - interval '1 minute' WHERE job_id = $2", &[&t1, &b.job_id]).await.unwrap();
    core_db::watch::check(&mut client, &s, &t1).await.unwrap();
    let t2: String = client.query_one(&at(40), &[&t0]).await.unwrap().get(0);
    client.execute("UPDATE core.lease SET expires_at = $1::text::timestamptz + interval '1 hour' WHERE ended_at IS NULL", &[&t2]).await.unwrap();
    core_db::watch::check(&mut client, &s, &t2).await.unwrap();
    assert_eq!(open_problems(&client, "STALLED").await, ["DAY_BLOCK"]);
    client.execute("UPDATE core.drain SET until = now() + interval '6 hours'", &[]).await.unwrap();
    // the done day's check ends its lease
    refcheck::put(&mut client, &a.token, &check(DAYS[0], base, local)).await.unwrap();
    assert_eq!(job(&client, a.job_id).await.0, "DONE");
    assert!(matches!(work::heartbeat(&mut client, &a.token, HOUR).await, Err(LeaseError::Gone)));
    scratch.drop().await;
}

/// Release-3 audit, Gemini 3: a drain that ends after its day's base and before its day-local revision
/// leaves no half day published (the base superseded, as a day set aside's is). Codex 16, Gemini 16: its
/// day goes to a build that runs, though the hand-over's successor was withdrawn since with nothing handed
/// on. Gemini 13: its day failing for the last time beside two failed days pauses nothing (the sweep carries it).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_drain_that_ends_without_its_day_leaves_no_half_day_and_finds_a_running_build() {
    let (scratch, mut client) = ready("c077_half_day").await;
    let old = registered(&mut client, "slice-old-image").await;
    admit(&client, &old).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[0], DAYS[0], 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[1], DAYS[1], 1).await.unwrap();
    let a = work::claim(&mut client, Kind::DayBlock, &old, "pod-a", HOUR).await.unwrap().unwrap();
    let b = work::claim(&mut client, Kind::DayBlock, &old, "pod-b", HOUR).await.unwrap().unwrap();
    let mid = registered(&mut client, "slice-mid-image").await;
    let preview = build::hand_over(&mut client, &old, &mid, "release 3", true, None).await.unwrap();
    build::hand_over(&mut client, &old, &mid, "release 3", false, Some(&preview.token)).await.unwrap();
    admit(&client, &mid).await;
    // the base of the first day, then nothing more
    let (h, s, f) = parts(BASE);
    let mut h = h;
    (h.represented_date, h.engine_build) = (DAYS[0].into(), old.clone());
    let path = bundle(&unique("c077-half"), &h, &s, &f);
    let base = publish_for_lease(&mut client, &path, &a.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    // the successor withdrawn with nothing to hand on; another build runs
    build::withdraw(&mut client, &mid, "a bad image").await.unwrap();
    let last = registered(&mut client, "slice-last-image").await;
    admit(&client, &last).await;
    // two days stand failed, then the second drain fails for the last time: no pause
    client.execute("INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, state, attempts, max_attempts)
                    VALUES ('DAY_BLOCK', $1, $2, '2025-12-15', '2025-12-15', '2025-12-15', 'FAILED', 3, 3), ('DAY_BLOCK', $1, $2, '2025-12-16', '2025-12-16', '2025-12-16', 'FAILED', 3, 3)",
                   &[&PROFILE, &last]).await.unwrap();
    client.execute("UPDATE core.job SET attempts = max_attempts WHERE job_id = $1", &[&b.job_id]).await.unwrap();
    assert!(!work::fail(&mut client, &b.token, "day 2025-12-09", "stop: 2025-12-09 not published: a fault", "log").await.unwrap());
    let paused: bool = client.query_one("SELECT paused FROM core.queue_control", &[]).await.unwrap().get(0);
    assert!(!paused, "the withdrawn build's draining day is no third failed day of the running builds");
    // the first drain's bound passes
    client.execute("UPDATE core.drain SET until = now() - interval '1 second' WHERE job_id = $1", &[&a.job_id]).await.unwrap();
    work::sweep(&mut client).await.unwrap();
    let state: String = client.query_one("SELECT state FROM vyreon.day_revision WHERE revision_id = $1", &[&base]).await.unwrap().get(0);
    assert_eq!(state, "SUPERSEDED", "no base without its day-local revision stays published");
    let carried: Vec<String> = client
        .query("SELECT j.engine_build FROM core.drain d JOIN core.job j ON j.job_id = d.carried_job ORDER BY d.job_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(carried, [last.clone(), last.clone()], "both days to the build that runs");
    scratch.drop().await;
}

/// Release-3 audit, Codex 2, Gemini 2: a draining block publishes a day earlier than days whose history the
/// old build already derived (a day retried after later passes ran). Those histories fall; the publisher
/// and the fallen revisions' build are both withdrawn, so the successor that runs derives them again.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_draining_days_publish_has_its_later_history_derived_again_by_the_successor() {
    let (scratch, mut client) = ready("c077_rederive").await;
    let old = registered(&mut client, "slice-old-image").await;
    admit(&client, &old).await;
    // the second day published and its history derived by the old build
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[1], DAYS[1], 1).await.unwrap();
    let b = work::claim(&mut client, Kind::DayBlock, &old, "pod-b", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &old, &b, DAYS[1]).await.unwrap();
    work::day_done(&mut client, &b.token, DAYS[1]).await.unwrap();
    work::finish(&mut client, &b.token).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &old, DAYS[1], DAYS[1], 1).await.unwrap();
    assert_eq!(run_pass(&mut client, &old).await.as_deref(), Some(DAYS[1]));
    // the first day, computed later, drains through a release
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, DAYS[0], DAYS[0], 1).await.unwrap();
    let a = work::claim(&mut client, Kind::DayBlock, &old, "pod-a", HOUR).await.unwrap().unwrap();
    let new = registered(&mut client, "slice-new-image").await;
    let preview = build::hand_over(&mut client, &old, &new, "release 3", true, None).await.unwrap();
    build::hand_over(&mut client, &old, &new, "release 3", false, Some(&preview.token)).await.unwrap();
    admit(&client, &new).await;
    publish_day(&mut client, &old, &a, DAYS[0]).await.unwrap();
    let passes: Vec<(String, String)> = client
        .query("SELECT engine_build, state FROM core.job WHERE kind = 'ORDERED_PASS' AND first_session = '2025-12-09' AND state <> 'DONE' ORDER BY job_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(passes, [(new.clone(), "QUEUED".to_owned())], "the fallen history is derived again under the build that runs");
    assert!(open_problems(&client, "NOT_DERIVED").await.is_empty());
    scratch.drop().await;
}

/// Release-4 audit, Codex 1: a release resumes only its own pause, atomically: a cause added after its read
/// (a draining worker's bundle refused) leaves the queue paused, the causes kept.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_release_resumes_only_its_own_pause() {
    let (scratch, client) = ready("c077_resume_if").await;
    let mine = "release abc+def: release 4";
    assert!(!work::resume_if(&client, mine).await.unwrap(), "not paused: nothing to resume");
    work::pause(&client, mine).await.unwrap();
    work::pause(&client, "bundle refused: 2024-07-23").await.unwrap();
    let refused = work::resume_if(&client, mine).await.unwrap_err();
    assert!(refused.contains("bundle refused: 2024-07-23") && refused.contains("left paused"), "{refused}");
    let paused: bool = client.query_one("SELECT paused FROM core.queue_control", &[]).await.unwrap().get(0);
    assert!(paused);
    work::resume(&client).await.unwrap();
    work::pause(&client, mine).await.unwrap();
    assert!(work::resume_if(&client, mine).await.unwrap());
    let paused: bool = client.query_one("SELECT paused FROM core.queue_control", &[]).await.unwrap().get(0);
    assert!(!paused);
    scratch.drop().await;
}
