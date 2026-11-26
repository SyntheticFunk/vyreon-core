//! C-086 (the backfill's day blocks ramp up, one at a time until the build has published a real day clean),
//! C-087 (the backfill's stop, one flag) and C-088 (core's downtime costs no lease), owner-approved
//! 2026-11-26. On the real slice of 2025-12-10 (its base and day-local revisions as a day block publishes
//! them, and the engine's real faulted slot of that day) and the engine's calendar. Run with
//! `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{backfill, build, migrate, progress, universe};
use serde_json::{Value, json};
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
use test_support::{ENGINE_FAULT_BODY_XLB, Scratch};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAY: &str = "2025-12-10";

/// A migrated database with the calendar, the universe and the slice's base build, admitted.
async fn ready(test: &str) -> (Scratch, Client, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let base = build::register(&mut client, &with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    build::admit(&client, &base, &json!({"checklist": "backfill test"})).await.unwrap();
    (scratch, client, base)
}

/// KEDA's own login (group `vyreon_scaler`), which may only execute the count.
async fn scaler(scratch: &Scratch, client: &Client, name: &str) -> Client {
    let login = format!("scaler_{name}_{}", std::process::id());
    client.batch_execute(&format!("DROP ROLE IF EXISTS {login}; CREATE ROLE {login} LOGIN PASSWORD 'scaler-test' IN ROLE vyreon_scaler")).await.unwrap();
    let mut config = scratch.config.clone();
    config.user(&login).password("scaler-test");
    test_support::connect(&config).await
}

async fn counted(scaler: &Client, kind: &str, build: &str, daily: Option<bool>) -> i64 {
    scaler.query_one("SELECT core.scaler_count($1, $2, $3)", &[&kind, &build, &daily]).await.unwrap().get(0)
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

/// Every backfill day-block lease granted `minutes` earlier than it was (the ramp's clock moved on).
async fn age_leases(client: &Client, minutes: i64) {
    client
        .execute(
            "UPDATE core.lease l SET granted_at = granted_at - make_interval(mins => $1::int) FROM core.job j WHERE j.job_id = l.job_id AND j.kind = 'DAY_BLOCK'",
            &[&(minutes as i32)],
        )
        .await
        .unwrap();
}

/// The slice's base and day-local revisions published under `token`, the day marked done: (base, local).
async fn publish_day(client: &mut Client, build: &str, token: &str, name: &str) -> (i64, i64) {
    let (header, scalars, files) = parts(BASE);
    let path = bundle(&format!("{name}-base"), &header, &scalars, &files);
    let base = publish_for_lease(client, &path, token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(build, DAY, base);
    let path = bundle(&format!("{name}-day-local"), &lh, &ls, &lf);
    let local = publish_for_lease(client, &path, token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    work::day_done(client, token, DAY).await.unwrap();
    (base, local)
}

fn settings(refcheck: bool) -> Settings {
    Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: refcheck.then(|| DAY.into()),
        backup_days: None,
        spend: None,
        recompute: None,
        refcheck_rerun_hours: refcheck.then_some(core_db::refcheck::RERUN_AFTER_HOURS),
    }
}

async fn open(client: &Client, kind: &str) -> Vec<String> {
    client
        .query("SELECT summary FROM core.problem WHERE resolved_at IS NULL AND kind = $1 ORDER BY key", &[&kind])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

/// `minutes` from the database's now, as the watch takes its time.
async fn later(client: &Client, minutes: i32) -> String {
    client.query_one("SELECT (now() + make_interval(mins => $1))::text", &[&minutes]).await.unwrap().get(0)
}

/// C-086: KEDA's count for the backfill's day blocks is at most 1, and 0 for `RAMP_MINUTES` after one was
/// leased; until the build has published one real day with no engine-fault slot (its base and day-local
/// revisions counted by the watch), at most one backfill block runs at a time; once it has, one more every
/// `RAMP_MINUTES` whatever runs. A day whose revisions hold the engine's faulted slot proves nothing. The
/// daily session's block always counts; the claim itself is not gated (as C-073's budget).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn backfill_blocks_start_one_at_a_time_until_the_build_published_a_real_day_clean() {
    let (scratch, mut client, build) = ready("backfill_ramp").await;
    let keda = scaler(&scratch, &client, "ramp").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, "2025-12-16", 1).await.unwrap();
    assert_eq!(count(&client, &format!("SELECT core.claimable_count('DAY_BLOCK', '{build}')")).await, 5);
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 1, "one block, not five");
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, None).await, 1);

    // the first block claims: nothing more for the ramp's minutes, nor after them while it is the only proof
    let first = work::claim(&mut client, Kind::DayBlock, &build, "day-block-1", HOUR).await.unwrap().unwrap();
    assert_eq!(first.sessions[0], DAY);
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0, "leased within the ramp's minutes");
    age_leases(&client, work::RAMP_MINUTES + 1).await;
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0, "the build has published no real day: one block at a time");
    // a daily session's block counts whatever the backfill does
    client.execute("UPDATE core.job SET priority = 1 WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-16'", &[]).await.unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(true)).await, 1);
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, None).await, 1);
    client.execute("UPDATE core.job SET priority = 0 WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-16'", &[]).await.unwrap();

    // the day published, with the engine's real faulted slot in its base: counted, it proves nothing
    let (base, local) = publish_day(&mut client, &build, &first.token, "ramp").await;
    let body = ENGINE_FAULT_BODY_XLB.trim_end();
    let body_json: Value = serde_json::from_str(body).unwrap();
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             VALUES ($1, 'SAI-EXQ01', 'XLB', $2::text::date, 'DELIVERED', 'engine-fault', 'engine-fault', $3, $4, $5::text::jsonb, '{}')",
            &[&base, &DAY, &core_contract::sha256_hex(ENGINE_FAULT_BODY_XLB.as_bytes()), &body, &body_json.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0, "not counted yet: not proven");
    watch::tally_batches(&mut client, &[base, local], None).await.unwrap();
    assert_eq!(count(&client, &format!("SELECT engine_faults::bigint FROM core.revision_tally WHERE revision_id = {base}")).await, 1);
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0, "a day with an engine fault proves nothing");

    // the same day clean: proven, so one more block now, though the first still runs
    client.execute("DELETE FROM vyreon.measurement WHERE revision_id = $1 AND measurement_id = 'SAI-EXQ01' AND object_id = 'XLB' AND method_version = 'engine-fault'", &[&base]).await.unwrap();
    client.execute("DELETE FROM core.revision_tally WHERE revision_id = ANY($1)", &[&vec![base, local]]).await.unwrap();
    watch::tally_batches(&mut client, &[base, local], None).await.unwrap();
    assert!(client.query_one("SELECT core.build_proven($1)", &[&build]).await.unwrap().get::<_, bool>(0));
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 1, "proven: the next block may start");
    let second = work::claim(&mut client, Kind::DayBlock, &build, "day-block-2", HOUR).await.unwrap().unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0, "and the ramp holds the one after for its minutes");
    age_leases(&client, work::RAMP_MINUTES + 1).await;
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 1, "one at a time however many run");
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease WHERE ended_at IS NULL").await, 2);

    // the claim is not gated: a pod already started still takes a block (KEDA's one window)
    assert!(work::claim(&mut client, Kind::DayBlock, &build, "day-block-3", HOUR).await.unwrap().is_some());
    work::finish(&mut client, &second.token).await.ok();
    scratch.drop().await;
}

