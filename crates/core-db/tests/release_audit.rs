//! C-073: the release audit of core-auto (2026-11-24, Codex and Gemini), each real finding in core
//! as a test on the real slice of 2025-12-10 and the engine's calendar. Run with `cargo test -- --ignored`.
use core_contract::sha256_hex;
use core_db::publish::publish_for_lease;
use core_db::refcheck::{self, SCHEMA};
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Write;
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, reading, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAY: &str = "2025-12-10";

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
    build::admit(&client, &base, &json!({"checklist": "release audit"})).await.unwrap();
    (scratch, client, base, derived)
}

fn settings(refcheck: bool) -> Settings {
    Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: refcheck.then(|| DAY.to_owned()),
        backup_days: None, spend: None, recompute: None,
        refcheck_rerun_hours: refcheck.then_some(refcheck::RERUN_AFTER_HOURS),
    }
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn open(client: &Client, kind: &str) -> Vec<String> {
    client.query("SELECT summary FROM core.problem WHERE resolved_at IS NULL AND kind = $1 ORDER BY key", &[&kind]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

async fn at(client: &Client, sql_interval: &str) -> String {
    client.query_one(&format!("SELECT (now() + interval '{sql_interval}')::text"), &[]).await.unwrap().get(0)
}

/// Publishes 2025-12-10's base and day-local revision under a one-day block; returns (base, local, the block's token).
async fn published_day(client: &mut Client, build: &str) -> (i64, i64, String) {
    work::schedule(client, Kind::DayBlock, PROFILE, build, DAY, DAY, 30).await.unwrap();
    let block = work::claim(client, Kind::DayBlock, build, "w1", HOUR).await.unwrap().unwrap();
    let (h, s, f) = parts(BASE);
    let path = bundle("ra-base", &h, &s, &f);
    let base = publish_for_lease(client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(build, DAY, base);
    let path = bundle("ra-local", &lh, &ls, &lf);
    let local = publish_for_lease(client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    work::day_done(client, &block.token, DAY).await.unwrap();
    (base, local, block.token)
}

fn account(outcome: &str, revisions: Value) -> refcheck::Checked {
    let a = json!({
        "schema": SCHEMA, "session": DAY, "outcome": outcome, "why": "its process group was killed",
        "wall_seconds": 10.0, "suite_seconds": 9.0, "cpu_seconds": null, "cpu_complete": false, "jobs": 6, "deadline_seconds": 36000,
        "revisions": revisions, "counts": null, "summary": null,
    });
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(a.to_string().as_bytes()).unwrap();
    refcheck::check(gz.finish().unwrap(), DAY).unwrap()
}

/// Codex 3, 14, 17, Gemini 2, 3, 5: re-runs are queued only for a complete day, never while the queue
/// is paused or a schedule holds its lock; a day whose worker reported a check that did not complete
/// is due though its block runs on; a re-run of superseded revisions or of a build that does not run
/// is cancelled and the current pair queued; a base-only completed check does not cover a day-local
/// revision; finishing a re-run never says the ordered passes are done; re-runs never refuse a hand-over.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn reruns_are_queued_for_what_is_current_and_runs() {
    let (scratch, mut client, b, d) = ready("ra_reruns").await;
    let (base, local, token) = published_day(&mut client, &b).await;
    let s = settings(true);
    let reruns = "SELECT count(*) FROM core.job WHERE kind = 'REFCHECK' AND state <> 'CANCELLED'";
    // its worker reported a check that timed out; its block (lease) runs on: due at 7 h all the same
    refcheck::put(&mut client, &token, &account("TIMEOUT", json!([base, local]))).await.unwrap();
    // paused: nothing queued
    let tx = client.transaction().await.unwrap();
    work::pause(&tx, "operator").await.unwrap();
    tx.commit().await.unwrap();
    let t7 = at(&client, "7 hours").await;
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 0, "nothing queued while paused");
    let tx = client.transaction().await.unwrap();
    work::resume(&tx).await.unwrap();
    tx.commit().await.unwrap();
    // a schedule holds the scheduler's lock: tried, not waited for
    let holder = scratch.client().await;
    holder.batch_execute("BEGIN; SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))").await.unwrap();
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 0, "the scheduler's lock held: next minute");
    holder.batch_execute("COMMIT").await.unwrap();
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 1, "due though the block's lease lives: its check reported");
    // the day-local revision republished: the stale re-run is cancelled and the current pair queued
    client.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = $1", &[&local]).await.unwrap();
    let local2: i64 = client
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members, base_revision, day_local)
             SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, 'PUBLISHED', published_at, listed_members, base_revision, true
             FROM vyreon.day_revision WHERE revision_id = $1 RETURNING revision_id",
            &[&local],
        )
        .await
        .unwrap()
        .get(0);
    watch::check(&mut client, &s, &t7).await.unwrap();
    let scopes: Vec<String> = client.query("SELECT scope::text FROM core.job WHERE kind = 'REFCHECK' AND state <> 'CANCELLED'", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(scopes.len(), 1);
    assert_eq!(serde_json::from_str::<Value>(&scopes[0]).unwrap(), json!({"base_revision": base, "day_local_revision": local2}), "the current pair");
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE kind = 'REFCHECK' AND state = 'CANCELLED'").await, 1);
    // a base-only completed check is no check of the pair
    let rerun = work::claim(&mut client, Kind::Refcheck, &b, "rc-1", HOUR).await.unwrap().unwrap();
    work::cancel(&mut client, &[rerun.job_id]).await.unwrap();
    client
        .execute(
            "INSERT INTO core.refcheck_report (lease_id, job_id, attempt, worker, engine_build, session, base_revision, outcome, checked, passed, failed, skipped,
                 reference_errors, failure_classes, measurements_failing, wall_seconds, cpu_complete, jobs, deadline_seconds, report_bytes, document_sha256, document)
             SELECT lease_id, job_id, 1, worker, $2, '2025-12-10', $1, 'COMPLETED', 1, 1, 0, 0, 0, 0, 0, 1, true, 1, 1, 0, repeat('0', 64), '\\x00'
             FROM core.lease WHERE job_id = $3",
            &[&base, &b, &rerun.job_id],
        )
        .await
        .unwrap();
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 1, "the pair is queued again: a base-only report does not cover it");
    // a re-run under a build that does not run: cancelled, and none queued while none runs
    build::withdraw(&mut client, &b, "test").await.unwrap();
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 0);
    // a hand-over is not refused for another build's re-run of the same session
    build::admit(&client, &b, &json!({"checklist": "again"})).await.unwrap();
    watch::check(&mut client, &s, &t7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 1);
    let other = client.query_one("SELECT job_id FROM core.job WHERE kind = 'REFCHECK' AND state = 'QUEUED'", &[]).await.unwrap().get::<_, i64>(0);
    client.execute("UPDATE core.job SET engine_build = $2 WHERE job_id = $1", &[&other, &d]).await.unwrap();
    client
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, max_attempts)
             VALUES ('REFCHECK', $1, $2, '2025-12-10', '2025-12-10', '2025-12-10', jsonb_build_object('base_revision', $3::bigint, 'day_local_revision', 1), 2)",
            &[&PROFILE, &b, &base],
        )
        .await
        .unwrap();
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-11", "2025-12-11", 1).await.unwrap();
    let token = build::hand_over(&mut client, &b, "nobody-yet", "x", true, None).await;
    assert!(token.is_err(), "an unknown successor is refused");
    let inv = with_day_local(inventories().remove(0));
    let mut next = inv.clone();
    next["engine_build"] = json!("ffffffff+000000000000");
    build::register(&mut client, &next).await.unwrap();
    let preview = build::hand_over(&mut client, &b, "ffffffff+000000000000", "x", true, None).await.unwrap();
    assert!(preview.carried.iter().any(|c| c.kind == "REFCHECK"), "{preview:?}");
    // finishing the last re-run says so, never that the ordered passes are done
    let line: Vec<String> = client.query("SELECT message FROM core.notice WHERE message LIKE 'All scheduled%' OR message LIKE 'Every scheduled%'", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(line.iter().all(|l| !l.contains("ordered pass")), "{line:?}");
    scratch.drop().await;
}