/// C-086: a backfill block the ramp holds back is not a stall: a block that failed in its first minute
/// leaves the others claimable with no lease, and the watch raises STALLED only once the ramp would have
/// let a pod start and none claimed for `STALL_MINUTES` after.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_block_the_ramp_holds_back_is_no_stall() {
    let (scratch, mut client, build) = ready("backfill_ramp_stall").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, "2025-12-12", 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 5", &[]).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &build, "day-block-1", HOUR).await.unwrap().unwrap();
    work::fail(&mut client, &block.token, "fill", "fill: the provider refused", "log").await.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease WHERE ended_at IS NULL").await, 0);
    let s = settings(false);
    for m in [1, 10, 19] {
        let at = later(&client, m).await;
        watch::check(&mut client, &s, &at).await.unwrap();
        assert!(open(&client, "STALLED").await.is_empty(), "{m} min: held back by the ramp");
    }
    // the ramp passed at 20 minutes: a pod should start; none claims for the stall's minutes
    for m in [21, 30] {
        let at = later(&client, m).await;
        watch::check(&mut client, &s, &at).await.unwrap();
        assert!(open(&client, "STALLED").await.is_empty(), "{m} min");
    }
    let at = later(&client, 37).await;
    watch::check(&mut client, &s, &at).await.unwrap();
    assert_eq!(open(&client, "STALLED").await.len(), 1, "no pod since the ramp let one start: a stall");
    scratch.drop().await;
}

/// C-087: `backfill stop` cancels every queued or leased backfill day block and ordered pass and every
/// queued re-run, never a daily session's work, in one transaction with the flag; while stopped the scaler
/// counts no backfill block, pass or re-run, the watch queues no re-run (the setting's `off` without a
/// restart) and held work is no stall; the progress channel says it with the reason. `resume` clears it, and
/// the re-run is queued as before.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_backfill_stops_and_resumes_by_one_flag() {
    let (scratch, mut client, build) = ready("backfill_stop").await;
    let keda = scaler(&scratch, &client, "stop").await;
    // a day published by a block whose worker ended without a check: due for a re-run 6 h after
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, DAY, 1).await.unwrap();
    let done = work::claim(&mut client, Kind::DayBlock, &build, "day-block-1", HOUR).await.unwrap().unwrap();
    let (base, _) = publish_day(&mut client, &build, &done.token, "stop").await;
    work::finish(&mut client, &done.token).await.unwrap();
    // the backfill's later blocks (one running) and passes, and a daily session's block
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, "2025-12-11", "2025-12-16", 2).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &build, "2025-12-11", "2025-12-12", 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, "2025-12-17", "2025-12-17", 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-17'", &[]).await.unwrap();
    let running = work::claim(&mut client, Kind::DayBlock, &build, "day-block-2", HOUR).await.unwrap().unwrap();
    assert_eq!(running.sessions[0], "2025-12-17", "the daily block is claimed first");
    let running_backfill = work::claim(&mut client, Kind::DayBlock, &build, "day-block-3", HOUR).await.unwrap().unwrap();
    let s = settings(true);
    let at7: String = client.query_one("SELECT (published_at + interval '7 hours')::text FROM vyreon.day_revision WHERE revision_id = $1", &[&base]).await.unwrap().get(0);
    let reruns = "SELECT count(*) FROM core.job WHERE kind = 'REFCHECK' AND state = 'QUEUED'";
    watch::check(&mut client, &s, &at7).await.unwrap();
    assert_eq!(count(&client, reruns).await, 1, "the re-run queued, as before");

    assert_eq!(backfill::status(&client).await.unwrap().line(), "backfill running");
    let cancelled = backfill::stop(&mut client, "  engine not frozen  ").await.unwrap();
    let kinds: Vec<&str> = cancelled.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(kinds, ["DAY_BLOCK", "DAY_BLOCK", "ORDERED_PASS", "ORDERED_PASS", "REFCHECK"], "{cancelled:?}");
    assert!(cancelled.iter().any(|c| c.0 == running_backfill.job_id && c.4 == "LEASED"), "the running backfill block too");
    assert!(cancelled.iter().all(|c| c.0 != running.job_id), "never the daily session's");
    assert!(matches!(work::heartbeat(&mut client, &running_backfill.token, HOUR).await, Err(LeaseError::Gone)), "its worker is told to stop");
    assert!(work::heartbeat(&mut client, &running.token, HOUR).await.is_ok());
    let st = backfill::status(&client).await.unwrap();
    assert!(st.stopped && st.reason.as_deref() == Some("engine not frozen") && st.line().starts_with("backfill STOPPED since "), "{st:?}");
    assert_eq!(work::status(&client).await.unwrap()["backfill_stopped"], true);
    let logged: String = client.query_one("SELECT message FROM core.notice ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(logged.starts_with("Backfill stopped (2 day blocks, 2 ordered passes and 1 reference-check re-runs cancelled;"), "{logged}");
    assert_eq!(progress::short_notice(&logged).as_deref(), Some("Backfill stopped: engine not frozen"));

    // work queued while stopped (a schedule by hand) starts no pod, and is no stall; the daily's counts
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, "2025-12-11", "2025-12-12", 2).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &build, "2025-12-11", "2025-12-11", 1).await.unwrap();
    client.execute("UPDATE core.lease SET granted_at = granted_at - interval '1 hour'", &[]).await.unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 0);
    assert_eq!(counted(&keda, "ORDERED_PASS", &build, None).await, 0);
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, "2025-12-18", "2025-12-18", 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-18'", &[]).await.unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(true)).await, 1, "the daily session's work is never held");
    // the daily session's blocks gone, so no live day-block lease excuses a stall below
    let dailies: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND priority = 1", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    work::cancel(&mut client, &dailies).await.unwrap();
    for m in [30, 60] {
        let at = later(&client, m).await;
        watch::check(&mut client, &s, &at).await.unwrap();
    }
    assert!(open(&client, "STALLED").await.iter().all(|p| !p.starts_with("Day blocks")), "{:?}", open(&client, "STALLED").await);
    // (the watch's clock moves on 8 h here)
    // the re-run watch queues nothing while stopped (the cancelled re-run's pair would be queued again)
    let at8: String = client.query_one("SELECT (published_at + interval '8 hours')::text FROM vyreon.day_revision WHERE revision_id = $1", &[&base]).await.unwrap().get(0);
    watch::check(&mut client, &s, &at8).await.unwrap();
    assert_eq!(count(&client, reruns).await, 0, "no re-run while stopped");

    // stopped again: the first reason kept
    backfill::stop(&mut client, "again").await.unwrap();
    assert_eq!(backfill::status(&client).await.unwrap().reason.as_deref(), Some("engine not frozen"));
    assert_eq!(backfill::stop(&mut client, " ").await.unwrap_err(), "a stop needs a reason");

    // resumed: counted and queued again
    assert!(backfill::resume(&mut client).await.unwrap());
    assert!(!backfill::resume(&mut client).await.unwrap());
    let logged: String = client.query_one("SELECT message FROM core.notice WHERE message LIKE 'Backfill resumed%' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert_eq!(progress::short_notice(&logged).as_deref(), Some("Backfill resumed"));
    assert!(backfill::status(&client).await.unwrap().line().starts_with("backfill running (resumed "));
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, "2025-12-19", "2025-12-19", 1).await.unwrap();
    assert_eq!(counted(&keda, "DAY_BLOCK", &build, Some(false)).await, 1);
    // and claimable blocks with no pod are a stall again
    for m in [70, 90] {
        let at = later(&client, m).await;
        watch::check(&mut client, &s, &at).await.unwrap();
    }
    assert!(open(&client, "STALLED").await.iter().any(|p| p.starts_with("Day blocks stalled")), "{:?}", open(&client, "STALLED").await);
    watch::check(&mut client, &s, &at8).await.unwrap();
    assert_eq!(count(&client, reruns).await, 1, "the re-run queued again");
    scratch.drop().await;
}