/// Codex 4, Gemini 1: the daily session's tape is never held back by the budget, so its stall is
/// alerted while the backfill presses the budget; Gemini 6: a job retried after its cause was fixed,
/// or of a build withdrawn, is not thrashing.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_budget_hides_no_daily_stall_and_a_retry_starts_clean() {
    let (scratch, mut client, b, _) = ready("ra_stall").await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, DAY, DAY, 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1", &[]).await.unwrap();
    let s = settings(false);
    let (t20, t40) = (at(&client, "20 minutes").await, at(&client, "60 minutes").await);
    for t in [&t20, &t40] {
        client
            .execute("INSERT INTO core.budget_wait (minute, waits, last_at) VALUES (date_trunc('minute', $1::text::timestamptz), 3, $1::text::timestamptz - interval '1 minute') ON CONFLICT DO NOTHING", &[t])
            .await
            .unwrap();
        watch::check(&mut client, &s, t).await.unwrap();
    }
    assert_eq!(open(&client, "STALLED").await.len(), 1, "the daily tape stalled under pressure is alerted");
    // thrashing: three lost leases, then retry-failed (the cause fixed): clean
    client.execute("UPDATE core.job SET max_attempts = 1, priority = 0", &[]).await.unwrap();
    for _ in 0..3 {
        let c = work::claim(&mut client, Kind::Tape, &b, "t1", HOUR).await.unwrap().unwrap();
        client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1 AND ended_at IS NULL", &[&c.job_id]).await.unwrap();
        work::sweep(&mut client).await.unwrap();
    }
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE state = 'FAILED'").await, 1);
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    let now = watch::now(&client).await.unwrap();
    watch::check(&mut client, &s, &now).await.unwrap();
    assert!(open(&client, "JOB_THRASHING").await.is_empty(), "a retried job starts clean");
    scratch.drop().await;
}