/// C-088 (2026-11-26: core down 20 minutes, the daily block's lease ended for the heartbeats core did not
/// answer, about 3 h lost): a lease whose expiry passed while core was down gets the lease period from
/// core's start; its worker's next heartbeat renews it and the sweep ends nothing. A lease with longer to
/// run is not shortened. Without it the heartbeat is refused and the sweep ends the lease.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn core_downtime_costs_no_lease() {
    let (scratch, mut client, build) = ready("backfill_grace").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, "2025-12-11", 1).await.unwrap();
    let daily = work::claim(&mut client, Kind::DayBlock, &build, "day-block-daily-1", 300.0).await.unwrap().unwrap();
    let long = work::claim(&mut client, Kind::DayBlock, &build, "day-block-1", 4.0 * HOUR).await.unwrap().unwrap();
    // core was down 20 minutes: the daily block's last renewal ran out 15 minutes ago
    client.execute("UPDATE core.lease SET expires_at = now() - interval '15 minutes' WHERE job_id = $1", &[&daily.job_id]).await.unwrap();
    let long_expiry = "SELECT expires_at::text FROM core.lease WHERE job_id = $1";
    let before: String = client.query_one(long_expiry, &[&long.job_id]).await.unwrap().get(0);
    assert!(matches!(work::heartbeat(&mut client, &daily.token, 300.0).await, Err(LeaseError::Gone)), "the heartbeat refused, as on 2026-11-26");

    let lapsed = work::grace_on_start(&mut client, 300.0).await.unwrap();
    let lease: i64 = client.query_one("SELECT lease_id FROM core.lease WHERE job_id = $1", &[&daily.job_id]).await.unwrap().get(0);
    assert_eq!(lapsed, [lease]);
    assert_eq!(work::sweep(&mut client).await.unwrap(), 0, "the sweep ends nothing");
    assert!(work::heartbeat(&mut client, &daily.token, 300.0).await.is_ok(), "the worker renews");
    assert_eq!(client.query_one(long_expiry, &[&long.job_id]).await.unwrap().get::<_, String>(0), before, "a longer lease is not shortened");
    let logged: String = client.query_one("SELECT message FROM core.notice ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert_eq!(logged, format!("Core started with 1 live leases past their expiry (core was not answering their heartbeats): each given 300 s to renew, leases {lease}."));
    assert_eq!(progress::short_notice(&logged), None, "housekeeping, kept by the log");

    // a worker really gone is found one lease period later
    client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1", &[&daily.job_id]).await.unwrap();
    assert_eq!(work::sweep(&mut client).await.unwrap(), 1);
    scratch.drop().await;
}

/// C-089 (owner, 2026-11-26: the nightly is pilot-critical): a nightly session's job whose attempt ends
/// unfinished and is queued again (its lease lost, as at 13:45 MT that day, or a failed attempt) is posted on
/// the alert channel at once; a backfill block's retry stays in the log.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_nightly_run_that_restarts_is_said_at_once() {
    let (scratch, mut client, build) = ready("backfill_nightly").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, "2025-12-11", 1).await.unwrap();
    client.execute("UPDATE core.job SET priority = 1 WHERE first_session = $1::text::date", &[&DAY]).await.unwrap();
    let posts = "SELECT message FROM core.notice WHERE post ORDER BY notice_id";
    let posted = async |c: &Client| c.query(posts, &[]).await.unwrap().iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>();
    let nightly = work::claim(&mut client, Kind::DayBlock, &build, "day-block-daily-1", 300.0).await.unwrap().unwrap();
    let backfill = work::claim(&mut client, Kind::DayBlock, &build, "day-block-1", 300.0).await.unwrap().unwrap();
    assert_eq!(nightly.sessions[0], DAY);
    client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second'", &[]).await.unwrap();
    assert_eq!(work::sweep(&mut client).await.unwrap(), 2);
    assert_eq!(
        posted(&client).await,
        ["Nightly run of Dec 10 stopped before it finished (its lease expired); it starts again by itself. If it cannot finish, the day's deadline alert follows."],
        "the nightly's lost lease only"
    );
    // a failed attempt with attempts left: said too
    let again = work::claim(&mut client, Kind::DayBlock, &build, "day-block-daily-2", 300.0).await.unwrap().unwrap();
    assert_eq!(again.job_id, nightly.job_id);
    assert!(work::fail(&mut client, &again.token, "fill", "fill: the provider refused", "log").await.unwrap());
    let said = posted(&client).await;
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].starts_with("Nightly run of Dec 10 stopped before it finished (fill: fill: the provider refused)"), "{said:?}");
    let _ = backfill;
    scratch.drop().await;
}