/// Codex 18, Gemini 10: another job's tape cache is no progress of a day block; a verify job's checks are.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn progress_is_the_workers_own() {
    let (scratch, mut client, b, _) = ready("ra_progress").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "day-1", 24.0 * HOUR).await.unwrap().unwrap();
    work::report_step(&mut client, &block.token, "compute", None, None).await.unwrap();
    let t25 = at(&client, "25 minutes").await;
    assert!(core_db::tape::put(&client, "AAPL", DAY, "engine-v1", &core_db::tape::Cache::Body(test_support::TAPE_AAPL.to_vec())).await.unwrap());
    client.execute("UPDATE core.tape_cache SET stored_at = $1::text::timestamptz - interval '1 minute'", &[&t25]).await.unwrap();
    watch::check(&mut client, &settings(false), &t25).await.unwrap();
    assert_eq!(open(&client, "NO_PROGRESS").await.len(), 1, "a tape cache stored by another job is not this block's progress");
    scratch.drop().await;
}

/// Codex 11, 12, 13, Gemini 11, 13: a re-sent day is acknowledged only when every field is the
/// published one (a -0 kept as 0 is the same number); a changed body from the same lease is refused,
/// never published over the first; a second base of another profile on the day is refused whichever
/// lease published the first.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_resend_is_the_published_day_or_refused() {
    let (scratch, mut client, b, _) = ready("ra_resend").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (header, mut scalars, files) = parts(BASE);
    scalars.reader_scalar[0].value = -0.0;
    let path = bundle("ra-first", &header, &scalars, &files);
    let first = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    assert!(publish_for_lease(&mut client, &path, &block.token).await.unwrap().already_published, "the same, its -0 stored as 0");
    std::fs::remove_file(&path).unwrap();
    // a changed receipt, its digest matching: refused
    let (mut h2, s2, mut f2) = (header.clone(), scalars.clone(), files.clone());
    let mut receipt: Value = serde_json::from_slice(&f2[0].receipt).unwrap();
    receipt["changed"] = json!(true);
    f2[0].receipt = serde_json::to_vec(&receipt).unwrap();
    h2.slots[0].receipt_sha256 = sha256_hex(&f2[0].receipt);
    let path = bundle("ra-receipt", &h2, &s2, &f2);
    let e = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&e, LeaseError::Refused(m) if m.contains("differs (its state, versions, receipt, coordinates or stream)")), "{e:?}");
    // a changed body: refused, the first stands
    let (mut h3, mut f3) = (header.clone(), files.clone());
    let mut body: Value = serde_json::from_slice(&f3[0].body).unwrap();
    body["changed"] = json!(true);
    f3[0].body = format!("{body}\n").into_bytes();
    h3.slots[0].body_sha256 = sha256_hex(&f3[0].body);
    let path = bundle("ra-body", &h3, &scalars, &f3);
    let e = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&e, LeaseError::Refused(m) if m.contains(&format!("as revision {}; this bundle differs from it", first.revision_id))), "{e:?}");
    assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.day_revision WHERE revision_id = {} AND state = 'PUBLISHED'", first.revision_id)).await, 1);
    // another lease: a stripped day-local bundle is no second base
    work::cancel(&mut client, &[block.job_id]).await.unwrap();
    client.execute("UPDATE vyreon.day_revision SET state = 'PUBLISHED' WHERE revision_id = $1", &[&first.revision_id]).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let second = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    let (mut lh, ls, lf) = day_local_parts(&b, DAY, first.revision_id);
    (lh.base_profile_revision, lh.base_revision, lh.day_local) = (None, None, false);
    let path = bundle("ra-stripped", &lh, &ls, &lf);
    let e = publish_for_lease(&mut client, &path, &second.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&e, LeaseError::Refused(m) if m.contains("a second base in profile")), "{e:?}");
    scratch.drop().await;
}

/// Codex 10: the CLI's count of waits is never more than the waits within its minutes.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_wait_count_is_of_its_minutes() {
    let (scratch, client, _, _) = ready("ra_waits").await;
    client
        .execute("INSERT INTO core.budget_wait (minute, waits, last_at) VALUES ('2026-11-24 12:00+00', 2, '2026-11-24 12:00:55+00'), ('2026-11-24 12:03+00', 1, '2026-11-24 12:03:10+00')", &[])
        .await
        .unwrap();
    let (last, n) = core_db::budget::pressed(&client, "2026-11-24 12:05:30+00").await.unwrap().unwrap();
    assert_eq!((last.as_str(), n), ("2026-11-24 12:03:10", 1), "12:00's two waits are before 12:00:30");
    scratch.drop().await;
}

/// C-073 (owner, after the release audit's C13 residual): a bundle's layer is what its registered
/// profile is. On a build's very first day, with nothing of it published, a day-local bundle stripped
/// of its base fields is refused as a base; the inventory's declaration is checked when registered.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_bundles_layer_is_its_registered_profiles() {
    let (scratch, mut client, _, _) = ready("ra_layer").await;
    let mut inv = with_day_local(inventories().remove(0));
    let base_profile = inv["slots"][0]["profile_revision"].as_str().unwrap().to_owned();
    let local_profile = test_support::slice::day_local_profile();
    inv["engine_build"] = json!("abcdef01+0123456789ab");
    // a declaration that misses a profile, or names an unknown layer, is refused
    inv["profiles"] = json!({base_profile.clone(): "base"});
    assert!(build::register(&mut client, &inv).await.unwrap_err().contains("declares exactly the profile revisions"));
    inv["profiles"] = json!({base_profile.clone(): "base", local_profile.clone(): "derived"});
    assert!(build::register(&mut client, &inv).await.unwrap_err().contains("is base, day-local or history"));
    inv["profiles"] = json!({base_profile.clone(): "base", local_profile.clone(): "day-local"});
    let b = build::register(&mut client, &inv).await.unwrap().engine_build;
    let mut other = inv.clone();
    other["profiles"] = json!({base_profile.clone(): "day-local", local_profile.clone(): "base"});
    assert!(build::register(&mut client, &other).await.unwrap_err().contains("is registered as its"));
    build::admit(&client, &b, &json!({"checklist": "layer"})).await.unwrap();
    // its first day: nothing of the build published yet; the stripped day-local bundle comes first
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (mut lh, ls, lf) = day_local_parts(&b, DAY, 1);
    (lh.base_profile_revision, lh.base_revision, lh.day_local) = (None, None, false);
    let path = bundle("ra-first-day-stripped", &lh, &ls, &lf);
    let e = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&e, LeaseError::Refused(m) if m.contains("is registered as its day-local profile; this bundle is presented as a base revision")), "{e:?}");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, 0, "nothing published");
    // the real base, then its day-local revision, go through
    let (mut h, s, f) = parts(BASE);
    h.engine_build = b.clone();
    let path = bundle("ra-first-day-base", &h, &s, &f);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(&b, DAY, base);
    let path = bundle("ra-first-day-local", &lh, &ls, &lf);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    scratch.drop().await;
}
