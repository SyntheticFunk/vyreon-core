//! The job queue on real sessions (the engine's calendar at engine-v1, `fixtures/engine-v1-sessions.json`),
//! the real builds of the droplet's 2025-12-10 and, for the ordered pass, that day's slice
//! published. Run with `cargo test -- --ignored`.
use test_support::Scratch;
use test_support::slice::{BASE, DERIVED, bundle, day_local_parts, inventories, parts, reading, with_day_local};
use core_db::publish::{self, Retired, publish_file, publish_for_lease};
use core_db::work::{self, Claim, Kind, LeaseError};
use core_db::{build, migrate, universe, watch};
use serde_json::{Value, json};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// A migrated database with the calendar and the slice's two builds; returns their ids (base, derived).
/// The base build publishes each day's day-local revision as well (C-031), as a day block does; the
/// derived build registers the profiles its pass reads (`reading`).
async fn ready(test: &str) -> (Scratch, Client, String, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    let calendar: Value = serde_json::from_str(test_support::CALENDAR).unwrap();
    assert_eq!(work::load_calendar(&mut client, &calendar).await.unwrap(), 2261);
    // a published day is checked against the universe's members listed that day
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for (i, inventory) in inventories().into_iter().enumerate() {
        let inventory = if i == 0 { with_day_local(inventory) } else { reading(inventory) };
        builds.push(build::register(&mut client, &inventory).await.unwrap().engine_build);
    }
    let derived = builds.pop().unwrap();
    (scratch, client, builds.pop().unwrap(), derived)
}

async fn admit(client: &Client, build: &str) {
    build::admit(client, build, &json!({"checklist": "queue test", "by": "cargo test"})).await.unwrap();
}

/// Marks the claim's next session done, as publishing its day does.
async fn done(client: &mut Client, claim: &Claim, session: &str) -> Result<(), LeaseError> {
    let tx = client.transaction().await.unwrap();
    let held = work::held(&tx, &claim.token).await?;
    work::session_done(&tx, &held, session).await?;
    tx.commit().await.unwrap();
    Ok(())
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn ordered(client: &mut Client, build: &str) -> Option<Claim> {
    work::claim(client, Kind::OrderedPass, build, "pass", HOUR).await.unwrap()
}

async fn expire(client: &Client, claim: &Claim) {
    client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1 AND ended_at IS NULL", &[&claim.job_id]).await.unwrap();
}

/// A bundle file name no other test of this process uses at the same time.
fn unique(name: &str) -> String {
    static SENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!("{name}-{}", SENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// `day`'s day-local revision on base revision `base`, by `build`: under the block's lease (`token`),
/// as a day block sends it after the base (C-031), or by hand naming its base. Returns its revision.
async fn day_local(client: &mut Client, build: &str, day: &str, base: i64, token: Option<&str>) -> i64 {
    let (h, s, f) = day_local_parts(build, day, base);
    let path = bundle(&unique(&format!("day-local-{day}")), &h, &s, &f);
    let published = match token {
        Some(token) => publish_for_lease(client, &path, token).await.unwrap(),
        None => publish_file(client, &path).await.unwrap(),
    };
    std::fs::remove_file(&path).unwrap();
    published.revision_id
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn work_is_scheduled_over_sessions_and_claimed_in_order() {
    let (scratch, mut client, b, _) = ready("queue").await;
    // the list only grows
    let shorter = json!({"calendar_sha256": "x", "sessions": ["2025-12-10"]});
    assert!(work::load_calendar(&mut client, &shorter).await.unwrap_err().contains("2260 stored sessions are not in the new list"));

    let tapes = work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-10", "2025-12-12", 1).await.unwrap();
    assert_eq!((tapes.jobs, tapes.sessions, tapes.partitions), (3, 3, 0));
    // 2025-12-06 and 07 are a weekend: five sessions from the 6th to the 12th, in blocks of two
    let blocks = work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-06", "2025-12-12", 2).await.unwrap();
    assert_eq!((blocks.jobs, blocks.sessions, blocks.partitions), (3, 5, 4));
    let ranges: Vec<(String, String)> = client
        .query("SELECT first_session::text, last_session::text FROM core.job WHERE kind = 'DAY_BLOCK' ORDER BY 1", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let range = |a: &str, z: &str| (a.to_owned(), z.to_owned());
    assert_eq!(ranges, [range("2025-12-08", "2025-12-09"), range("2025-12-10", "2025-12-11"), range("2025-12-12", "2025-12-12")]);
    for (first, last, why) in [("2025-12-12", "2025-12-16", "overlaps unfinished DAY_BLOCK"), ("2025-12-13", "2025-12-14", "no trading session"), ("2029-01-02", "2029-01-05", "no trading session")] {
        let refused = work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, first, last, 2).await.unwrap_err();
        assert!(refused.contains(why), "{refused}");
    }
    assert!(work::schedule(&mut client, Kind::Tape, PROFILE, "nobody", "2025-12-15", "2025-12-15", 1).await.unwrap_err().contains("not registered"));

    // nothing is claimable until the build is admitted, and then only by that build's workers
    assert_eq!(work::claim(&mut client, Kind::Tape, &b, "w1", HOUR).await.unwrap(), None);
    admit(&client, &b).await;
    assert_eq!(work::claim(&mut client, Kind::Tape, "another-build", "w1", HOUR).await.unwrap(), None);
    assert_eq!(work::status(&client).await.unwrap()["claimable"], json!({"TAPE": 3, "DAY_BLOCK": 1}));

    // the earliest tape first; a block waits for every tape up to its last session
    let tape10 = work::claim(&mut client, Kind::Tape, &b, "w1", HOUR).await.unwrap().unwrap();
    assert_eq!((tape10.sessions.as_slice(), tape10.attempt, tape10.token.len()), (&["2025-12-10".to_owned()][..], 1, 64));
    let first_block = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!(first_block.sessions, ["2025-12-08", "2025-12-09"]);
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap(), None);
    let tape11 = work::claim(&mut client, Kind::Tape, &b, "w3", HOUR).await.unwrap().unwrap();

    // a job is finished only when its sessions are done, in order, under a live lease
    assert!(matches!(work::finish(&mut client, &tape10.token).await, Err(LeaseError::Refused(m)) if m.contains("2025-12-10 to 2025-12-10 remain")));
    assert!(matches!(done(&mut client, &first_block, "2025-12-09").await, Err(LeaseError::Refused(m)) if m.contains("next session is 2025-12-08")));
    assert_eq!(work::heartbeat(&mut client, "not-a-token", HOUR).await, Err(LeaseError::Gone));
    assert!(!work::heartbeat(&mut client, &tape10.token, HOUR).await.unwrap().1);
    for tape in [&tape10, &tape11] {
        done(&mut client, tape, &tape.sessions[0]).await.unwrap();
        work::finish(&mut client, &tape.token).await.unwrap();
    }
    // the lease is over; a finish sent again is answered as done
    assert_eq!(work::heartbeat(&mut client, &tape10.token, HOUR).await, Err(LeaseError::Gone));
    assert_eq!(work::finish(&mut client, &tape10.token).await, Ok(tape10.job_id));
    // tapes for the 10th and 11th are in: that block is free, the 12th's still waits for its tape
    let second_block = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!(second_block.sessions, ["2025-12-10", "2025-12-11"]);
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap(), None);
    assert_eq!(work::status(&client).await.unwrap()["live_leases"], 2);
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_lost_workers_block_resumes_and_a_failed_day_is_set_aside() {
    let (scratch, mut client, b, _) = ready("leases").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-12", 30).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-15", "2025-12-16", 30).await.unwrap();

    // the first worker publishes the 10th and is lost: the block is claimed again from the 11th
    let first = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    done(&mut client, &first, "2025-12-10").await.unwrap();
    expire(&client, &first).await;
    assert_eq!(work::heartbeat(&mut client, &first.token, HOUR).await, Err(LeaseError::Gone));
    assert!(matches!(done(&mut client, &first, "2025-12-11").await, Err(LeaseError::Gone)));
    // C-068 (C-035 amended): a lost lease with no stop is not an attempt, the first two on a day
    let second = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!((second.job_id, second.sessions.clone(), second.attempt), (first.job_id, vec!["2025-12-11".to_owned(), "2025-12-12".to_owned()], 1));

    // a worker that fails says where and sends its log; the job is tried again, three times in all
    assert!(work::fail(&mut client, &second.token, "surfaces", "surface fit did not converge for XLB", "…the day's log…").await.unwrap());
    let third = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!((third.job_id, third.attempt), (first.job_id, 2));
    assert!(work::fail(&mut client, &third.token, "intent B", "worker ran out of disk", "log").await.unwrap());
    let fourth = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!((fourth.job_id, fourth.attempt), (first.job_id, 3));
    // the third failure is the last: the 11th is set aside, the 10th's job is done, the 12th is
    // queued as a block of its own, and the queue goes on (C-035)
    assert!(!work::fail(&mut client, &fourth.token, "intent B", "worker ran out of disk", "log").await.unwrap());
    assert_eq!(work::status(&client).await.unwrap()["paused"], false);
    type Job = (String, String, String, i64);
    let jobs: Vec<Job> = client
        .query("SELECT first_session::text, last_session::text, state, job_id FROM core.job WHERE first_session < '2025-12-15' ORDER BY first_session", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect();
    let range = |j: &Job| (j.0.clone(), j.1.clone(), j.2.clone());
    let own = |d: &str, state: &str| (d.to_owned(), d.to_owned(), state.to_owned());
    assert_eq!(jobs.iter().map(range).collect::<Vec<_>>(), [own("2025-12-10", "DONE"), own("2025-12-11", "FAILED"), own("2025-12-12", "QUEUED")]);
    assert_eq!(jobs[1].3, first.job_id, "the failed day keeps the job and its leases");
    type Ended = (String, Option<String>, Option<String>, Option<String>);
    let kept: Vec<Ended> = client
        .query("SELECT outcome, failed_stage, failure, failure_log FROM core.lease ORDER BY lease_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect();
    assert_eq!(kept[0], ("EXPIRED".to_owned(), None, Some("the lease was not renewed".to_owned()), None));
    assert_eq!(kept[1], ("FAILED".to_owned(), Some("surfaces".to_owned()), Some("surface fit did not converge for XLB".to_owned()), Some("…the day's log…".to_owned())));
    let rest = work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap().unwrap();
    assert_eq!((rest.sessions.clone(), rest.attempt), (vec!["2025-12-12".to_owned()], 1));

    // a lease that expires on the last attempt sets its day aside too, once the day has lost two
    // leases free of charge; the third failed day stops the queue
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE state <> 'FAILED'", &[]).await.unwrap();
    let mut rest = rest;
    for free in 1..=work::LOST_LEASES_FREE {
        expire(&client, &rest).await;
        assert_eq!(work::sweep(&mut client).await.unwrap(), 1);
        assert_eq!(job_state(&client, rest.job_id).await, "QUEUED", "lost lease {free} on the 12th is free");
        rest = work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap().unwrap();
        assert_eq!((rest.sessions.clone(), rest.attempt), (vec!["2025-12-12".to_owned()], 1));
    }
    expire(&client, &rest).await;
    assert_eq!(work::sweep(&mut client).await.unwrap(), 1);
    assert_eq!(job_state(&client, rest.job_id).await, "FAILED", "the third lost lease on the 12th is charged");
    assert_eq!(work::status(&client).await.unwrap()["paused"], false);
    let other = work::claim(&mut client, Kind::DayBlock, &b, "w5", HOUR).await.unwrap().unwrap();
    assert_eq!(other.sessions, ["2025-12-15", "2025-12-16"]);
    assert!(!work::fail(&mut client, &other.token, "plan", "an input is missing", "log").await.unwrap());
    let status = work::status(&client).await.unwrap();
    assert_eq!(status["paused"], true);
    assert_eq!(status["pause_reason"], format!("3 days stand failed; the last, job {} (2025-12-15): plan: an input is missing", other.job_id));
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w6", HOUR).await.unwrap(), None, "the 16th is queued but nothing is claimed while paused");

    // once someone has looked: the first reason is reported, and the failed days start afresh
    work::pause(&client, "a later reason").await.unwrap();
    assert!(work::resume(&client).await.unwrap().unwrap().contains("3 days stand failed"));
    assert_eq!(work::retry_failed(&client).await.unwrap(), 3);
    let again = work::claim(&mut client, Kind::DayBlock, &b, "w6", HOUR).await.unwrap().unwrap();
    assert_eq!((again.job_id, again.attempt, again.sessions.clone()), (first.job_id, 1, vec!["2025-12-11".to_owned()]));
    // a withdrawn build's jobs are not claimable either
    work::resume(&client).await.unwrap();
    build::withdraw(&mut client, &b, "a wrong number on 2025-12-10").await.unwrap();
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w5", HOUR).await.unwrap(), None);
    scratch.drop().await;
}

/// Audit 2026-11-22 opsurface round 2 (round2-core.md) 9, C-035 amended: two failures charged on the
/// 10th, then a third lease finishes the 10th and fails on the 11th: the block's three attempts were
/// spent, so the 11th was set aside on its first failure. A block's attempts are now its next day's:
/// the lease that moves it on is that day's first, and the 11th is tried three times.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_days_attempts_are_its_own() {
    let (scratch, mut client, b, _) = ready("c068_day_attempts").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 30).await.unwrap();
    for _ in 0..2 {
        let lease = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        assert!(work::fail(&mut client, &lease.token, "day 2025-12-10", "worker ran out of disk", "log").await.unwrap());
    }
    let third = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    assert_eq!(third.attempt, 3);
    publish_day(&mut client, &third, "2025-12-10").await;
    assert!(work::fail(&mut client, &third.token, "day 2025-12-11", "worker ran out of disk", "log").await.unwrap(), "the 11th's first failure");
    assert_eq!(block_state(&client).await, ("QUEUED".to_owned(), 1));
    for attempt in 2..=3 {
        let lease = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
        assert_eq!((lease.sessions[0].as_str(), lease.attempt), ("2025-12-11", attempt));
        assert_eq!(work::fail(&mut client, &lease.token, "day 2025-12-11", "worker ran out of disk", "log").await.unwrap(), attempt < 3);
    }
    assert_eq!(cause(&client, "2025-12-11").await.1, "worker ran out of disk", "set aside after its own three attempts");
    scratch.drop().await;
}

/// The first day block's state and attempts.
async fn block_state(client: &Client) -> (String, i32) {
    let r = client.query_one("SELECT state, attempts FROM core.job WHERE kind = 'DAY_BLOCK' ORDER BY job_id LIMIT 1", &[]).await.unwrap();
    (r.get(0), r.get(1))
}

/// Audit 2026-11-22 opsurface (ops.md) 13, C-035 amended: two leases lost on the 10th, then a third
/// that finishes the 10th and is lost on the 11th, used to set the 11th aside on its first try (the
/// block's three attempts were spent). A lease lost without a stop is now free twice per day: the
/// 11th is queued again with no attempt counted. A day that keeps killing its node still stands
/// FAILED, after `LOST_LEASES_FREE` + `max_attempts` lost leases; a lease that left its evidence
/// before it was lost is a stop, charged.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_lost_node_never_sets_a_day_aside_on_its_first_loss() {
    let (scratch, mut client, b, _) = ready("c068_lost").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-12", 30).await.unwrap();
    for _ in 0..2 {
        let lost = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        assert_eq!((lost.sessions[0].as_str(), lost.attempt), ("2025-12-10", 1));
        expire(&client, &lost).await;
        work::sweep(&mut client).await.unwrap();
        assert_eq!(block_state(&client).await, ("QUEUED".to_owned(), 0));
    }
    let third = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &third, "2025-12-10").await;
    expire(&client, &third).await;
    work::sweep(&mut client).await.unwrap();
    assert_eq!(block_state(&client).await, ("QUEUED".to_owned(), 0), "the 11th's first lost node sets nothing aside");
    let stopped: Vec<Option<String>> =
        client.query("SELECT stopped_on::text FROM core.lease ORDER BY lease_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(stopped, [Some("2025-12-10".to_owned()), Some("2025-12-10".to_owned()), Some("2025-12-11".to_owned())]);
    assert!(notices(&client).await.last().unwrap().contains("lost its lease on 2025-12-11 without a stop (1 of 2 such losses free on that day)"));

    // the 11th kills every node: its second loss is free, then each is charged until it stands FAILED
    let mut losses = 1;
    let failed = "SELECT count(*) FROM core.job WHERE kind = 'DAY_BLOCK' AND state = 'FAILED' AND first_session = '2025-12-11'";
    while count(&client, failed).await == 0 {
        let lease = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
        assert_eq!(lease.sessions[0], "2025-12-11");
        expire(&client, &lease).await;
        work::sweep(&mut client).await.unwrap();
        losses += 1;
        assert!(losses <= 10, "a day that kills every node is set aside in the end");
    }
    assert_eq!(losses, work::LOST_LEASES_FREE + 3, "two free, then the block's three attempts");

    // a lease that left its evidence and was then lost is a stop: charged at once
    let r = client.query_one("SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND state = 'QUEUED'", &[]).await.unwrap();
    let twelfth: i64 = r.get(0);
    let lease = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!((lease.job_id, lease.sessions[0].as_str()), (twelfth, "2025-12-12"));
    evidence(&mut client, &lease.token, "day 2025-12-12", "stop: 2025-12-12 not published: worker ran out of disk", "2025-12-12").await;
    expire(&client, &lease).await;
    work::sweep(&mut client).await.unwrap();
    let r = client.query_one("SELECT attempts FROM core.job WHERE job_id = $1", &[&twelfth]).await.unwrap();
    assert_eq!(r.get::<_, i32>(0), 1, "explained by its evidence: counted");
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_ordered_pass_follows_the_published_days_one_at_a_time() {
    let (scratch, mut client, b, d) = ready("ordered").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-11", 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, "2025-12-10", "2025-12-11", 1).await.unwrap();
    // nothing is published yet
    assert_eq!(ordered(&mut client, &d).await, None);
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    done(&mut client, &block, "2025-12-09").await.unwrap();
    assert_eq!(ordered(&mut client, &d).await, None, "the 10th's base day is not published");

    // the worker publishes the 10th (the real slice) under its lease and says the day is done,
    // which moves its block on; the ordered pass may then take that day, and only it
    let (header, scalars, files) = parts(BASE);
    let path = bundle("ordered-base", &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    // the same day sent again (its first answer lost on the way) is answered, not written twice
    let again = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    assert_eq!((again.already_published, again.revision_id, again.slots), (true, base.revision_id, 4));
    assert!(!base.already_published);
    // but not one that only resembles it (audit 2026-11-14, core.md 15): one slot four times, or
    // another listed set, goes through the full checks and is refused
    let (header, scalars, files) = parts(BASE);
    let mut repeated = header.clone();
    repeated.slots = vec![header.slots[0].clone(); 4];
    let repeated = bundle("ordered-repeated", &repeated, &scalars, &vec![files[0].clone(); 4]);
    assert!(matches!(publish_for_lease(&mut client, &repeated, &block.token).await, Err(LeaseError::Refused(m)) if m.contains("is listed twice")));
    std::fs::remove_file(&repeated).unwrap();
    let mut fewer = header.clone();
    fewer.listed_members.as_mut().unwrap().retain(|k| k != "ETHA");
    let fewer = bundle("ordered-fewer", &fewer, &scalars, &files);
    assert!(matches!(publish_for_lease(&mut client, &fewer, &block.token).await, Err(LeaseError::Refused(m)) if m.contains("disagree about the members")));
    std::fs::remove_file(&fewer).unwrap();
    // the block is still on the 10th until its worker says the day is done (its day-local
    // revision comes after the base): the pass of the 10th waits for that (C-039)
    assert_eq!(ordered(&mut client, &d).await, None, "the block is still on the 10th");
    // C-070 (audit 2026-11-23, Codex 1, Gemini 2 and 11): nor is the day done on its base alone
    assert!(matches!(work::day_done(&mut client, &block.token, "2025-12-10").await, Err(LeaseError::Refused(m)) if m.contains("no day-local revision on it")));
    day_local(&mut client, &b, "2025-12-10", base.revision_id, Some(&block.token)).await;
    // the block's day is done when its worker says so, and not before its base is published
    assert!(matches!(work::day_done(&mut client, &block.token, "2025-12-11").await, Err(LeaseError::Refused(m)) if m.contains("has not published 2025-12-11")));
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    // said twice (the first answer lost), it is answered and changes nothing
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    // but another day's bundle, or this day with another body, is not the block's next day
    let (mut other, scalars_again, mut changed) = parts(BASE);
    other.represented_date = "2025-12-09".into();
    let wrong_day = bundle("ordered-wrong-day", &other, &scalars_again, &changed);
    assert!(matches!(publish_for_lease(&mut client, &wrong_day, &block.token).await, Err(LeaseError::Refused(m)) if m.contains("expects 2025-12-11")));
    std::fs::remove_file(&wrong_day).unwrap();
    let (mut edited, _, _) = parts(BASE);
    changed[0].receipt.push(b' ');
    changed[0].body.insert(0, b' ');
    edited.slots[0].body_sha256 = core_contract::sha256_hex(&changed[0].body);
    edited.slots[0].receipt_sha256 = core_contract::sha256_hex(&changed[0].receipt);
    let other_body = bundle("ordered-other-body", &edited, &scalars_again, &changed);
    assert!(matches!(publish_for_lease(&mut client, &other_body, &block.token).await, Err(LeaseError::Refused(m)) if m.contains("expects 2025-12-11")));
    std::fs::remove_file(&other_body).unwrap();
    assert_eq!(publish_for_lease(&mut client, &path, "not-a-token").await, Err(LeaseError::Gone));
    let tenth = ordered(&mut client, &d).await.unwrap();
    assert_eq!(tenth.sessions, ["2025-12-10"]);
    assert_eq!(ordered(&mut client, &d).await, None, "one at a time, and the 11th is not published");
    // a lease publishes only its own job's profile and build
    assert!(matches!(publish_for_lease(&mut client, &path, &tenth.token).await, Err(LeaseError::Refused(m)) if m.contains("from build")));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, 2, "the base and its day-local revision: the refused bundles left nothing");

    // a pass out of attempts stays failed without stopping the queue: the day block goes on,
    // and no pass is handed out behind the failed one until it is retried
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE job_id = $1", &[&tenth.job_id]).await.unwrap();
    assert!(!work::fail(&mut client, &tenth.token, "ordered pass", "VS-O02 absent", "log").await.unwrap());
    assert_eq!(work::status(&client).await.unwrap()["paused"], false);
    assert_eq!(ordered(&mut client, &d).await, None);
    assert!(work::heartbeat(&mut client, &block.token, HOUR).await.is_ok());
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    let tenth = ordered(&mut client, &d).await.unwrap();

    // it publishes its revision on the base it read, and is done
    let (mut dh, ds, df) = parts(DERIVED);
    dh.base_revision = Some(base.revision_id);
    let path = bundle("ordered-derived", &dh, &ds, &df);
    publish_for_lease(&mut client, &path, &tenth.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::finish(&mut client, &tenth.token).await.unwrap();
    assert_eq!(ordered(&mut client, &d).await, None, "the 11th still waits for its base day");
    assert_eq!(work::status(&client).await.unwrap()["claimable"], json!({}));

    scratch.drop().await;
}

async fn notices(client: &Client) -> Vec<String> {
    client.query("SELECT message FROM core.notice WHERE NOT post ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
}

async fn pass_jobs(client: &Client) -> Vec<(String, String, i32)> {
    client
        .query("SELECT first_session::text, state, attempts FROM core.job WHERE kind = 'ORDERED_PASS' ORDER BY first_session", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
}

/// The derived revision of `day` computed from `base`, sent under a lease.
async fn derive(client: &mut Client, day: &str, base: i64, token: &str) -> Result<i64, LeaseError> {
    let (mut h, s, f) = parts(DERIVED);
    (h.represented_date, h.base_revision) = (day.into(), Some(base));
    let path = bundle(&unique(&format!("derive-{day}-{base}")), &h, &s, &f);
    let published = publish_for_lease(client, &path, token).await;
    std::fs::remove_file(&path).unwrap();
    published.map(|p| p.revision_id)
}

/// Audit 2026-11-14 (core.md) 1, 2 and 10: a day published again supersedes what read it, and the
/// ordered pass derives that again instead of leaving it DONE with nothing published; a pass
/// running on an affected day is stopped, and a pass whose lease predates a publish of a day it
/// reads is refused. The slice's real bodies of 2025-12-10 are sent under the 11th as well: core's
/// checks never read a body's date.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_published_again_takes_down_what_read_it_and_the_pass_derives_it_again() {
    let (scratch, mut client, b, d) = ready("rederive").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let mut bases = Vec::new();
    for day in days {
        let (mut h, s, f) = parts(BASE);
        h.represented_date = day.into();
        let path = bundle(&format!("rederive-base-{day}"), &h, &s, &f);
        bases.push(publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id);
        std::fs::remove_file(&path).unwrap();
        day_local(&mut client, &b, day, *bases.last().unwrap(), Some(&block.token)).await;
        work::day_done(&mut client, &block.token, day).await.unwrap();
    }
    work::finish(&mut client, &block.token).await.unwrap();
    for (day, base) in days.iter().zip(&bases) {
        let pass = ordered(&mut client, &d).await.unwrap();
        assert_eq!(pass.sessions, [*day]);
        derive(&mut client, day, *base, &pass.token).await.unwrap();
        work::finish(&mut client, &pass.token).await.unwrap();
    }
    let done = |state: &str, attempts: i32| days.iter().map(|d| (d.to_string(), state.to_owned(), attempts)).collect::<Vec<_>>();
    assert_eq!(pass_jobs(&client).await, done("DONE", 1));

    // the 10th's base is published again: the 10th's day-local and derived revisions were computed
    // from the old base and the 11th's derived revision read the 10th, so they fall, and both
    // passes are queued again; the 10th's waits for its day-local revision on the new base (sent
    // by hand at once, as the day block would)
    let (h, s, f) = parts(BASE);
    let again = publish_file(&mut client, &bundle("rederive-again", &h, &s, &f)).await.unwrap();
    assert_eq!(again.superseded, 4, "the old base, the 10th's day-local and derived revisions and the 11th's derived one");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local").await, 0);
    assert_eq!(pass_jobs(&client).await, done("QUEUED", 0));
    assert_eq!(claimable_passes(&client).await.split(' ').next(), Some("2025-12-11"), "the 10th's waits for a day-local revision on its new base");
    day_local(&mut client, &b, days[0], again.revision_id, None).await;
    let said = notices(&client).await;
    assert!(said.last().unwrap().contains("the ordered pass derives 2 days again, 2025-12-10 to 2025-12-11"), "{said:?}");

    // the 10th first, the 11th behind it; the base changes again while the 10th's pass runs: the
    // pass is stopped (its lease is gone) and what it computed on the old base is not taken
    let tenth = ordered(&mut client, &d).await.unwrap();
    assert_eq!((tenth.sessions.as_slice(), ordered(&mut client, &d).await), (&["2025-12-10".to_owned()][..], None));
    let third = publish_file(&mut client, &bundle("rederive-third", &h, &s, &f)).await.unwrap();
    assert_eq!(derive(&mut client, days[0], again.revision_id, &tenth.token).await, Err(LeaseError::Gone));
    let third_local = day_local(&mut client, &b, days[0], third.revision_id, None).await;
    assert_eq!(count(&client, &format!("SELECT count(*) FROM core.lease WHERE job_id = {} AND outcome = 'CANCELLED'", tenth.job_id)).await, 1);
    assert_eq!(pass_jobs(&client).await, done("QUEUED", 0), "the stopped attempt is not counted against the pass");

    // and if a publish of a day it reads came after its lease was granted without stopping it, the
    // pass is refused all the same (made here by moving the grant back)
    let tenth = ordered(&mut client, &d).await.unwrap();
    client.execute("UPDATE core.lease SET granted_at = granted_at - interval '1 hour' WHERE job_id = $1 AND ended_at IS NULL", &[&tenth.job_id]).await.unwrap();
    let refused = derive(&mut client, days[0], third.revision_id, &tenth.token).await.unwrap_err();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.contains(&format!("revision {third_local} of 2025-12-10 was published after job {}", tenth.job_id))), "{refused}");
    client.execute("UPDATE core.lease SET granted_at = clock_timestamp() WHERE job_id = $1 AND ended_at IS NULL", &[&tenth.job_id]).await.unwrap();
    derive(&mut client, days[0], third.revision_id, &tenth.token).await.unwrap();
    work::finish(&mut client, &tenth.token).await.unwrap();
    let eleventh = ordered(&mut client, &d).await.unwrap();
    derive(&mut client, days[1], bases[1], &eleventh.token).await.unwrap();
    work::finish(&mut client, &eleventh.token).await.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 6, "two bases, their two day-local and two derived revisions");
    scratch.drop().await;
}

/// When a build schedules its ordered passes (C-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passes {
    /// every session's pass, once, before the first day block publishes
    AtStart,
    /// every session's pass, once, after the last day block is done
    AfterBlocks,
    /// each wave's passes with its blocks: the shape that re-runs the later passes
    WithEachWave,
}

/// The published base revision of `day`.
async fn base_of(client: &Client, day: &str) -> i64 {
    client
        .query_one("SELECT revision_id FROM vyreon.day_revision WHERE represented_date = $1::text::date AND state = 'PUBLISHED' AND base_revision IS NULL", &[&day])
        .await
        .unwrap()
        .get(0)
}

/// A pass worker that runs whatever ordered pass is claimable, at once, until none is: the
/// earliest a pass can run, so the most it can be run again.
async fn drain_passes(client: &mut Client, d: &str) {
    while let Some(pass) = ordered(client, d).await {
        let day = pass.sessions[0].clone();
        let base = base_of(client, &day).await;
        derive(client, &day, base, &pass.token).await.unwrap();
        work::finish(client, &pass.token).await.unwrap();
    }
}

/// C-046: a build newest first over `sessions` (oldest first), in blocks of `block`, on `nodes`
/// workers: each wave schedules the newest `nodes` blocks not yet scheduled (the claim takes the
/// earliest claimable block, so newest first is the order of scheduling), the nodes claim them and
/// publish their days in turn, and after every publish and every day done a pass worker runs
/// every claimable pass. Returns how many times an ordered pass ran (its leases).
async fn newest_first_build(client: &mut Client, b: &str, d: &str, sessions: &[String], block: usize, nodes: usize, passes: Passes) -> i64 {
    let (first, last) = (sessions[0].as_str(), sessions[sessions.len() - 1].as_str());
    if passes == Passes::AtStart {
        work::schedule(client, Kind::OrderedPass, PROFILE, d, first, last, 1).await.unwrap();
    }
    // blocks counted from the newest session, newest block first
    let blocks: Vec<&[String]> = sessions.rchunks(block).collect();
    for wave in blocks.chunks(nodes) {
        for range in wave {
            work::schedule(client, Kind::DayBlock, PROFILE, b, &range[0], &range[range.len() - 1], block as i32).await.unwrap();
        }
        if passes == Passes::WithEachWave {
            let (oldest, newest) = (&wave[wave.len() - 1][0], wave[0].last().unwrap());
            work::schedule(client, Kind::OrderedPass, PROFILE, d, oldest, newest, 1).await.unwrap();
        }
        let mut held = Vec::new();
        for node in 0..wave.len() {
            held.push(work::claim(client, Kind::DayBlock, b, &format!("node{node}"), HOUR).await.unwrap().unwrap());
        }
        for k in 0..block {
            for claim in held.iter().filter(|c| k < c.sessions.len()) {
                let day = &claim.sessions[k];
                let (mut h, s, f) = parts(BASE);
                h.represented_date = day.clone();
                let path = bundle(&format!("newest-first-{day}"), &h, &s, &f);
                let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
                std::fs::remove_file(&path).unwrap();
                drain_passes(client, d).await;
                day_local(client, b, day, base, Some(&claim.token)).await;
                drain_passes(client, d).await;
                work::day_done(client, &claim.token, day).await.unwrap();
                drain_passes(client, d).await;
            }
        }
        for claim in &held {
            work::finish(client, &claim.token).await.unwrap();
        }
        drain_passes(client, d).await;
    }
    if passes == Passes::AfterBlocks {
        work::schedule(client, Kind::OrderedPass, PROFILE, d, first, last, 1).await.unwrap();
        drain_passes(client, d).await;
    }
    // every day ends with its base and one derived revision on it, and every pass done
    assert_eq!(count(client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local").await, sessions.len() as i64);
    assert_eq!(count(client, "SELECT count(*) FROM core.job WHERE kind = 'ORDERED_PASS' AND state <> 'DONE'").await, 0);
    count(client, "SELECT count(*) FROM core.lease JOIN core.job USING (job_id) WHERE kind = 'ORDERED_PASS'").await
}

/// C-046 (the verifier's question before the build): a day published newest first takes down the
/// derived revisions of every later day and queues their passes again (C-037), so a pass that ran
/// before an earlier day's first publish runs again. The claim rule (0005, 0014: a pass waits for
/// its day's base, for every earlier pass and for every unfinished block at or before its day)
/// holds every pass of a range whose passes are all scheduled before its first publish: the
/// earliest pass waits for the earliest day, which a newest-first build publishes last. So each
/// pass runs once whether the passes are scheduled at the start or after the blocks; scheduled
/// with each wave, the later passes run again after every earlier wave (quadratic). It rests on a
/// day whose block is not scheduled yet holding the later passes (C-062 keeps it so: with only the
/// published and pending days holding, AtStart measured 18 runs). Twelve real
/// sessions to 2025-12-12 (the slice's bodies under each date: core's checks never read a body's
/// date), blocks of three; counts and orders are simulated, no market value.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_newest_first_build_runs_each_ordered_pass_once() {
    for (passes, nodes, runs) in [(Passes::AtStart, 2, 12), (Passes::AfterBlocks, 2, 12), (Passes::WithEachWave, 1, 3 + 6 + 9 + 12)] {
        let (scratch, mut client, b, d) = ready(&format!("newest_first_{}", format!("{passes:?}").to_lowercase())).await;
        admit(&client, &b).await;
        admit(&client, &d).await;
        let sessions: Vec<String> = client
            .query("SELECT session::text FROM (SELECT session FROM core.session WHERE session <= '2025-12-12' ORDER BY session DESC LIMIT 12) s ORDER BY 1", &[])
            .await
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect();
        assert_eq!((sessions[0].as_str(), sessions.len()), ("2025-11-26", 12));
        assert_eq!(newest_first_build(&mut client, &b, &d, &sessions, 3, nodes, passes).await, runs, "{passes:?} on {nodes} nodes");
        scratch.drop().await;
    }
}

/// One build registered with both of the slice's real inventories, base and derived, as the
/// cluster's image is one build for every profile. The slice's bodies are sent under its name: core's
/// checks never read which build computed a body.
async fn whole_build(client: &mut Client) -> String {
    let slots: Vec<Value> = inventories().iter().flat_map(|i| i["slots"].as_array().unwrap().clone()).collect();
    let name = "slice-base-and-derived";
    build::register(client, &with_day_local(json!({"engine_build": name, "slots": slots}))).await.unwrap();
    admit(client, name).await;
    name.to_owned()
}

/// `day`'s base, by `build`, under the block's lease; the day is then done.
async fn publish_base_as(client: &mut Client, build: &str, claim: &Claim, day: &str) {
    let (mut h, s, f) = parts(BASE);
    (h.represented_date, h.engine_build) = (day.into(), build.into());
    let path = bundle(&unique(&format!("c062-{day}-{}", claim.job_id)), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    day_local(client, build, day, base, Some(&claim.token)).await;
    work::day_done(client, &claim.token, day).await.unwrap();
}

/// `day`'s base published again by hand (the base build's, as the slice has it), and its day-local
/// revision on it.
async fn base_by_hand(client: &mut Client, day: &str) {
    let (mut h, s, f) = parts(BASE);
    h.represented_date = day.into();
    let path = bundle(&unique(&format!("c062-hand-{day}")), &h, &s, &f);
    let base = publish_file(client, &path).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let build = h.engine_build.clone();
    day_local(client, &build, day, base, None).await;
}

/// Runs `build`'s claimable ordered pass, if any, on its day's published base; returns its day.
async fn run_pass_as(client: &mut Client, build: &str) -> Option<String> {
    let pass = ordered(client, build).await?;
    let day = pass.sessions[0].clone();
    let base = base_of(client, &day).await;
    let (mut h, s, f) = parts(DERIVED);
    (h.represented_date, h.base_revision, h.engine_build) = (day.clone(), Some(base), build.into());
    let path = bundle(&unique(&format!("c062-derive-{day}-{}", pass.job_id)), &h, &s, &f);
    publish_for_lease(client, &path, &pass.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::finish(client, &pass.token).await.unwrap();
    Some(day)
}

/// Each ordered pass job: (day, build, state), by day and job.
async fn passes_by_build(client: &Client) -> Vec<(String, String, String)> {
    client
        .query("SELECT first_session::text, engine_build, state FROM core.job WHERE kind = 'ORDERED_PASS' ORDER BY first_session, job_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
}

/// Audit 2026-11-22 (prebuild surface.md) 6: a publish that supersedes later history revisions used to
/// queue their finished passes again under the builds that ran them, which the new build's workers
/// never claim, and then refused the new build's whole-range schedule for overlap. Now (C-062) the
/// pass goes to the publishing build when it runs and publishes the fallen revision's profile (the
/// earlier build's finished pass stays DONE), the new build's schedule keeps it, and the database
/// refuses a second unfinished pass of a session. As in phase 1: earlier builds' days (2025-12-10
/// and 11), then the new build's first day before its passes are scheduled (2025-12-09).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_superseded_pass_is_queued_under_the_publishing_build_and_its_schedule_keeps_it() {
    let (scratch, mut client, b, d) = ready("c062_publisher").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_base_as(&mut client, &b, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    assert_eq!((run_pass(&mut client, &d).await.as_deref(), run_pass(&mut client, &d).await.as_deref()), (Some(days[0]), Some(days[1])));

    // the new build publishes an earlier day: both later history revisions fall, and their passes
    // are queued under it, not under the earlier build, which still runs but whose image is gone
    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, "2025-12-09", "2025-12-09", 1).await.unwrap();
    let first = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &first, "2025-12-09").await;
    work::finish(&mut client, &first.token).await.unwrap();
    let row = |day: &str, build: &str, state: &str| (day.to_owned(), build.to_owned(), state.to_owned());
    assert_eq!(passes_by_build(&client).await, [row(days[0], &d, "DONE"), row(days[0], &w, "QUEUED"), row(days[1], &d, "DONE"), row(days[1], &w, "QUEUED")]);
    assert_eq!(ordered(&mut client, &d).await, None);
    let said = notices(&client).await;
    assert!(said.iter().any(|n| n.contains("the ordered pass derives 2 days again, 2025-12-10 to 2025-12-11")), "{said:?}");

    // its whole range scheduled now keeps them (it used to be refused for overlap), and the database
    // refuses a second unfinished pass of one session
    let made = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, "2025-12-09", days[1], 1).await.unwrap();
    assert_eq!((made.jobs, made.sessions, made.kept, made.replaced), (1, 3, 2, vec![]));
    let twice = client
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session) VALUES ('ORDERED_PASS', $1, $2, '2025-12-10', '2025-12-10', '2025-12-10')",
            &[&PROFILE, &d],
        )
        .await
        .unwrap_err();
    assert!(twice.to_string().contains("job_one_unfinished_pass") || twice.as_db_error().is_some_and(|e| e.constraint() == Some("job_one_unfinished_pass")), "{twice}");
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &w).await {
        ran.push(day);
    }
    assert_eq!(ran, ["2025-12-09", days[0], days[1]], "each pass once, in date order");
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE kind = 'ORDERED_PASS' AND state <> 'DONE'").await, 0);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 1: a schedule of ordered passes and a publish that
/// queues passes again both read "no unfinished pass" and insert one; under two different locks the
/// second insert hit the unique index, and the publish read that as a refused bundle (the worker's
/// 422 pauses the queue). Now `schedule` waits for the publish lock, and a violation of a core
/// constraint inside a publish is core's failure, never a refusal: shown with a writer that holds the
/// conflicting pass uncommitted while the publish requeues the same day's finished pass.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_schedule_waits_for_a_publish_and_a_queue_conflict_is_no_refused_bundle() {
    let (scratch, mut client, b, d) = ready("c063_schedule_lock").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_day(&mut client, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    while run_pass(&mut client, &d).await.is_some() {}

    // a schedule of passes waits while a publish holds its lock
    let mut publisher = scratch.client().await;
    let held = publisher.transaction().await.unwrap();
    held.execute(work::PUBLISH_LOCK, &[]).await.unwrap();
    let mut scheduler = scratch.client().await;
    let d2 = d.clone();
    let scheduling = tokio::spawn(async move { work::schedule(&mut scheduler, Kind::OrderedPass, PROFILE, &d2, "2025-12-12", "2025-12-12", 1).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!scheduling.is_finished(), "the schedule waits for the publish lock");
    held.commit().await.unwrap();
    scheduling.await.unwrap().unwrap();

    // a pass of the 11th inserted by another writer, not yet committed, while the 10th's base is
    // published again and its fall requeues the 11th's finished pass: the unique index stops the
    // publish, which fails (nothing written) and is not refused
    let writer = publisher.transaction().await.unwrap();
    writer
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session) VALUES ('ORDERED_PASS', $1, $2, '2025-12-11', '2025-12-11', '2025-12-11')",
            &[&PROFILE, &d],
        )
        .await
        .unwrap();
    let mut hand = scratch.client().await;
    let (h, s, f) = parts(BASE);
    let path = bundle(&unique("c063-again"), &h, &s, &f);
    let path2 = path.clone();
    let publishing = tokio::spawn(async move { publish_file(&mut hand, &path2).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    writer.commit().await.unwrap();
    let refused = publishing.await.unwrap().unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&refused, publish::Error::Failed(m) if m.contains("job_one_unfinished_pass")), "{refused}");
    let bases = count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE represented_date = '2025-12-10' AND base_revision IS NULL").await;
    assert_eq!(bases, 1, "nothing of the failed publish was written");
    assert_eq!(work::status(&client).await.unwrap()["paused"], false);
    scratch.drop().await;
}

/// C-062: a pass under a build that does not run (withdrawn) holds no later pass, and the next
/// schedule over its session replaces it; one under another build that runs refuses the schedule.
/// When neither the publishing build (here the base-only build, which does not publish the derived
/// profile) nor the fallen revision's build runs, no pass is queued: the days are named in a notice
/// and a NOT_DERIVED problem, never queued under a build no worker claims.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_pass_under_a_build_that_does_not_run_holds_nothing_and_is_replaced() {
    let (scratch, mut client, b, d) = ready("c062_stranded").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_base_as(&mut client, &b, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    while run_pass(&mut client, &d).await.is_some() {}
    let done: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' ORDER BY first_session", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();

    // the base build publishes the 10th again: it does not publish the derived profile, so the
    // finished passes of the build that does (and runs) are queued again, the same jobs
    base_by_hand(&mut client, days[0]).await;
    let row = |day: &str, build: &str, state: &str| (day.to_owned(), build.to_owned(), state.to_owned());
    assert_eq!(passes_by_build(&client).await, [row(days[0], &d, "QUEUED"), row(days[1], &d, "QUEUED")]);
    let again: Vec<i64> = client.query("SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' ORDER BY first_session", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(again, done);

    // then that build is withdrawn (its image replaced): its queued pass of the 10th holds nothing,
    // so the new build's pass of the 11th may run past it
    build::withdraw(&mut client, &d, "replaced by the whole image").await.unwrap();
    let w = whole_build(&mut client).await;
    work::cancel(&mut client, &done[1..]).await.unwrap();
    let made = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[1], days[1], 1).await.unwrap();
    assert_eq!((made.jobs, made.kept), (1, 0));
    assert_eq!(claimable_passes(&client).await, days[1], "the withdrawn build's pass of the 10th holds nothing");
    // scheduled over the 10th, the new build replaces it and keeps its own of the 11th
    let made = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[0], days[1], 1).await.unwrap();
    assert_eq!((made.jobs, made.kept, made.replaced.clone()), (1, 1, vec![done[0]]));
    assert_eq!(job_state(&client, done[0]).await, "CANCELLED");
    assert!(notices(&client).await.last().unwrap().contains(&format!("jobs {} cancelled", done[0])));
    assert_eq!(claimable_passes(&client).await, days[0], "the new pass of the 10th holds the 11th's");
    // a pass of another build that runs is not replaced: the schedule is refused
    admit(&client, &d).await;
    let refused = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap_err();
    assert!(refused.contains(&format!("of build {w}, a build that runs")), "{refused}");
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &w).await {
        ran.push(day);
    }
    assert_eq!(ran, days);

    // neither the publisher (no derived profile) nor the fallen revisions' build runs: nothing is
    // queued, the days are named and a problem is open
    build::withdraw(&mut client, &w, "a later image").await.unwrap();
    base_by_hand(&mut client, days[0]).await;
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE kind = 'ORDERED_PASS' AND state IN ('QUEUED', 'LEASED', 'FAILED')").await, 0);
    let said = notices(&client).await;
    // (published by hand, the 10th's day-local revision fell with its base and is named too, though
    // `base_by_hand` sends it again at once)
    assert!(said.last().unwrap().contains("no job derives these again, schedule them: 2025-12-10, 2025-12-11, 2025-12-10 (day-local)"), "{said:?}");
    let open: Vec<String> =
        client.query("SELECT summary FROM core.problem WHERE kind = 'NOT_DERIVED' AND resolved_at IS NULL", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert!(open.iter().any(|p| p == "A republish left 3 days that no job derives again: 2025-12-10, 2025-12-10 (day-local), 2025-12-11"), "{open:?}");
    scratch.drop().await;
}

/// A build with the slice's every profile (base, day-local and derived), registered under `name` and
/// admitted: one image that runs tapes, day blocks and ordered passes, as 914ffed7 and its successor.
async fn whole_build_named(client: &mut Client, name: &str) -> String {
    let name = registered_whole_build(client, name).await;
    admit(client, &name).await;
    name
}

/// `whole_build_named`, registered and not admitted (a successor before its hand-over).
async fn registered_whole_build(client: &mut Client, name: &str) -> String {
    let slots: Vec<Value> = inventories().iter().flat_map(|i| i["slots"].as_array().unwrap().clone()).collect();
    build::register(client, &with_day_local(json!({"engine_build": name, "slots": slots}))).await.unwrap();
    name.to_owned()
}

/// Audit 2026-11-22 opsurface (ops.md) 1 and 4, C-068: the rollout withdraws the build that holds the
/// whole range's ordered passes, its queued tapes and blocks. Withdrawn, they held the successor for
/// ever (its blocks wait for every earlier unfinished tape, its passes for the day blocks before
/// them, a schedule over them is refused) and its passes were claimed by no worker. `build hand-over`
/// moves them, once, after a preview whose token it must be given: a failed tape, a block withdrawn
/// mid-way (its done day kept as a DONE job of the old build) and five passes; the successor then
/// runs everything in order.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_withdrawn_builds_unfinished_work_is_handed_to_its_successor_once() {
    let (scratch, mut client, _, _) = ready("c068_hand_over").await;
    let old = whole_build_named(&mut client, "slice-old-image").await;
    let days = ["2025-12-08", "2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];
    work::schedule(&mut client, Kind::Tape, PROFILE, &old, days[2], days[3], 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, days[0], days[4], 2).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &old, days[0], days[4], 1).await.unwrap();
    // the old image publishes the 8th of its first block; the 11th's tape fails for good (the queue
    // pauses on it; the operator resumes)
    let block = work::claim(&mut client, Kind::DayBlock, &old, "w1", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &old, &block, days[0]).await;
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'TAPE' AND first_session = '2025-12-11'", &[]).await.unwrap();
    let t10 = work::claim(&mut client, Kind::Tape, &old, "t1", HOUR).await.unwrap().unwrap();
    let t11 = work::claim(&mut client, Kind::Tape, &old, "t2", HOUR).await.unwrap().unwrap();
    assert_eq!((t10.sessions[0].as_str(), t11.sessions[0].as_str()), (days[2], days[3]));
    done(&mut client, &t10, days[2]).await.unwrap();
    work::finish(&mut client, &t10.token).await.unwrap();
    assert!(!work::fail(&mut client, &t11.token, "tape", "HttpStatus(502)", "log").await.unwrap());
    work::resume(&client).await.unwrap();

    // the image changes. Round 2, 1: the successor is admitted only after the hand-over, so none of its
    // workers runs a later pass before the moved ones; an admitted one is refused
    let new = whole_build_named(&mut client, "slice-new-image").await;
    let refused = build::hand_over(&mut client, &old, &new, "the rulings image", true, None).await.unwrap_err();
    assert!(refused.contains(&format!("build {new} is admitted: hand over before admitting it")), "{refused}");
    build::withdraw(&mut client, &new, "admitted too early").await.unwrap();
    build::withdraw(&mut client, &old, "the rulings image").await.unwrap();
    assert!(build::hand_over(&mut client, &old, &new, "the rulings image", false, None).await.unwrap_err().contains("--preview first"));
    // what the withdrawn work does to the successor without it
    assert_eq!(count(&client, &format!("SELECT count(*) FROM core.claimable_job WHERE engine_build = '{new}'")).await, 0);
    let refused = work::schedule(&mut client, Kind::DayBlock, PROFILE, &new, days[4], days[4], 1).await.unwrap_err();
    assert!(refused.contains("overlaps unfinished DAY_BLOCK"), "{refused}");

    let unfinished = format!("SELECT count(*) FROM core.job WHERE engine_build = '{old}' AND state IN ('QUEUED', 'FAILED')");
    let preview = build::hand_over(&mut client, &old, &new, "the rulings image", true, None).await.unwrap();
    let shown: Vec<(String, String, String, String)> = preview.carried.iter().map(|c| (c.kind.clone(), c.state.clone(), c.first.clone(), c.last.clone())).collect();
    let job = |k: &str, s: &str, a: &str, z: &str| (k.to_owned(), s.to_owned(), a.to_owned(), z.to_owned());
    let mut expected = vec![job("TAPE", "FAILED", days[3], days[3]), job("DAY_BLOCK", "QUEUED", days[1], days[1]), job("DAY_BLOCK", "QUEUED", days[2], days[3]), job("DAY_BLOCK", "QUEUED", days[4], days[4])];
    expected.extend(days.iter().map(|d| job("ORDERED_PASS", "QUEUED", d, d)));
    assert_eq!(shown, expected);
    assert_eq!((preview.hand_over_id, count(&client, &unfinished).await), (None, 9), "a preview changes nothing");
    let refused = build::hand_over(&mut client, &old, &new, "the rulings image", false, Some("0000000000000000")).await.unwrap_err();
    assert!(refused.contains("changed since the preview"), "{refused}");

    let handed = build::hand_over(&mut client, &old, &new, "the rulings image", false, Some(&preview.token)).await.unwrap();
    assert!(handed.hand_over_id.is_some() && handed.carried.iter().all(|c| c.new_job.is_some()));
    assert_eq!(count(&client, &unfinished).await, 0);
    let moved = format!("SELECT count(*) FROM core.job WHERE engine_build = '{new}' AND state = 'QUEUED' AND attempts = 0");
    assert_eq!(count(&client, &moved).await, 9);
    let kept: (String, String, String) = {
        let r = client.query_one(&format!("SELECT first_session::text, last_session::text, state FROM core.job WHERE engine_build = '{old}' AND kind = 'DAY_BLOCK' AND state = 'DONE'"), &[]).await.unwrap();
        (r.get(0), r.get(1), r.get(2))
    };
    assert_eq!(kept, (days[0].to_owned(), days[0].to_owned(), "DONE".to_owned()), "the day the old block did stays its");
    assert!(notices(&client).await.iter().any(|n| n.contains(&format!("is build {new}'s, the old jobs cancelled: the rulings image. Admit build {new} now."))));
    assert!(build::hand_over(&mut client, &old, &new, "again", true, None).await.unwrap_err().contains("no unfinished work"), "once");
    // then the successor is admitted (the hand-over's notice says so)
    admit(&client, &new).await;

    // the successor runs it all: the tape, the blocks, then the passes in date order
    let tape = work::claim(&mut client, Kind::Tape, &new, "t3", HOUR).await.unwrap().unwrap();
    assert_eq!(tape.sessions, [days[3]]);
    done(&mut client, &tape, days[3]).await.unwrap();
    work::finish(&mut client, &tape.token).await.unwrap();
    while let Some(block) = work::claim(&mut client, Kind::DayBlock, &new, "w2", HOUR).await.unwrap() {
        for day in block.sessions.clone() {
            publish_base_as(&mut client, &new, &block, &day).await;
        }
        work::finish(&mut client, &block.token).await.unwrap();
    }
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &new).await {
        ran.push(day);
    }
    assert_eq!(ran, days);
    scratch.drop().await;
}

/// Audit 2026-11-22 opsurface round 2 (round2-core.md) 6, 11 and 1, C-068: (6) the successor reads
/// other profile revisions than the old build (here it registers no day-local profile), so a day the
/// old build published is one its pass cannot read; with no day block to compute it again its pass
/// would hold every later one: refused, naming the day, until a block of the successor covers it.
/// (11) the old build's tape verify job is cancelled, not carried, so the successor's tapes can be
/// scheduled over its session. (1) the old build, still running, is withdrawn by the hand-over itself.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_hand_over_carries_no_pass_its_successor_cannot_run() {
    let (scratch, mut client, _, _) = ready("c068_hand_over_unreadable").await;
    let old = whole_build_named(&mut client, "slice-old-image").await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &old, day, day, 1).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &old, day, day, 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &old, "w1", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &old, &block, day).await;
    work::finish(&mut client, &block.token).await.unwrap();
    // a verify sample of the old build, as C-059 schedules one, over a stored cache
    assert!(core_db::tape::put(&client, "AAPL", day, "engine-v1", &core_db::tape::Cache::Body(test_support::TAPE_AAPL.to_vec())).await.unwrap());
    let verify = work::schedule_verify(&mut client, PROFILE, &old, &[day]).await.unwrap();
    // the successor reads the base but no day-local profile
    let mut base_only = inventories()[0].clone();
    base_only["engine_build"] = json!("slice-new-base-only");
    let new = build::register(&mut client, &base_only).await.unwrap().engine_build;
    let refused = build::hand_over(&mut client, &old, &new, "a new image", true, None).await.unwrap_err();
    assert!(refused.contains(&format!("1 days are published in profile revisions build {new} does not read, and no day block computes them again: {day}")), "{refused}");
    // its block scheduled over the day: carried; the old build still ran and is withdrawn by it
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &new, day, day, 1).await.unwrap();
    let preview = build::hand_over(&mut client, &old, &new, "a new image", true, None).await.unwrap();
    assert!(preview.withdrew);
    assert_eq!((preview.carried.len(), preview.verify_cancelled.clone()), (1, verify.clone()));
    assert_eq!(job_state(&client, verify[0]).await, "QUEUED", "a preview changes nothing");
    let handed = build::hand_over(&mut client, &old, &new, "a new image", false, Some(&preview.token)).await.unwrap();
    assert!(handed.withdrew);
    assert_eq!(job_state(&client, verify[0]).await, "CANCELLED");
    let withdrawn: bool = client.query_one("SELECT withdrawn_at IS NOT NULL FROM core.engine_build WHERE engine_build = $1", &[&old]).await.unwrap().get(0);
    assert!(withdrawn);
    // the successor's tapes are scheduled over the verified session
    work::schedule(&mut client, Kind::Tape, PROFILE, &new, day, day, 1).await.unwrap();
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 8 and the triage's note: a withdrawn build's live lease
/// could still publish, while the passes after it no longer waited for it (C-062), so a later day's
/// history could publish before an earlier one. Withdrawal now ends the build's live leases: the
/// running pass of the 10th is refused at its publish, its job is queued again (the attempt not
/// counted), and the new build's schedule replaces it and runs the days in order.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn withdrawing_a_build_stops_its_running_jobs() {
    let (scratch, mut client, b, d) = ready("c063_withdraw").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_day(&mut client, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    let tenth = ordered(&mut client, &d).await.unwrap();
    assert_eq!(tenth.sessions, [days[0]]);

    // withdrawn while the 10th's pass runs: its lease ends, and what it computed is not taken
    let stopped = build::withdraw(&mut client, &d, "its image is replaced").await.unwrap();
    assert_eq!(stopped, [tenth.job_id]);
    let base = base_of(&client, days[0]).await;
    assert_eq!(derive(&mut client, days[0], base, &tenth.token).await, Err(LeaseError::Gone));
    assert_eq!(work::heartbeat(&mut client, &tenth.token, HOUR).await, Err(LeaseError::Gone));
    let row = client.query_one("SELECT state, attempts FROM core.job WHERE job_id = $1", &[&tenth.job_id]).await.unwrap();
    assert_eq!((row.get::<_, String>(0), row.get::<_, i32>(1)), ("QUEUED".to_owned(), 0), "queued again, the attempt not counted");
    let lease = client.query_one("SELECT outcome, failure FROM core.lease WHERE job_id = $1", &[&tenth.job_id]).await.unwrap();
    assert_eq!((lease.get::<_, String>(0), lease.get::<_, String>(1)), ("CANCELLED".to_owned(), "its engine build was withdrawn".to_owned()));
    assert!(notices(&client).await.last().unwrap().ends_with(&format!("its image is replaced. 1 running jobs stopped, their leases ended: {}.", tenth.job_id)));
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local").await, 0);

    // the new build replaces both passes and runs them in date order
    let w = whole_build(&mut client).await;
    let made = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[0], days[1], 1).await.unwrap();
    assert_eq!((made.jobs, made.replaced.len()), (2, 2));
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &w).await {
        ran.push(day);
    }
    assert_eq!(ran, days);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 7 and 8: a withdrawal took no publish lock, so it
/// could commit between two statements of a publish's re-derive (an old build's queued pass then stood
/// in the way of the new build's), and it locked a live job's lease then the job while `retire` locked
/// the job then the lease. A withdrawal now waits for the publish lock, as `retire` does, and both lock
/// leases before jobs: a retirement and a withdrawal of the build running the day's block, released
/// together, both end, and the block is cancelled whichever ran first.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_withdrawal_waits_for_the_publish_lock_and_a_retirement_beside_it_ends() {
    let (scratch, mut client, b, _) = ready("r2_withdraw_lock").await;
    admit(&client, &b).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, day).await;
    work::finish(&mut client, &block.token).await.unwrap();
    // the day's block again, running: a live lease of the build that published the day
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let again = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    let preview = publish::retire(&mut client, "an earlier build", &[day], true, None).await.unwrap();
    assert_eq!(preview.cancelled, [again.job_id]);

    let mut holder = scratch.client().await;
    let held = holder.transaction().await.unwrap();
    held.execute(work::PUBLISH_LOCK, &[]).await.unwrap();
    let (mut c1, mut c2) = (scratch.client().await, scratch.client().await);
    let b2 = b.clone();
    let withdrawing = tokio::spawn(async move { build::withdraw(&mut c1, &b2, "its image is replaced").await });
    let token = preview.token.clone();
    let retiring = tokio::spawn(async move { publish::retire(&mut c2, "an earlier build", &[day], false, Some(&token)).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!withdrawing.is_finished(), "the withdrawal waits for the publish lock");
    assert!(!retiring.is_finished());
    held.commit().await.unwrap();
    withdrawing.await.unwrap().unwrap();
    retiring.await.unwrap().unwrap();
    assert_eq!(job_state(&client, again.job_id).await, "CANCELLED");
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease WHERE ended_at IS NULL").await, 0);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 0);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 11: a withdrawal stamped its time and its leases'
/// ends with its transaction's start, though it waits for the publish lock (and a claim) first; a
/// publish and a grant are stamped with the clock, so a lease could read as ended before what it waited
/// for. Held behind the publish lock, the withdrawal's stamps are after the lock was released.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_withdrawal_is_stamped_when_it_ends_its_leases() {
    let (scratch, mut client, b, _) = ready("r2_withdraw_clock").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let mut holder = scratch.client().await;
    let held = holder.transaction().await.unwrap();
    held.execute(work::PUBLISH_LOCK, &[]).await.unwrap();
    let mut c = scratch.client().await;
    let b2 = b.clone();
    let withdrawing = tokio::spawn(async move { build::withdraw(&mut c, &b2, "its image is replaced").await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let released: String = held.query_one("SELECT clock_timestamp()::text", &[]).await.unwrap().get(0);
    held.commit().await.unwrap();
    assert_eq!(withdrawing.await.unwrap().unwrap(), [block.job_id]);
    let row = client
        .query_one(
            "SELECT b.withdrawn_at >= $1::text::timestamptz, l.ended_at >= $1::text::timestamptz, l.ended_at >= l.granted_at
             FROM core.engine_build b, core.lease l WHERE b.engine_build = $2 AND l.job_id = $3",
            &[&released, &b, &block.job_id],
        )
        .await
        .unwrap();
    assert_eq!((row.get::<_, bool>(0), row.get::<_, bool>(1), row.get::<_, bool>(2)), (true, true, true));
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-watch.md) 1: an ordered pass's claim waits for the
/// publish lock; its lease was granted on the clock but expired from the transaction's start, so
/// a wait longer than the lease handed out a lease already expired. Held 3 s behind the lock, a
/// 2 s lease is granted after the lock is released and is still live when it comes back.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_claim_that_waited_for_the_publish_lock_gets_a_live_lease() {
    let (scratch, mut client, b, d) = ready("r2w_lease_clock").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, day).await;
    work::finish(&mut client, &block.token).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, day, day, 1).await.unwrap();
    let mut holder = scratch.client().await;
    let held = holder.transaction().await.unwrap();
    held.execute(work::PUBLISH_LOCK, &[]).await.unwrap();
    let mut c = scratch.client().await;
    let d2 = d.clone();
    let claiming = tokio::spawn(async move { work::claim(&mut c, Kind::OrderedPass, &d2, "pass", 2.0).await });
    tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
    assert!(!claiming.is_finished(), "the pass's claim waits for the publish lock");
    let released: String = held.query_one("SELECT clock_timestamp()::text", &[]).await.unwrap().get(0);
    held.commit().await.unwrap();
    let pass = claiming.await.unwrap().unwrap().unwrap();
    let row = client
        .query_one(
            "SELECT granted_at >= $1::text::timestamptz, expires_at - granted_at = interval '2 seconds' FROM core.lease WHERE job_id = $2",
            &[&released, &pass.job_id],
        )
        .await
        .unwrap();
    assert_eq!((row.get::<_, bool>(0), row.get::<_, bool>(1)), (true, true));
    assert!(work::heartbeat(&mut client, &pass.token, HOUR).await.is_ok(), "the lease is live when the worker has it");
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-watch.md) 3: a pause committed while a claim ran (after
/// its statement's snapshot) let the claim lease, stamped after the pause (C-025). A claim now
/// holds the queue's control row shared: behind a pause not yet committed it waits and reads the
/// queue paused; a pause behind a claim waits for it and is stamped after it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_pause_during_a_claim_hands_out_no_lease_after_it() {
    let (scratch, mut client, b, _) = ready("r2w_pause_claim").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-09", "2025-12-10", 1).await.unwrap();
    let mut pauser = scratch.client().await;
    let pausing = pauser.transaction().await.unwrap();
    work::pause(&pausing, "a check failed").await.unwrap();
    let mut c = scratch.client().await;
    let b2 = b.clone();
    let claiming = tokio::spawn(async move { work::claim(&mut c, Kind::Tape, &b2, "tape-1", HOUR).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!claiming.is_finished(), "the claim waits for the pause under way");
    pausing.commit().await.unwrap();
    assert_eq!(claiming.await.unwrap().unwrap(), None, "the pause committed first: nothing is handed out");
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease").await, 0);
    work::resume(&client).await.unwrap();

    // a claim under way (holding the control row as a claim does): the pause waits and is
    // stamped after it
    let mut claimer = scratch.client().await;
    let in_flight = claimer.transaction().await.unwrap();
    in_flight.execute("SELECT paused FROM core.queue_control FOR SHARE", &[]).await.unwrap();
    let mut p = scratch.client().await;
    let pause = tokio::spawn(async move {
        let tx = p.transaction().await.unwrap();
        work::pause(&tx, "a check failed").await.unwrap();
        tx.commit().await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!pause.is_finished(), "the pause waits for the claim under way");
    let ended: String = in_flight.query_one("SELECT clock_timestamp()::text", &[]).await.unwrap().get(0);
    in_flight.commit().await.unwrap();
    pause.await.unwrap();
    let after: bool = client.query_one("SELECT changed_at >= $1::text::timestamptz FROM core.queue_control", &[&ended]).await.unwrap().get(0);
    assert!(after, "the pause is stamped after the claim it waited for");
    assert_eq!(work::claim(&mut client, Kind::Tape, &b, "tape-2", HOUR).await.unwrap(), None);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 7: a fallen day whose only unfinished pass was of a
/// withdrawn build counted as derived again: the publishing build got no pass, the withdrawn one
/// never ran, and no problem was opened. Now that pass is cancelled and the day's pass goes to the
/// publishing build. The 11th's pass of the earlier build scheduled again (queued), that build
/// withdrawn, then the new build publishes the 10th.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_fallen_day_whose_pass_is_under_a_withdrawn_build_goes_to_the_publishing_build() {
    let (scratch, mut client, b, d) = ready("c063_unfinished").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_base_as(&mut client, &b, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    while run_pass(&mut client, &d).await.is_some() {}
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[1], days[1], 1).await.unwrap();
    let stale = count(&client, "SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' AND state = 'QUEUED'").await;
    build::withdraw(&mut client, &d, "its image is replaced").await.unwrap();

    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[0], days[0], 1).await.unwrap();
    let fresh = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &fresh, days[0]).await;
    work::finish(&mut client, &fresh.token).await.unwrap();
    let row = |day: &str, build: &str, state: &str| (day.to_owned(), build.to_owned(), state.to_owned());
    assert_eq!(passes_by_build(&client).await, [row(days[0], &d, "DONE"), row(days[0], &w, "QUEUED"), row(days[1], &d, "DONE"), row(days[1], &d, "CANCELLED"), row(days[1], &w, "QUEUED")]);
    let said = notices(&client).await;
    assert!(said.iter().any(|n| n.contains(&format!("passes under builds that do not run cancelled: jobs {stale}; the ordered pass derives 2 days again"))), "{said:?}");
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &w).await {
        ran.push(day);
    }
    assert_eq!(ran, days);
    scratch.drop().await;
}

/// Publishes `day`'s base (the slice's real bodies under that date: core's checks never read a
/// body's date) under the block's lease and says the day is done.
async fn publish_day(client: &mut Client, claim: &Claim, day: &str) {
    let (mut h, s, f) = parts(BASE);
    h.represented_date = day.into();
    let path = bundle(&unique(&format!("c061-{day}-{}", claim.job_id)), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let build = claim.engine_build.clone();
    day_local(client, &build, day, base, Some(&claim.token)).await;
    work::day_done(client, &claim.token, day).await.unwrap();
}

/// Runs the claimable ordered pass, if any, on its day's published base; returns its day.
async fn run_pass(client: &mut Client, d: &str) -> Option<String> {
    let pass = ordered(client, d).await?;
    let day = pass.sessions[0].clone();
    let base = base_of(client, &day).await;
    derive(client, &day, base, &pass.token).await.unwrap();
    work::finish(client, &pass.token).await.unwrap();
    Some(day)
}

/// The days whose ordered pass is claimable now.
async fn claimable_passes(client: &Client) -> String {
    client
        .query_one("SELECT COALESCE(string_agg(first_session::text, ' ' ORDER BY first_session), '') FROM core.claimable_job WHERE kind = 'ORDERED_PASS'", &[])
        .await
        .unwrap()
        .get(0)
}

async fn cause(client: &Client, day: &str) -> (String, String) {
    let row = client.query_one("SELECT c->>'code', c->>'detail' FROM core.session_cause($1::text::date, $2) c", &[&day, &PROFILE]).await.unwrap();
    (row.get(0), row.get(1))
}

/// C-061: a failed day is a missing date. Four real sessions, blocks of one day, each with its
/// ordered pass scheduled whole (C-046); the 10th's block stops for the last time on the engine's
/// provider-input hold. The later passes go on past it (the 11th's, then the 12th's), its own does
/// not run (its base is unpublished); queued again (`retry-failed`), the 10th holds the passes
/// again, and once it publishes, C-037 derives the 11th again and the passes run on in date order.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failed_day_holds_no_later_pass_and_a_retried_one_holds_them_again() {
    let (scratch, mut client, b, d) = ready("c061_failed_day").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[3], 1).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[3], 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let mut failed_job = 0;
    for day in days {
        let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        assert_eq!(claim.sessions, [day]);
        if day == days[1] {
            assert!(!work::fail(&mut client, &claim.token, &format!("day {day}"), &test_support::provider_input_hold(day), "log").await.unwrap());
            failed_job = claim.job_id;
        } else {
            publish_day(&mut client, &claim, day).await;
            work::finish(&mut client, &claim.token).await.unwrap();
        }
    }
    assert_eq!(job_state(&client, failed_job).await, "FAILED");
    assert_eq!(cause(&client, days[1]).await, ("PROVIDER_INPUT_MISSING".into(), "412 of 3904 final slots lack a provider input (more than 0.1)".into()));

    // the 9th's pass, then the 11th's past the failed 10th; the 10th's own never runs, and the
    // 12th's is next
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[0]));
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[2]), "the failed 10th holds no later pass");
    assert_eq!(claimable_passes(&client).await, "2025-12-12", "the 10th's pass has no base to read; the 12th's is free");
    let state = |s: &[&str]| days.iter().zip(s).map(|(d, s)| (d.to_string(), s.to_string())).collect::<Vec<_>>();
    let passes = |p: Vec<(String, String, i32)>| p.into_iter().map(|(d, s, _)| (d, s)).collect::<Vec<_>>();
    assert_eq!(passes(pass_jobs(&client).await), state(&["DONE", "QUEUED", "DONE", "QUEUED"]));

    // queued again, the 10th holds the passes again, as any pending day does
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    assert_eq!(cause(&client, days[1]).await, ("NOT_COMPUTED_YET".into(), "its day is queued".into()));
    assert_eq!(claimable_passes(&client).await, "", "a pending day holds every later pass");
    assert_eq!(ordered(&mut client, &d).await, None);
    // it publishes: the 11th's history revision read a window without it and falls (C-037), and the
    // passes run in date order from the 10th
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!((claim.job_id, claim.sessions.clone()), (failed_job, vec![days[1].to_owned()]));
    publish_day(&mut client, &claim, days[1]).await;
    work::finish(&mut client, &claim.token).await.unwrap();
    let mut ran = Vec::new();
    while let Some(day) = run_pass(&mut client, &d).await {
        ran.push(day);
    }
    assert_eq!(ran, [days[1], days[2], days[3]]);
    assert_eq!(passes(pass_jobs(&client).await), state(&["DONE", "DONE", "DONE", "DONE"]));
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local").await, 4);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 6: an upload that ends before its scalars is refused
/// before its lease is checked; from a withdrawn build's worker it was a refused bundle, and the
/// service paused the queue on it. A refusal from a lease that has ended is now the lease gone; the
/// same upload under a live lease is still refused.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refused_upload_from_an_ended_lease_is_the_lease_gone() {
    let (scratch, mut client, b, _) = ready("r2_ended_upload").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (mut h, s, f) = parts(BASE);
    h.represented_date = "2025-12-10".into();
    let path = bundle(&unique("r2-truncated"), &h, &s, &f);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 8]).unwrap();
    let live = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    assert!(matches!(&live, LeaseError::Refused(_)), "{live}");
    build::withdraw(&mut client, &b, "its image is replaced").await.unwrap();
    let ended = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    assert_eq!(ended, LeaseError::Gone);
    std::fs::remove_file(&path).unwrap();
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 2: a block whose last attempt published the day's
/// base and failed before its day-local revision (C-031) left a day the engine's ordered pass refuses
/// ("no published day-local derived revision on base revision N"); its pass was claimable, failed
/// three times, and then held every later pass for ever. A day is now published for the passes only
/// with its day-local revision on its base: the 10th here is a missing day, its pass never claimed,
/// the later passes run past it; the notice and the digest name the base without its day-local
/// revision; queued again, the 10th holds the passes until it publishes whole.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_whose_block_failed_after_its_base_is_missing_for_the_passes() {
    let (scratch, mut client, b, d) = ready("c061_base_only").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-09", "2025-12-10", "2025-12-11", "2025-12-12"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[3], 1).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[3], 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let mut base10 = 0;
    for day in days {
        let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        if day == days[1] {
            // the base commits, the day-local upload does not (a 422, or core restarting)
            let (mut h, s, f) = parts(BASE);
            h.represented_date = day.into();
            let path = bundle(&unique("c061-base-only"), &h, &s, &f);
            base10 = publish_for_lease(&mut client, &path, &claim.token).await.unwrap().revision_id;
            std::fs::remove_file(&path).unwrap();
            assert!(!work::fail(&mut client, &claim.token, &format!("day {day}"), "day-local derived: core answered 503", "log").await.unwrap());
        } else {
            publish_day(&mut client, &claim, day).await;
            work::finish(&mut client, &claim.token).await.unwrap();
        }
    }
    let said = notices(&client).await;
    assert!(
        said.iter().any(|n| n.contains(&format!(
            "its base revision {base10} was published but no day-local derived revision on it: the base is superseded, so no later day's history reads 2025-12-10"
        ))),
        "{said:?}"
    );
    // round 2 core-r2 1: the engine's pass reads every published base, so the base is no longer
    // published: the 10th is missing for the later days' windows too, and its row is kept
    let state = |id: i64| format!("SELECT state FROM vyreon.day_revision WHERE revision_id = {id}");
    assert_eq!(client.query_one(&state(base10), &[]).await.unwrap().get::<_, String>(0), "SUPERSEDED");
    assert_eq!(count(&client, &format!("SELECT count(*) FROM core.dropped_base WHERE revision_id = {base10} AND represented_date = '2025-12-10'")).await, 1);
    assert_eq!(
        cause(&client, days[1]).await,
        ("FAILED".into(), format!("base revision {base10} superseded, no day-local revision was published on it; day-local derived: core answered 503"))
    );
    // the 9th, then the 11th past the 10th, whose own pass is never handed out; the 12th's is next
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[0]));
    assert_eq!(claimable_passes(&client).await, "2025-12-11", "the 10th's pass waits for a day-local revision; the 11th's is free");
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[2]));
    assert_eq!(claimable_passes(&client).await, "2025-12-12");
    // the digest names the day set aside, with the base it left
    let s = watch::Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: 10, daily: None, archive: None, refcheck_from: None, backup_days: None };
    let now = watch::now(&client).await.unwrap();
    let digest = watch::digest(&client, &s, &now).await.unwrap();
    let line = format!("Set aside: Wed Dec 10, 2025 · FAILED · base revision {base10} superseded, no day-local revision was published on it; day-local derived: core answered 503");
    assert!(digest.lines().any(|l| l == line), "{digest}");

    // queued again, it holds the later passes until it publishes whole; then they run in order
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    assert_eq!(claimable_passes(&client).await, "");
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &claim, days[1]).await;
    work::finish(&mut client, &claim.token).await.unwrap();
    let mut ran = Vec::new();
    while let Some(day) = run_pass(&mut client, &d).await {
        ran.push(day);
    }
    assert_eq!(ran, [days[1], days[2], days[3]]);
    // the retry published the 10th afresh: a new base, its day-local revision on it
    let published = count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE represented_date = '2025-12-10' AND state = 'PUBLISHED' AND base_revision IS NULL").await;
    assert_eq!(published, 1);
    assert_ne!(base_of(&client, days[1]).await, base10);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 1, by hand: a day block cancelled after it published
/// its day's base and before the day-local revision leaves no base for the later passes to read either:
/// the base is superseded, the notice names it and the session's cause states it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_block_cancelled_after_its_base_leaves_no_base_to_read() {
    let (scratch, mut client, b, _) = ready("r2_cancelled_base").await;
    admit(&client, &b).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, "2025-12-11", 2).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (mut h, s, f) = parts(BASE);
    h.represented_date = day.into();
    let path = bundle(&unique("r2-cancelled-base"), &h, &s, &f);
    let base = publish_for_lease(&mut client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    assert_eq!(work::cancel(&mut client, &[claim.job_id]).await.unwrap(), 1);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 0);
    let said = notices(&client).await;
    assert!(said.iter().any(|n| n.starts_with(&format!("Day block job {} cancelled on {day}; its base revision {base} was published", claim.job_id))), "{said:?}");
    assert_eq!(
        cause(&client, day).await,
        ("NOT_SCHEDULED".into(), format!("its day job {} was cancelled; base revision {base} superseded, no day-local revision was published on it", claim.job_id))
    );
    scratch.drop().await;
}

/// C-062 (the gap the 2026-11-22 triage found in C-061): a day block cancelled by hand, not through
/// `omit`, left its queued pass holding every later pass. A day is now missing whatever ended its
/// block (FAILED, CANCELLED, or DONE with nothing published) while no block is pending; a day whose
/// block has never been scheduled is still to come and holds (C-046), and a day scheduled again holds
/// again. Blocks of one day on the 9th, 10th, 11th and 15th, passes 9th to 15th (the 12th's block
/// not scheduled; the 13th and 14th are a weekend).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_cancelled_by_hand_holds_no_later_pass_and_one_still_to_come_does() {
    let (scratch, mut client, b, d) = ready("c062_cancelled_day").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-11", 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-15", "2025-12-15", 1).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, "2025-12-09", "2025-12-15", 1).await.unwrap();
    let tenth: i64 = count(&client, "SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-10'").await;
    assert_eq!(work::cancel(&mut client, &[tenth]).await.unwrap(), 1);
    for day in ["2025-12-09", "2025-12-11", "2025-12-15"] {
        let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        assert_eq!(claim.sessions, [day]);
        publish_day(&mut client, &claim, day).await;
        work::finish(&mut client, &claim.token).await.unwrap();
    }
    // the 9th, then the 11th past the cancelled 10th, whose queued pass holds nothing
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some("2025-12-09"));
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some("2025-12-11"), "the 10th's block was cancelled by hand: a missing date");
    assert_eq!(cause(&client, "2025-12-10").await.0, "NOT_SCHEDULED");
    // the 12th's block was never scheduled: it is still to come and holds the 15th's pass
    assert_eq!(claimable_passes(&client).await, "", "a day never scheduled holds the later passes");
    // scheduled and then cancelled by hand, it is missing too
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-12", "2025-12-12", 1).await.unwrap();
    assert_eq!(claimable_passes(&client).await, "", "a pending day holds them");
    let twelfth: i64 = count(&client, "SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-12'").await;
    work::cancel(&mut client, &[twelfth]).await.unwrap();
    assert_eq!(claimable_passes(&client).await, "2025-12-15");
    // the 10th scheduled again holds them again, as any pending day
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    assert_eq!(claimable_passes(&client).await, "");
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &claim, "2025-12-10").await;
    work::finish(&mut client, &claim.token).await.unwrap();
    let mut ran = Vec::new();
    while let Some(day) = run_pass(&mut client, &d).await {
        ran.push(day);
    }
    assert_eq!(ran, ["2025-12-10", "2025-12-11", "2025-12-15"], "the 11th derived again behind the 10th (C-037)");
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 6: any block that had ever covered a day made it a
/// missing date, so under a new build with its passes scheduled for the whole range first (C-046),
/// an earlier build's retired DONE block or cancelled block let the later passes run before the new
/// build's block for that day, and run again after it. Only a block of a build that runs makes its
/// day tried now, and a DONE one only while something of its day is published. The earlier build
/// published the 10th (since retired) and had the 11th's block cancelled; the new build's passes cover
/// the 10th to the 12th and its newest tranche, the 12th, is published.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_earlier_builds_ended_block_leaves_a_day_still_to_come() {
    let (scratch, mut client, b, _) = ready("c063_earlier_block").await;
    admit(&client, &b).await;
    let days = ["2025-12-10", "2025-12-11", "2025-12-12"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[0], 1).await.unwrap();
    let old = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &old, days[0]).await;
    work::finish(&mut client, &old.token).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[1], days[1], 1).await.unwrap();
    let cancelled = count(&client, "SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-11'").await;
    work::cancel(&mut client, &[cancelled]).await.unwrap();
    let preview = publish::retire(&mut client, "an earlier build", &[days[0]], true, None).await.unwrap();
    publish::retire(&mut client, "an earlier build", &[days[0]], false, Some(&preview.token)).await.unwrap();

    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[0], days[2], 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[2], days[2], 1).await.unwrap();
    let newest = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &newest, days[2]).await;
    work::finish(&mut client, &newest.token).await.unwrap();
    // the 10th's only block is DONE with nothing published: still to come, it holds the 12th
    assert_eq!(claimable_passes(&client).await, "", "a retired day's DONE block is no attempt of the day");
    // the earlier build withdrawn, its cancelled block of the 11th makes no missing date either
    build::withdraw(&mut client, &b, "its image is replaced").await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[0], days[0], 1).await.unwrap();
    let tenth = work::claim(&mut client, Kind::DayBlock, &w, "w3", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &tenth, days[0]).await;
    work::finish(&mut client, &tenth.token).await.unwrap();
    assert_eq!(run_pass_as(&mut client, &w).await.as_deref(), Some(days[0]));
    assert_eq!(claimable_passes(&client).await, "", "the 11th's block of a withdrawn build: still to come");
    // the new build's block of the 11th, cancelled by hand: now a missing date
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[1], days[1], 1).await.unwrap();
    let mine = count(&client, &format!("SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND first_session = '2025-12-11' AND engine_build = '{w}'")).await;
    work::cancel(&mut client, &[mine]).await.unwrap();
    assert_eq!(run_pass_as(&mut client, &w).await.as_deref(), Some(days[2]));
    let runs = count(&client, "SELECT count(*) FROM core.lease JOIN core.job USING (job_id) WHERE kind = 'ORDERED_PASS' AND first_session = '2025-12-12'").await;
    assert_eq!(runs, 1, "the 12th's pass ran once");
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 2: a day whose day-local revision is of a profile
/// revision the pass's build does not read (an earlier build's, named `<day-local>-EARLIER` here; only
/// the name is made) was published for the claim, so its pass was handed out, refused by the engine
/// three times, and then held every later pass. It is now a missing day for that pass: the 10th,
/// published by the earlier build, base and day-local revision; the whole build's passes run the 9th
/// and then the 11th, and the 10th's pass is never handed out.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_local_revision_the_pass_does_not_read_makes_a_missing_day() {
    let (scratch, mut client, _, _) = ready("r2_other_day_local").await;
    let w = whole_build(&mut client).await;
    let mut earlier = with_day_local(inventories().remove(0));
    let other = format!("{}-EARLIER", test_support::slice::day_local_profile());
    earlier["engine_build"] = json!("slice-earlier-day-local");
    for row in earlier["slots"].as_array_mut().unwrap() {
        if row["profile_revision"] == json!(test_support::slice::day_local_profile()) {
            row["profile_revision"] = json!(other);
        }
    }
    let e = build::register(&mut client, &earlier).await.unwrap().engine_build;
    admit(&client, &e).await;
    let days = ["2025-12-09", "2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[0], days[2], 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &e, days[1], days[1], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &e, "w1", HOUR).await.unwrap().unwrap();
    let (mut h, s, f) = parts(BASE);
    (h.represented_date, h.engine_build) = (days[1].into(), e.clone());
    let path = bundle(&unique("r2-earlier-base"), &h, &s, &f);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (mut h, s, f) = day_local_parts(&e, days[1], base);
    h.profile_revision = other.clone();
    let path = bundle(&unique("r2-earlier-day-local"), &h, &s, &f);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::day_done(&mut client, &block.token, days[1]).await.unwrap();
    work::finish(&mut client, &block.token).await.unwrap();
    for day in [days[0], days[2]] {
        work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, day, day, 1).await.unwrap();
        let claim = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
        publish_base_as(&mut client, &w, &claim, day).await;
        work::finish(&mut client, &claim.token).await.unwrap();
    }
    assert_eq!(run_pass_as(&mut client, &w).await.as_deref(), Some(days[0]));
    assert_eq!(claimable_passes(&client).await, "2025-12-11", "the 10th's day-local revision is not the pass's: a missing day, never a pass handed out");
    assert_eq!(run_pass_as(&mut client, &w).await.as_deref(), Some(days[2]));
    assert_eq!(claimable_passes(&client).await, "");
    let tenth = count(&client, "SELECT count(*) FROM core.lease JOIN core.job USING (job_id) WHERE kind = 'ORDERED_PASS' AND first_session = '2025-12-10'").await;
    assert_eq!(tenth, 0, "the 10th's pass was never handed out");
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 3: the claim counted any omission of a day as an
/// attempt, the reader only a current one. The 10th omitted (the earlier build's block failed on the
/// provider), published by hand, then retired: it reads RETIRED, and under the new build, with its
/// passes scheduled whole and its block for the 10th still to come, the 10th now holds the 11th's pass
/// as a day still to come does; once its block publishes, the passes run in date order, each once.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_omission_no_longer_current_leaves_a_day_still_to_come() {
    let (scratch, mut client, b, _) = ready("r2_old_omission").await;
    admit(&client, &b).await;
    let days = ["2025-12-09", "2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[1], days[1], 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    assert!(!work::fail(&mut client, &claim.token, &format!("day {}", days[1]), &test_support::provider_input_hold(days[1]), "log").await.unwrap());
    work::omit(&mut client, PROFILE, days[1], "Intrinio delivered no option prices").await.unwrap();
    base_by_hand(&mut client, days[1]).await;
    let preview = publish::retire(&mut client, "published by hand before phase 1", &[days[1]], true, None).await.unwrap();
    publish::retire(&mut client, "published by hand before phase 1", &[days[1]], false, Some(&preview.token)).await.unwrap();
    assert_eq!(cause(&client, days[1]).await.0, "RETIRED");
    build::withdraw(&mut client, &b, "its image is replaced").await.unwrap();

    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &w, days[0], days[2], 1).await.unwrap();
    for day in [days[0], days[2]] {
        work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, day, day, 1).await.unwrap();
        let claim = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
        publish_base_as(&mut client, &w, &claim, day).await;
        work::finish(&mut client, &claim.token).await.unwrap();
    }
    assert_eq!(run_pass_as(&mut client, &w).await.as_deref(), Some(days[0]));
    assert_eq!(claimable_passes(&client).await, "", "an omission retired since is no attempt of the day: the 10th is still to come");
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[1], days[1], 1).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &w, "w3", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &claim, days[1]).await;
    work::finish(&mut client, &claim.token).await.unwrap();
    let mut ran = Vec::new();
    while let Some(day) = run_pass_as(&mut client, &w).await {
        ran.push(day);
    }
    assert_eq!(ran, [days[1], days[2]], "the 11th's pass ran once, after the 10th");
    scratch.drop().await;
}

/// C-062: `retire` takes the published days of an earlier build out of what is read, with a reason,
/// keeping every row. Four real sessions (10th, 11th, 12th, 15th) published by the slice's builds
/// with their passes done; the earlier build's pass of the 11th scheduled again (unfinished), and a
/// day block of the new build queued on the 10th. Retiring the 10th and 11th: a preview changes
/// nothing; then their four revisions are superseded and named, the earlier build's unfinished pass
/// is cancelled and the new build's block stays, the later history revisions that read them fall and
/// their passes are queued again (C-037), and the reader states the 11th RETIRED. A retired day is a
/// missing date for the later passes.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn published_days_of_an_earlier_build_are_retired_kept_and_stated() {
    let (scratch, mut client, b, d) = ready("c062_retire").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11", "2025-12-12", "2025-12-15"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[3], 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[3], 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_base_as(&mut client, &b, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    while run_pass(&mut client, &d).await.is_some() {}
    let rerun = work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[1], days[1], 1).await.unwrap();
    assert_eq!(rerun.jobs, 1);
    let pass11 = count(&client, "SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' AND state = 'QUEUED'").await;
    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, days[0], days[0], 1).await.unwrap();
    let block10 = count(&client, &format!("SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND engine_build = '{w}'")).await;
    let reason = "published by earlier builds; phase 1 computes them afresh";
    let revisions_before = count(&client, "SELECT count(*) FROM vyreon.day_revision").await;
    let notices_before = notices(&client).await.len();

    // refused: no reason, a date that is not a session, a day block of the days' build that also
    // covers a date not named
    assert!(publish::retire(&mut client, " ", &[days[1]], true, None).await.unwrap_err().contains("needs a reason"));
    assert!(publish::retire(&mut client, reason, &["2025-12-13"], true, None).await.unwrap_err().contains("2025-12-13 is not a trading session"));
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[3], "2025-12-16", 2).await.unwrap();
    let wide = count(&client, "SELECT job_id FROM core.job WHERE kind = 'DAY_BLOCK' AND last_session = '2025-12-16'").await;
    let refused = publish::retire(&mut client, reason, &[days[3]], true, None).await.unwrap_err();
    assert!(refused.contains(&format!("day block {wide} (2025-12-15 to 2025-12-16) also covers dates not named")), "{refused}");
    work::cancel(&mut client, &[wide]).await.unwrap();

    // previewed: the report, and nothing changed
    let preview = publish::retire(&mut client, reason, &[days[1], days[0]], true, None).await.unwrap();
    let kinds: Vec<(String, String, String)> = preview.revisions.iter().map(|r| (r.date.clone(), r.engine_build.clone(), r.kind.clone())).collect();
    let row = |day: &str, build: &str, kind: &str| (day.to_owned(), build.to_owned(), kind.to_owned());
    assert_eq!(
        kinds,
        [row(days[0], &b, "base"), row(days[0], &b, "day-local"), row(days[0], &d, "history"), row(days[1], &b, "base"), row(days[1], &b, "day-local"), row(days[1], &d, "history")]
    );
    assert_eq!((preview.retirement_id, preview.cancelled.clone(), preview.fallen), (None, vec![pass11], 2));
    assert_eq!((preview.rederived.clone(), preview.unowned.clone()), (vec![days[2].to_owned(), days[3].to_owned()], vec![]));
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 12);
    assert_eq!((job_state(&client, pass11).await, count(&client, "SELECT count(*) FROM core.retirement").await), ("QUEUED".into(), 0));
    assert_eq!(notices(&client).await.len(), notices_before, "nothing logged");

    // done: the same, kept and recorded
    // C-063: done only with the preview's token; without one, or with another, nothing is done
    assert!(publish::retire(&mut client, reason, &[days[1], days[0]], false, None).await.unwrap_err().contains("run it with --preview first"));
    let other = publish::retire(&mut client, reason, &[days[1], days[0]], false, Some("0000000000000000")).await.unwrap_err();
    assert!(other.contains(&format!("its token 0000000000000000, now {}", preview.token)), "{other}");
    assert_eq!(count(&client, "SELECT count(*) FROM core.retirement").await, 0);
    let done = publish::retire(&mut client, reason, &[days[1], days[0]], false, Some(&preview.token)).await.unwrap();
    assert_eq!(Retired { retirement_id: None, ..done.clone() }, preview);
    let id = done.retirement_id.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, revisions_before, "no row is deleted");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 4, "the 12th's and 15th's bases and day-local revisions");
    assert_eq!(count(&client, &format!("SELECT count(*) FROM core.retired_revision WHERE retirement_id = {id}")).await, 6);
    let kept = client.query_one("SELECT reason, sessions::text, cancelled FROM core.retirement", &[]).await.unwrap();
    assert_eq!((kept.get::<_, String>(0), kept.get::<_, String>(1), kept.get::<_, Vec<i64>>(2)), (reason.to_owned(), "{2025-12-10,2025-12-11}".to_owned(), vec![pass11]));
    assert_eq!((job_state(&client, pass11).await, job_state(&client, block10).await), ("CANCELLED".into(), "QUEUED".into()), "the new build's block stays");
    let said = notices(&client).await;
    assert!(said.iter().any(|n| n == "Retired 2 dates, 2025-12-10 to 2025-12-11: 2 revisions that read them are superseded; the ordered pass derives 2 days again, 2025-12-12 to 2025-12-15"), "{said:?}");
    assert_eq!(said.last().unwrap(), &format!("Retirement {id}: 6 published revisions of 2 dates, 2025-12-10 to 2025-12-11 superseded (kept), jobs {pass11} cancelled: {reason}."));
    assert_eq!(cause(&client, days[1]).await, ("RETIRED".into(), reason.into()));
    assert_eq!(cause(&client, days[0]).await.0, "NOT_COMPUTED_YET", "the new build's block will compute it");
    assert!(publish::retire(&mut client, reason, &[days[1]], true, None).await.unwrap_err().contains("nothing is published on 2025-12-11"));

    // the later passes wait for the new build's pending day, then run past the two missing dates
    assert_eq!(claimable_passes(&client).await, "");
    work::cancel(&mut client, &[block10]).await.unwrap();
    let mut ran = Vec::new();
    while let Some(day) = run_pass(&mut client, &d).await {
        ran.push(day);
    }
    assert_eq!(ran, [days[2], days[3]]);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 13: a retirement chose its revisions when it ran, so a
/// day a new build published between the preview and the retirement was taken down unseen, and its
/// job cancelled. Now the retirement takes the preview's token and is refused when the dates hold
/// anything else: the earlier build's 10th previewed, the new build's block then publishes the 10th.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_retirement_takes_down_only_what_its_preview_showed() {
    let (scratch, mut client, b, _) = ready("c063_retire_token").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, "2025-12-10").await;
    work::finish(&mut client, &block.token).await.unwrap();
    let reason = "published by an earlier build";
    let preview = publish::retire(&mut client, reason, &["2025-12-10"], true, None).await.unwrap();
    assert_eq!(preview.revisions.len(), 2);

    // the new build publishes the 10th afresh before the retirement runs
    let w = whole_build(&mut client).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &w, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let fresh = work::claim(&mut client, Kind::DayBlock, &w, "w2", HOUR).await.unwrap().unwrap();
    publish_base_as(&mut client, &w, &fresh, "2025-12-10").await;
    work::finish(&mut client, &fresh.token).await.unwrap();
    let refused = publish::retire(&mut client, reason, &["2025-12-10"], false, Some(&preview.token)).await.unwrap_err();
    assert!(refused.contains("changed since the preview") && refused.contains("nothing was retired"), "{refused}");
    let fresh_published = format!("SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND engine_build = '{w}'");
    assert_eq!(count(&client, &fresh_published).await, 2, "the new build's base and day-local revision stand");
    assert_eq!(count(&client, "SELECT count(*) FROM core.retirement").await, 0);
    // previewed again, the report shows the new build's revisions and has another token
    let again = publish::retire(&mut client, reason, &["2025-12-10"], true, None).await.unwrap();
    assert_ne!(again.token, preview.token);
    assert!(again.revisions.iter().all(|r| r.engine_build == w), "{:?}", again.revisions);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 5: the token named the retired days' revisions and
/// jobs, not the later history revisions that fall with them, so a later day's pass that published
/// after the preview was taken down unseen. Previewed with the 10th's pass done and the 11th's not
/// yet run (nothing later falls), then the 11th's pass publishes: the retirement is refused.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_retirement_token_covers_the_later_history_it_takes_down() {
    let (scratch, mut client, b, d) = ready("r2_retire_later").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    for day in days {
        publish_day(&mut client, &block, day).await;
    }
    work::finish(&mut client, &block.token).await.unwrap();
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[0]));
    let reason = "published by an earlier build";
    let preview = publish::retire(&mut client, reason, &[days[0]], true, None).await.unwrap();
    assert_eq!(preview.fallen, 0);
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(days[1]));
    let refused = publish::retire(&mut client, reason, &[days[0]], false, Some(&preview.token)).await.unwrap_err();
    assert!(refused.contains("changed since the preview") && refused.contains("nothing was retired"), "{refused}");
    let eleventh = "SELECT count(*) FROM vyreon.day_revision WHERE represented_date = '2025-12-11' AND state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local";
    assert_eq!(count(&client, eleventh).await, 1, "the 11th's history revision stands");
    let again = publish::retire(&mut client, reason, &[days[0]], true, None).await.unwrap();
    assert_eq!(again.fallen, 1);
    publish::retire(&mut client, reason, &[days[0]], false, Some(&again.token)).await.unwrap();
    assert_eq!(count(&client, eleventh).await, 0);
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 9: a retirement selected its jobs without locking
/// them and recorded them all as cancelled; one whose worker finished in between (its finish takes no
/// publish lock) was skipped by the cancel but named in the retirement. A worker finishing the day's
/// block holds its lease and job while the retirement runs; it commits; the retirement names no job.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_retirement_names_only_the_jobs_it_cancelled() {
    let (scratch, mut client, b, _) = ready("r2_retire_cancelled").await;
    admit(&client, &b).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, day).await;
    let preview = publish::retire(&mut client, "an earlier build", &[day], true, None).await.unwrap();
    assert_eq!(preview.cancelled, [block.job_id]);
    // the worker's finish, under way: its lease and job held, both ended in its transaction
    let mut worker = scratch.client().await;
    let finishing = worker.transaction().await.unwrap();
    finishing.execute("SELECT FROM core.lease l JOIN core.job j USING (job_id) WHERE j.job_id = $1 AND l.ended_at IS NULL FOR UPDATE OF l, j", &[&block.job_id]).await.unwrap();
    finishing.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE job_id = $1 AND ended_at IS NULL", &[&block.job_id]).await.unwrap();
    finishing.execute("UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1", &[&block.job_id]).await.unwrap();
    let mut c = scratch.client().await;
    let token = preview.token.clone();
    let retiring = tokio::spawn(async move { publish::retire(&mut c, "an earlier build", &[day], false, Some(&token)).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!retiring.is_finished(), "the retirement waits for the job's lease");
    finishing.commit().await.unwrap();
    let retired = retiring.await.unwrap().unwrap();
    assert_eq!(retired.cancelled, Vec::<i64>::new());
    assert_eq!(job_state(&client, block.job_id).await, "DONE");
    let recorded: Vec<i64> = client.query_one("SELECT cancelled FROM core.retirement", &[]).await.unwrap().get(0);
    assert_eq!(recorded, Vec::<i64>::new());
    let said = notices(&client).await;
    assert!(said.iter().any(|n| n.starts_with("Retirement ") && n.contains("no unfinished job")), "{said:?}");
    scratch.drop().await;
}

/// C-061: `omit` takes only a day whose job stands FAILED, never the engine's fault hold; it
/// records the reason, cancels the day job and the session's queued pass and logs a notice; the
/// digest names each day set aside and each date omitted with its cause code.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_date_is_omitted_only_when_its_day_failed_for_the_provider() {
    let (scratch, mut client, b, d) = ready("c061_omit").await;
    admit(&client, &b).await;
    let days = ["2025-12-10", "2025-12-11"];
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[1], 1).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, days[0], days[1], 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let reason = "Intrinio delivered no option prices for 2025-12-10 on three attempts";
    for (session, refused) in [
        (days[0], "2025-12-10's day job 1 is QUEUED: only a day whose job stands FAILED may be omitted"),
        ("2025-12-12", "no day job of FREE_REPORT_64 covers 2025-12-12"),
        ("2025-12-13", "2025-12-13 is not a trading session"),
    ] {
        let e = work::omit(&mut client, PROFILE, session, reason).await.unwrap_err();
        assert!(e.contains(refused), "{e}");
    }
    assert!(work::omit(&mut client, PROFILE, days[0], "  ").await.unwrap_err().contains("needs a reason"));

    // held for an engine fault: fixed and recomputed, never omitted
    let first = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    work::fail(&mut client, &first.token, "day 2025-12-10", &test_support::engine_fault_hold(days[0]), "log").await.unwrap();
    let e = work::omit(&mut client, PROFILE, days[0], reason).await.unwrap_err();
    assert!(e.contains("held for an engine fault (400 of 3904 final slots are withheld for an engine fault (more than 0.1)): an engine fault is fixed and recomputed, never omitted"), "{e}");

    // audit 2026-11-22 changes-core 9: the executor's hold and the day-local derive's are the
    // engine's faults too, never omitted
    for (hold, detail) in [
        (test_support::executor_fault_hold(days[0]), "intent a: the executor held the day (too many faulted slots)"),
        (test_support::day_local_fault_hold(days[0]), "9 of 64 objects' day-local derived records faulted (more than 0.1)"),
    ] {
        work::retry_failed(&client).await.unwrap();
        let again = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
        work::fail(&mut client, &again.token, "day 2025-12-10", &hold, "log").await.unwrap();
        assert_eq!(cause(&client, days[0]).await, ("ENGINE_FAULT".into(), detail.into()));
        let e = work::omit(&mut client, PROFILE, days[0], reason).await.unwrap_err();
        assert!(e.contains(&format!("held for an engine fault ({detail}): an engine fault is fixed and recomputed, never omitted")), "{e}");
    }

    // tried again, it stops on the provider-input hold, and is omitted
    work::retry_failed(&client).await.unwrap();
    let again = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    assert_eq!(again.job_id, first.job_id);
    work::fail(&mut client, &again.token, "day 2025-12-10", &test_support::provider_input_hold(days[0]), "log").await.unwrap();
    let pass10 = count(&client, "SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' AND first_session = '2025-12-10'").await;
    // audit 2026-11-22 changes-core 12: it waits for a publish in progress (which may publish the
    // day it is about to omit), then checks
    let mut other = scratch.client().await;
    let publishing = other.transaction().await.unwrap();
    publishing.execute(work::PUBLISH_LOCK, &[]).await.unwrap();
    let mut omitter = scratch.client().await;
    let waiting = tokio::spawn(async move { work::omit(&mut omitter, PROFILE, "2025-12-10", reason).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!waiting.is_finished(), "omit waits for the publish lock");
    publishing.rollback().await.unwrap();
    let omitted = waiting.await.unwrap().unwrap();
    assert_eq!((omitted.day_job, omitted.passes.clone()), (first.job_id, vec![pass10]));
    assert_eq!(omitted.cause, ("PROVIDER_INPUT_MISSING".to_owned(), "412 of 3904 final slots lack a provider input (more than 0.1)".to_owned()));
    assert_eq!((job_state(&client, first.job_id).await, job_state(&client, pass10).await), ("CANCELLED".into(), "CANCELLED".into()));
    let row = client.query_one("SELECT session::text, profile_id, reason, job_id FROM core.omitted_session", &[]).await.unwrap();
    assert_eq!((row.get::<_, String>(0), row.get::<_, String>(1), row.get::<_, String>(2), row.get::<_, i64>(3)), (days[0].into(), PROFILE.into(), reason.into(), first.job_id));
    let said = notices(&client).await;
    assert!(said.last().unwrap().starts_with(&format!("Day 2025-12-10 of FREE_REPORT_64 omitted (job {}, PROVIDER_INPUT_MISSING", first.job_id)), "{said:?}");
    assert_eq!(cause(&client, days[0]).await, ("OMITTED".into(), reason.into()));
    // once omitted, its job is no longer FAILED: nothing is left to omit
    assert!(work::omit(&mut client, PROFILE, days[0], reason).await.unwrap_err().contains("no day job of FREE_REPORT_64 covers 2025-12-10"));

    // the 11th stops on a stop that is not a hold: set aside, FAILED with its first line
    let eleventh = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let stop = test_support::provider_502_stop();
    work::fail(&mut client, &eleventh.token, "day 2025-12-11", &stop, "log").await.unwrap();
    let first_line = stop.lines().next().unwrap().to_owned();
    assert_eq!(cause(&client, days[1]).await, ("FAILED".into(), first_line.clone()));

    // the digest: one line per day set aside or omitted, with its cause code
    let s = watch::Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: 10, daily: None, archive: None, refcheck_from: None, backup_days: None };
    let now = watch::now(&client).await.unwrap();
    let digest = watch::digest(&client, &s, &now).await.unwrap();
    assert!(digest.starts_with("🟡 Running; handled: 1 date omitted · "), "{digest}");
    let lines: Vec<&str> = digest.lines().filter(|l| l.starts_with("Set aside: ") || l.starts_with("Omitted: ")).collect();
    let cut = watch::one_line(&first_line, 160);
    assert_eq!(lines, [format!("Omitted: Wed Dec 10, 2025 · OMITTED (PROVIDER_INPUT_MISSING) · {reason}"), format!("Set aside: Thu Dec 11, 2025 · FAILED · {cut}")], "{digest}");

    // audit 2026-11-22 changes-core 11: an omission states the session only while it is current.
    // Scheduled again, the 10th fails on the engine's hold: that is its cause now, not the old
    // omission; published by hand and then retired, it reads RETIRED
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, days[0], days[0], 1).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK' AND state = 'QUEUED'", &[]).await.unwrap();
    assert_eq!(cause(&client, days[0]).await.0, "NOT_COMPUTED_YET");
    let anew = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    work::fail(&mut client, &anew.token, "day 2025-12-10", &test_support::engine_fault_hold(days[0]), "log").await.unwrap();
    assert_eq!(cause(&client, days[0]).await.0, "ENGINE_FAULT", "the current failure, not the old omission");
    work::cancel(&mut client, &[anew.job_id]).await.unwrap();
    assert_eq!(cause(&client, days[0]).await.0, "NOT_SCHEDULED");
    let (mut h, sc, f) = parts(BASE);
    h.represented_date = days[0].into();
    let path = bundle(&unique("c063-omitted-then-published"), &h, &sc, &f);
    publish_file(&mut client, &path).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    let retired = "published by hand by mistake";
    let preview = publish::retire(&mut client, retired, &[days[0]], true, None).await.unwrap();
    publish::retire(&mut client, retired, &[days[0]], false, Some(&preview.token)).await.unwrap();
    assert_eq!(cause(&client, days[0]).await, ("RETIRED".into(), retired.into()));
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild (changes-core.md) 10: on a block's last attempt, a stop after its day
/// was said done (the engine prunes the day's inputs after `day_done`; `stop: prune after <day>: ...`,
/// vyreon-cli `days.rs` `prune_after`, its evidence naming the block's first day as `stopped_on` does)
/// set the NEXT day aside, untried, with that stop as its cause. The attempt is now not counted: the
/// block is queued again from the day it never tried.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_block_that_stops_after_a_done_day_is_queued_again_uncounted() {
    let (scratch, mut client, b, _) = ready("c063_prune").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 30).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, "2025-12-10").await;
    let stop = "stop: prune after 2025-12-10: No space left on device (os error 28)";
    let account = json!({"schema": core_db::evidence::SCHEMA, "stage": "day 2025-12-10", "message": stop, "session": "2025-12-10"});
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, format!("{account}\n").as_bytes()).unwrap();
    let checked = core_db::evidence::check(gz.finish().unwrap()).unwrap();
    assert!(core_db::evidence::put(&mut client, &block.token, &checked).await.unwrap());
    assert!(work::fail(&mut client, &block.token, "day 2025-12-10", stop, "log").await.unwrap(), "queued again");
    let row = client.query_one("SELECT state, attempts, next_session::text FROM core.job WHERE job_id = $1", &[&block.job_id]).await.unwrap();
    assert_eq!((row.get::<_, String>(0), row.get::<_, i32>(1), row.get::<_, String>(2)), ("QUEUED".to_owned(), 0, "2025-12-11".to_owned()));
    assert_eq!(cause(&client, "2025-12-11").await.0, "NOT_COMPUTED_YET");
    let said = notices(&client).await;
    assert!(said.last().unwrap().contains("stopped on 2025-12-10, a day it had done") && said.last().unwrap().contains("2025-12-11 was not tried"), "{said:?}");
    let again = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!((again.job_id, again.sessions.clone(), again.attempt), (block.job_id, vec!["2025-12-11".to_owned()], 1));
    scratch.drop().await;
}

/// C-049's evidence of a lease, as the worker sends it: its account line alone.
async fn evidence(client: &mut Client, token: &str, stage: &str, message: &str, session: &str) {
    let account = json!({"schema": core_db::evidence::SCHEMA, "stage": stage, "message": message, "session": session});
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, format!("{account}\n").as_bytes()).unwrap();
    let checked = core_db::evidence::check(gz.finish().unwrap()).unwrap();
    assert!(core_db::evidence::put(client, token, &checked).await.unwrap());
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 10: the supplied worker names its claim's first day
/// for a stop whose message names none (`vyreon-worker` `stopped_on`): after the 10th was done, the
/// 11th's disk guard stopped the block (`diskguard: stop, ...`, vyreon-cli `guards.rs`, no date) and its
/// evidence named the 10th, so core refunded a real failure of the 11th. Core now reads the day from
/// the message: none named, the attempt is counted, and on the last one the 11th is set aside with
/// its own cause.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_stop_that_names_no_day_is_counted() {
    let (scratch, mut client, b, _) = ready("r2_fallback_day").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 30).await.unwrap();
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE kind = 'DAY_BLOCK'", &[]).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, "2025-12-10").await;
    let stop = "diskguard: stop, /work has 11 GB free (minimum 40 GB)";
    evidence(&mut client, &block.token, "day 2025-12-10", stop, "2025-12-10").await;
    assert!(!work::fail(&mut client, &block.token, "day 2025-12-10", stop, "log").await.unwrap(), "out of attempts");
    let row = client.query_one("SELECT state, first_session::text FROM core.job WHERE job_id = $1", &[&block.job_id]).await.unwrap();
    assert_eq!((row.get::<_, String>(0), row.get::<_, String>(1)), ("FAILED".to_owned(), "2025-12-11".to_owned()));
    assert_eq!(cause(&client, "2025-12-11").await, ("FAILED".into(), stop.into()));
    scratch.drop().await;
}

/// Audit 2026-11-22 prebuild round 2 (core-r2.md) 4: core took a refund's progress on trust, so a
/// worker that named the same done day on every new lease was refunded without end while its block
/// held the later passes. A refund now needs the day the stop names to be one this lease did: the
/// first lease publishes the 10th and stops pruning after it (refunded); the next lease, which began
/// at the 11th, names the 10th again and moves nothing on: counted.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refund_needs_a_day_done_under_the_same_lease() {
    let (scratch, mut client, b, _) = ready("r2_bounded_refund").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 30).await.unwrap();
    let stop = "stop: prune after 2025-12-10: No space left on device (os error 28)";
    let first = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &first, "2025-12-10").await;
    evidence(&mut client, &first.token, "day 2025-12-10", stop, "2025-12-10").await;
    assert!(work::fail(&mut client, &first.token, "day 2025-12-10", stop, "log").await.unwrap());
    let attempts = format!("SELECT attempts FROM core.job WHERE job_id = {}", first.job_id);
    assert_eq!(client.query_one(&attempts, &[]).await.unwrap().get::<_, i32>(0), 0, "the 10th was done under this lease: refunded");
    for n in 1..=2 {
        let again = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
        assert_eq!(again.sessions, ["2025-12-11"]);
        evidence(&mut client, &again.token, "day 2025-12-10", stop, "2025-12-10").await;
        assert!(work::fail(&mut client, &again.token, "day 2025-12-10", stop, "log").await.unwrap());
        assert_eq!(client.query_one(&attempts, &[]).await.unwrap().get::<_, i32>(0), n, "a stale day names no progress of this lease: counted");
    }
    let refunds = notices(&client).await.iter().filter(|n| n.contains("the attempt not counted")).count();
    assert_eq!(refunds, 1);
    scratch.drop().await;
}

async fn job_state(client: &Client, job: i64) -> String {
    client.query_one("SELECT state FROM core.job WHERE job_id = $1", &[&job]).await.unwrap().get(0)
}

/// Audit 2026-11-14 (core.md) 6 and 5: a job whose sessions are all done is DONE when its lease
/// ends, whatever became of its `finish` (lost, late, or sent twice), so no job without a next
/// session is ever queued for a claim to choke on; a session marked done twice is answered, not
/// refused; and a lease the sweep cannot end does not keep the other leases from ending or the
/// claims from going on.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_done_job_whose_finish_is_lost_is_done_and_one_bad_lease_stops_no_claim() {
    let (scratch, mut client, b, _) = ready("lost_finish").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-11", "2025-12-12", 30).await.unwrap();

    // the worker does its one day, says so twice (the first answer lost), and dies before finish
    let first = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    done(&mut client, &first, "2025-12-10").await.unwrap();
    done(&mut client, &first, "2025-12-10").await.unwrap();
    expire(&client, &first).await;
    // the next claim ends that lease, finds the job done, and takes the next block
    let second = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!(second.sessions, ["2025-12-11", "2025-12-12"]);
    assert_eq!(job_state(&client, first.job_id).await, "DONE");
    let said: String = client.query_one("SELECT message FROM core.notice WHERE NOT post ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert_eq!(said, "Day block 2025-12-10 to 2025-12-10 done in 0 min by its worker, whose finish never arrived (1 blocks to go).");
    // its finish arriving late, or twice, is answered as done
    assert_eq!(work::finish(&mut client, &first.token).await, Ok(first.job_id));
    assert_eq!(work::finish(&mut client, &first.token).await, Ok(first.job_id));

    // a block on its last attempt that fails after its last day is done, not set aside
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE job_id = $1", &[&second.job_id]).await.unwrap();
    for day in &second.sessions {
        done(&mut client, &second, day).await.unwrap();
    }
    assert!(!work::fail(&mut client, &second.token, "cleanup", "the disk could not be emptied", "log").await.unwrap());
    assert_eq!(job_state(&client, second.job_id).await, "DONE");
    assert_eq!(count(&client, "SELECT count(*) FROM core.job").await, 2, "nothing was split off");
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap(), None);

    // two tape leases expire; ending one of them fails (made here by a trigger on its job): the
    // other is ended and its job claimed again, and the sweep names the one it could not end
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-15", "2025-12-16", 1).await.unwrap();
    let t1 = work::claim(&mut client, Kind::Tape, &b, "t1", HOUR).await.unwrap().unwrap();
    let t2 = work::claim(&mut client, Kind::Tape, &b, "t2", HOUR).await.unwrap().unwrap();
    expire(&client, &t1).await;
    expire(&client, &t2).await;
    client
        .batch_execute(&format!(
            "CREATE FUNCTION stuck() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF OLD.job_id = {} THEN RAISE EXCEPTION 'stuck'; END IF; RETURN NEW; END $$;
             CREATE TRIGGER stuck BEFORE UPDATE ON core.job FOR EACH ROW EXECUTE FUNCTION stuck();",
            t1.job_id
        ))
        .await
        .unwrap();
    let again = work::claim(&mut client, Kind::Tape, &b, "t3", HOUR).await.unwrap().unwrap();
    // (its lost lease was not charged: C-035 amended, C-068)
    assert_eq!((again.job_id, again.attempt), (t2.job_id, 1));
    let error = work::sweep(&mut client).await.unwrap_err();
    assert!(error.contains("stuck"), "{error}");
    client.batch_execute("DROP TRIGGER stuck ON core.job").await.unwrap();
    assert_eq!(work::sweep(&mut client).await.unwrap(), 1);
    assert_eq!(job_state(&client, t1.job_id).await, "QUEUED");
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn workers_claiming_at_once_each_get_a_different_job() {
    let (scratch, mut client, b, _) = ready("race").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-08", "2025-12-12", 1).await.unwrap();
    let mut claims = Vec::new();
    for i in 0..8 {
        let (mut worker, b) = (scratch.client().await, b.clone());
        claims.push(tokio::spawn(async move { work::claim(&mut worker, Kind::Tape, &b, &format!("w{i}"), HOUR).await.unwrap() }));
    }
    let mut jobs = Vec::new();
    for claim in claims {
        if let Some(c) = claim.await.unwrap() {
            jobs.push(c.job_id);
        }
    }
    jobs.sort();
    jobs.dedup();
    assert_eq!(jobs.len(), 5, "five jobs, each claimed once; three workers found nothing");
    assert_eq!(work::status(&client).await.unwrap()["live_leases"], 5);
    scratch.drop().await;
}

/// `pods` claims of tape work at the same moment (each on its own connection, released together by
/// a barrier); returns what each got.
async fn claim_at_once(scratch: &Scratch, b: &str, pods: usize) -> Vec<Option<i64>> {
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(pods));
    let mut claims = Vec::new();
    for i in 0..pods {
        let (mut worker, b, barrier) = (scratch.client().await, b.to_owned(), barrier.clone());
        claims.push(tokio::spawn(async move {
            barrier.wait().await;
            work::claim(&mut worker, Kind::Tape, &b, &format!("pod-{i}"), HOUR).await.unwrap().map(|c| c.job_id)
        }));
    }
    let mut got = Vec::new();
    for claim in claims {
        got.push(claim.await.unwrap());
    }
    got
}

/// C-065 (the claim race of 2026-11-22: 24 tape pods claiming at once, about half found "no job to
/// claim" while 195 were claimable). Many claims at once over N claimable jobs lease exactly
/// min(N, claims) distinct jobs: none finds nothing while a job is left, and none is leased twice.
/// The real calendar's sessions 2020-01-02 to 2026-12-31, a tape job each (1,759: a queue as long as
/// a full build's, so each claim's look at it takes as long as on the cluster, where the race showed);
/// several rounds, since the race needs one claim's snapshot taken before another's commit and its
/// row lock after it. Then the last 10 jobs under 32 claims: exactly 10 leased.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn claims_at_once_each_lease_a_job_while_any_is_claimable() {
    let (scratch, mut client, b, _) = ready("claim_race").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2020-01-02", "2026-12-31", 1).await.unwrap();
    let total = count(&client, "SELECT count(*) FROM core.job WHERE kind = 'TAPE' AND state = 'QUEUED'").await;
    assert_eq!(total, 1759);
    let pods = 32;
    let mut leased = Vec::new();
    for round in 0..8 {
        let got = claim_at_once(&scratch, &b, pods).await;
        let jobs: Vec<i64> = got.iter().flatten().copied().collect();
        assert_eq!(jobs.len(), pods, "round {round}: {pods} claims at once over {} claimable jobs each lease one", total as usize - leased.len());
        leased.extend(jobs);
    }
    assert_eq!(count(&client, "SELECT count(*) FROM core.claim_miss").await, 0, "no claim found nothing while work waited");
    // all but the last 10 claimable jobs cancelled: 32 claims at once lease exactly those 10
    let gone = count(&client, "WITH c AS (UPDATE core.job SET state = 'CANCELLED' WHERE state = 'QUEUED'
                                          AND job_id NOT IN (SELECT job_id FROM core.job WHERE state = 'QUEUED' ORDER BY job_id DESC LIMIT 10) RETURNING 1)
                                SELECT count(*) FROM c").await;
    assert_eq!(gone, total - 8 * pods as i64 - 10);
    let got = claim_at_once(&scratch, &b, pods).await;
    let last: Vec<i64> = got.iter().flatten().copied().collect();
    assert_eq!(last.len(), 10, "32 claims over the last 10 jobs lease all 10");
    leased.extend(last);
    assert_eq!(count(&client, "SELECT count(*) FROM core.claimable_job").await, 0);
    let mut distinct = leased.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), leased.len(), "no job leased twice");
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease WHERE ended_at IS NULL").await, leased.len() as i64);
    scratch.drop().await;
}

/// C-065, amended by audit round 2 of core-watch (6, 8): a claim that finds only work other claims
/// are taking (held FOR UPDATE, as a claim in flight holds it) answers that there is nothing and
/// keeps no miss; one that finds a claimable job no claim holds tries it again, and keeps a miss
/// naming it only when it still cannot lease it after `CLAIM_TRIES` looks; one that finds the
/// queue empty keeps nothing.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_claim_misses_only_work_no_other_claim_holds() {
    let (scratch, mut client, b, _) = ready("claim_miss").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let job = count(&client, "SELECT job_id FROM core.job WHERE kind = 'TAPE'").await;
    // audit round 2 of core-watch, 6: the job held as a claim in flight holds it (FOR UPDATE):
    // another claim taking it is healthy concurrency, no miss
    let mut holder = scratch.client().await;
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' FOR UPDATE", &[]).await.unwrap();
    assert!(work::claim(&mut client, Kind::Tape, &b, "tape-pod-1", HOUR).await.unwrap().is_none());
    assert_eq!(count(&client, "SELECT count(*) FROM core.claim_miss").await, 0, "work other claims are taking is no miss");
    drop(tx);
    // held by a lock no claim takes (an insert that references the job holds KEY SHARE on it),
    // which still stops a claim's FOR UPDATE: the job is free of claims and not leased, a miss
    // naming it
    let tx = holder.transaction().await.unwrap();
    tx.execute("SELECT FROM core.job WHERE kind = 'TAPE' FOR KEY SHARE", &[]).await.unwrap();
    assert!(work::claim(&mut client, Kind::Tape, &b, "tape-pod-1", HOUR).await.unwrap().is_none());
    let miss = client.query_one("SELECT kind, engine_build, worker, daily, claimable, job_id FROM core.claim_miss", &[]).await.unwrap();
    assert_eq!(
        (miss.get::<_, String>(0), miss.get::<_, String>(1), miss.get::<_, String>(2), miss.get::<_, bool>(3), miss.get::<_, i64>(4), miss.get::<_, Option<i64>>(5)),
        ("TAPE".to_string(), b.clone(), "tape-pod-1".to_string(), false, 1, Some(job))
    );
    // a daily worker's claim counts only daily work: none is claimable for it, so no miss
    assert!(work::claim_as(&mut client, Kind::Tape, &b, "daily-pod", HOUR, true).await.unwrap().is_none());
    drop(tx);
    let got = work::claim(&mut client, Kind::Tape, &b, "tape-pod-2", HOUR).await.unwrap().unwrap();
    // the queue empty: nothing kept
    assert!(work::claim(&mut client, Kind::Tape, &b, "tape-pod-3", HOUR).await.unwrap().is_none());
    assert_eq!(count(&client, "SELECT count(*) FROM core.claim_miss").await, 1);
    assert_eq!(got.sessions, ["2025-12-10"]);
    scratch.drop().await;
}

/// `now` moved on by `minutes`, as the database reads it.
async fn later(client: &Client, now: &str, minutes: i32) -> String {
    client.query_one("SELECT ($1::text::timestamptz + make_interval(mins => $2))::text", &[&now, &minutes]).await.unwrap().get(0)
}

/// C-065, the progress channel: the real slice's day published under a block's lease reads as one
/// line with its counts (the tally core keeps of the published bodies), the block's start and its
/// log lines follow, and the queue's counts; one window each 30 minutes, queued for the progress
/// channel only; a window in which nothing happened posts nothing.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_progress_channel_says_what_moved_every_half_hour() {
    use core_db::notice::{self, Channel};
    use core_db::progress;
    let (scratch, mut client, b, _) = ready("progress").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 2).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "day-7f2c", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &claim, "2025-12-10").await;
    let t0 = watch::now(&client).await.unwrap();

    let first = progress::tick(&mut client, PROFILE, &t0).await.unwrap().unwrap();
    assert_eq!(first.posts, 1);
    let post: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress'", &[]).await.unwrap().get(0);
    let (slots, withheld, provider): (i64, i64, i64) = {
        let r = client
            .query_one(
                "SELECT sum(slots)::bigint, sum(withheld)::bigint, sum(provider_withheld)::bigint FROM core.revision_tally t
                 JOIN vyreon.day_revision d USING (revision_id) WHERE d.represented_date = '2025-12-10' AND (d.base_revision IS NULL OR d.day_local)",
                &[],
            )
            .await
            .unwrap();
        (r.get(0), r.get(1), r.get(2))
    };
    assert!(slots > 0);
    let lines: Vec<&str> = post.lines().collect();
    assert!(lines[0].starts_with("Progress · ") && lines[0].ends_with(" MT"), "{post}");
    let reasons = match (provider, withheld - provider) {
        (0, 0) => "none withheld".to_string(),
        (p, 0) => format!("withheld: {} provider input", progress::thousands(p)),
        (0, e) => format!("withheld: {} engine fault", progress::thousands(e)),
        (p, e) => format!("withheld: {} provider input, {} engine fault", progress::thousands(p), progress::thousands(e)),
    };
    assert_eq!(
        lines[1],
        format!(
            "Published Wed Dec 10, 2025: {} slots, {} with a value, {reasons} · reference check not in yet · 0 min · block 2025-12-10 to 2025-12-11 on day-7f2c (job {})",
            progress::thousands(slots),
            progress::thousands(slots - withheld),
            claim.job_id
        )
    );
    assert_eq!(lines[2], format!("Started block 2025-12-10 to 2025-12-11 on day-7f2c (job {}, attempt 1)", claim.job_id));
    assert!(lines[3].ends_with(&format!("Engine build {b} admitted: its workers may claim work.")), "{post}");
    assert_eq!(lines[4], "Day blocks: 0 done in this window · 0 of 1 done · 1 running");
    assert_eq!(lines.len(), 5, "{post}");
    // the window is the progress channel's only: the owner's channel has nothing to send
    assert!(notice::unsent(&client, 10).await.unwrap().is_empty());
    assert_eq!(notice::unsent_on(&client, 10, Channel::Progress).await.unwrap().len(), 1);

    // not due again before 30 minutes
    let early = later(&client, &t0, 29).await;
    assert_eq!(progress::tick(&mut client, PROFILE, &early).await.unwrap(), None);
    // the block's second day and its end, in the next window
    publish_day(&mut client, &claim, "2025-12-11").await;
    work::finish(&mut client, &claim.token).await.unwrap();
    let t1 = later(&client, &t0, 30).await;
    assert_eq!(progress::tick(&mut client, PROFILE, &t1).await.unwrap().unwrap().posts, 1);
    let post: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    let lines: Vec<&str> = post.lines().collect();
    assert!(lines[1].starts_with("Published Thu Dec 11, 2025: "), "{post}");
    assert!(lines[2].ends_with("Day block 2025-12-10 to 2025-12-11 done in 0 min by day-7f2c (0 blocks to go)."), "{post}");
    assert_eq!(lines[3], "Day blocks: 1 done in this window · 1 of 1 done");
    // a window in which nothing happened: recorded, nothing posted
    let t2 = later(&client, &t1, 30).await;
    let quiet = progress::tick(&mut client, PROFILE, &t2).await.unwrap().unwrap();
    assert_eq!((quiet.lines, quiet.posts), (0, 0));
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'progress'").await, 2);
    assert_eq!(count(&client, "SELECT count(*) FROM core.progress_window").await, 3);
    scratch.drop().await;
}

/// The reference check a worker sends for `session`, as its account states it: the real B0
/// census report's counts summed over its measurements (`test_support::REFCHECK_REPORT_2025_12_16`,
/// 3,958 failures of 91,066, from a suite without rulings) with `reference_errors` as given, of
/// the revisions it checked.
fn refcheck_of(session: &str, base: i64, local: i64, reference_errors: i64) -> core_db::refcheck::Checked {
    use std::io::{Read, Write};
    let mut text = String::new();
    flate2::read::GzDecoder::new(test_support::REFCHECK_REPORT_2025_12_16).read_to_string(&mut text).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    let summary = report["summary"].as_object().unwrap();
    let sum = |k: &str| summary.values().map(|m| m[k].as_i64().unwrap_or(0)).sum::<i64>();
    let account = json!({
        "schema": core_db::refcheck::SCHEMA, "session": session, "outcome": "COMPLETED", "why": null,
        "wall_seconds": 2460.5, "suite_seconds": 2400.25, "cpu_seconds": 16800.0, "cpu_complete": true, "jobs": 8, "deadline_seconds": 7200,
        "revisions": [base, local],
        "counts": {"checked": sum("checked"), "passed": sum("passed"), "failed": sum("failed"), "skipped": sum("skipped"),
                   "reference_errors": reference_errors, "failure_classes": report["failure_classes"].as_array().unwrap().len(),
                   "measurements_failing": summary.values().filter(|m| m["failed"].as_i64().unwrap_or(0) > 0).count()},
        "summary": report["summary"], "report_trimmed": null,
    });
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(account.to_string().as_bytes()).unwrap();
    core_db::refcheck::check(gz.finish().unwrap(), session).unwrap()
}

/// Audit round 2 of core-watch, 2 and 10: progress chose a day's reference check by its base alone,
/// so a day-local revision published again on the same base read as checked by the report of the
/// one it replaced; and reference errors were left out of the verdict. Now the day line takes only
/// a report of both its revisions (C-054) and states the reference errors beside the verdict.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn progress_states_a_check_only_of_the_revisions_it_checked() {
    use core_db::progress;
    let (scratch, mut client, b, _) = ready("r2w_progress_check").await;
    admit(&client, &b).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "day-7f2c", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &claim, day).await;
    let base = base_of(&client, day).await;
    let local = count(&client, "SELECT revision_id FROM vyreon.day_revision WHERE day_local AND state = 'PUBLISHED'").await;
    core_db::refcheck::put(&mut client, &claim.token, &refcheck_of(day, base, local, 9)).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    let posts = progress::preview(&client, PROFILE, &now).await.unwrap();
    let day_line = posts[0].lines().nth(1).unwrap().to_string();
    assert!(
        day_line.contains(" · reference check: 3,958 failures of 91,066, not triaged against rulings; 9 reference errors not judged · "),
        "{day_line}"
    );
    // the day-local revision published again by hand on the same base: its line has no check yet
    let again = day_local(&mut client, &b, day, base, None).await;
    assert_ne!(again, local);
    let now = watch::now(&client).await.unwrap();
    let posts = progress::preview(&client, PROFILE, &now).await.unwrap();
    let lines: Vec<&str> = posts[0].lines().collect();
    assert_eq!(lines[1], day_line, "the first revision's line keeps its own check");
    assert!(lines[2].starts_with("Published Wed Dec 10, 2025: ") && lines[2].contains(" · reference check not in yet"), "{}", posts[0]);
    assert!(!posts[0].contains("Reference check of"), "the report is stated once, on the line of what it checked: {}", posts[0]);
    scratch.drop().await;
}

/// Audit round 2 of core-watch, 11: a day-local revision published by hand inside a running
/// block's dates was credited to the block's worker, and its time shortened the block's next day.
/// What published a revision is now recorded (`core.lease_revision`): the hand revision has no
/// block and no minutes, and the block's next day counts from the block's own previous day. The
/// publication times are set on the test's clock (granted, +10, +50 by hand, +60 min).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn progress_credits_a_day_only_to_the_lease_that_published_it() {
    use core_db::progress;
    let (scratch, mut client, b, _) = ready("r2w_progress_lease").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-11", 2).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "day-7f2c", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &claim, "2025-12-10").await;
    let tenth = count(&client, "SELECT revision_id FROM vyreon.day_revision WHERE day_local AND state = 'PUBLISHED'").await;
    let base = base_of(&client, "2025-12-10").await;
    let by_hand = day_local(&mut client, &b, "2025-12-10", base, None).await;
    publish_day(&mut client, &claim, "2025-12-11").await;
    let eleventh = count(&client, "SELECT revision_id FROM vyreon.day_revision WHERE day_local AND state = 'PUBLISHED' AND represented_date = '2025-12-11'").await;
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease_revision").await, 4, "the block's two bases and two day-local revisions");
    for (revision, minutes) in [(tenth, 10), (by_hand, 50), (eleventh, 60)] {
        client
            .execute(
                "UPDATE vyreon.day_revision SET published_at = (SELECT granted_at FROM core.lease WHERE job_id = $2) + make_interval(mins => $3) WHERE revision_id = $1",
                &[&revision, &claim.job_id, &minutes],
            )
            .await
            .unwrap();
    }
    let now: String = client
        .query_one("SELECT (granted_at + interval '61 minutes')::text FROM core.lease WHERE job_id = $1", &[&claim.job_id])
        .await
        .unwrap()
        .get(0);
    let posts = progress::preview(&client, PROFILE, &now).await.unwrap();
    let lines: Vec<&str> = posts[0].lines().collect();
    assert!(lines[1].starts_with("Published Wed Dec 10, 2025: ") && lines[1].ends_with(" · reference check not in yet"), "by hand: no block, no minutes: {}", posts[0]);
    assert!(
        lines[2].starts_with("Published Thu Dec 11, 2025: ")
            && lines[2].ends_with(&format!(" · reference check not in yet · 50 min · block 2025-12-10 to 2025-12-11 on day-7f2c (job {})", claim.job_id)),
        "{}",
        posts[0]
    );
    scratch.drop().await;
}

/// Audit round 2 of core-watch, 12: an ordered pass done, then an earlier day published again in
/// the same window: the pass is queued again (C-037) with no `finished_at`, and the window counted
/// it as not done. A lease that ended DONE in the window counts its job done in it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_pass_done_then_queued_again_counts_as_done_in_its_window() {
    use core_db::progress;
    let (scratch, mut client, b, d) = ready("r2w_progress_done").await;
    admit(&client, &b).await;
    admit(&client, &d).await;
    let day = "2025-12-10";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, day, day, 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "day-7f2c", HOUR).await.unwrap().unwrap();
    publish_day(&mut client, &block, day).await;
    work::finish(&mut client, &block.token).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, day, day, 1).await.unwrap();
    assert_eq!(run_pass(&mut client, &d).await.as_deref(), Some(day));
    base_by_hand(&mut client, day).await;
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE kind = 'ORDERED_PASS' AND state = 'QUEUED'").await, 1, "the pass is queued again");
    let now = watch::now(&client).await.unwrap();
    let posts = progress::preview(&client, PROFILE, &now).await.unwrap();
    assert!(posts[0].lines().any(|l| l == "Ordered passes: 1 done in this window · 0 of 1 done · 1 waiting"), "{}", posts[0]);
    scratch.drop().await;
}

/// Audit round 2 of core-watch, 9 and the retention note: a pause whose transaction began before a
/// window ended and committed after it was composed was stamped inside that window (its
/// transaction's start), so the next window left it out by its time and the channel never said
/// it. Each window now records what it stated and the next takes what is not stated yet; windows
/// and posts done with are kept a week.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_event_committed_after_its_window_is_in_the_next_and_windows_are_kept_a_week() {
    use core_db::notice::{self, Channel};
    use core_db::progress;
    let (scratch, mut client, _, _) = ready("r2w_progress_late").await;
    let mut pauser = scratch.client().await;
    let pausing = pauser.transaction().await.unwrap();
    work::pause(&pausing, "a check failed").await.unwrap();
    let t0 = watch::now(&client).await.unwrap();
    let first = progress::tick(&mut client, PROFILE, &t0).await.unwrap().unwrap();
    assert_eq!(first.posts, 0, "the pause is not committed when the window is composed");
    pausing.commit().await.unwrap();
    let t1 = later(&client, &t0, 30).await;
    assert_eq!(progress::tick(&mut client, PROFILE, &t1).await.unwrap().unwrap().posts, 1);
    let post: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress'", &[]).await.unwrap().get(0);
    assert!(post.lines().any(|l| l.ends_with(" Queue paused: a check failed. No work is handed out until `vyreon-core queue resume`.")), "{post}");
    let t2 = later(&client, &t1, 30).await;
    assert_eq!(progress::tick(&mut client, PROFILE, &t2).await.unwrap().unwrap().posts, 0, "stated once");

    // a week on: the windows before are gone with what they stated, the newest kept; the post
    // delivered is gone, one still to send is kept
    let post_id = notice::next_on(&client, 120, Channel::Progress).await.unwrap().unwrap().id;
    notice::sent(&client, post_id).await.unwrap();
    notice::progress(&client, "progress test unsent", "Progress · unsent").await.unwrap();
    let week: String = client.query_one("SELECT (now() + interval '7 days 2 hours')::text", &[]).await.unwrap().get(0);
    assert_eq!(count(&client, "SELECT count(*) FROM core.progress_window").await, 3);
    progress::tick(&mut client, PROFILE, &week).await.unwrap().unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM core.progress_window").await, 1, "only the newest window is kept");
    assert_eq!(count(&client, "SELECT count(*) FROM core.progress_shown").await, 0);
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'progress'").await, 1, "the post not sent yet stays");
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn cancelling_a_job_ends_its_lease_and_takes_it_out_of_the_queue() {
    let (scratch, mut client, b, _) = ready("cancel").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-08", "2025-12-10", 1).await.unwrap();
    let held = work::claim(&mut client, Kind::Tape, &b, "w1", HOUR).await.unwrap().unwrap();
    let queued = count(&client, "SELECT min(job_id) FROM core.job WHERE state = 'QUEUED'").await;
    assert_eq!(work::cancel(&mut client, &[held.job_id, queued]).await.unwrap(), 2);
    // the worker holding the cancelled job is refused; the remaining job is still handed out
    assert!(work::heartbeat(&mut client, &held.token, HOUR).await.is_err());
    assert_eq!(count(&client, "SELECT count(*) FROM core.lease WHERE outcome = 'CANCELLED'").await, 1);
    let next = work::claim(&mut client, Kind::Tape, &b, "w2", HOUR).await.unwrap().unwrap();
    assert!(next.job_id != held.job_id && next.job_id != queued);
    done(&mut client, &next, "2025-12-10").await.unwrap();
    work::finish(&mut client, &next.token).await.unwrap();
    // a finished job stays finished, and nothing is left to claim
    assert_eq!(work::cancel(&mut client, &[next.job_id]).await.unwrap(), 0);
    assert!(work::claim(&mut client, Kind::Tape, &b, "w3", HOUR).await.unwrap().is_none());
    scratch.drop().await;
}

/// Round cc, 11 (C-049's principle): a worker's failure message carrying a credential pattern its
/// redaction missed is withheld whole wherever core keeps it: the lease's failure, the log's
/// requeue and pause notices, the queue's pause reason. (The text is the worker API test's
/// unredacted-key example.)
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failure_message_with_a_credential_is_never_kept() {
    let (scratch, mut client, base, _) = ready("work_scrub").await;
    admit(&client, &base).await;
    work::schedule(&mut client, Kind::Tape, "FREE_REPORT_64", &base, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let leaked = "replay_capture failed: GET https://api-v2.intrinio.com/x?api_key=OmRlYWRiZWVm: HTTP 500";
    let mut attempts = 0;
    while let Some(claim) = work::claim(&mut client, Kind::Tape, &base, "tape-1", 3600.0).await.unwrap() {
        work::fail(&mut client, &claim.token, "tape 2025-12-10", leaked, leaked).await.unwrap();
        attempts += 1;
    }
    assert!(attempts > 1);
    let kept: Vec<String> = client
        .query(
            "SELECT failure FROM core.lease UNION ALL SELECT failure_log FROM core.lease UNION ALL SELECT failed_stage FROM core.lease
             UNION ALL SELECT message FROM core.notice UNION ALL SELECT reason FROM core.queue_control",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .filter_map(|r| r.get::<_, Option<String>>(0))
        .collect();
    assert!(kept.iter().any(|t| t.contains("(not kept: it carried a credential pattern, an unredacted api_key=)")), "{kept:?}");
    assert!(!kept.iter().any(|t| t.contains("OmRlYWRiZWVm")), "{kept:?}");
    assert_eq!(count(&client, "SELECT count(*) FROM core.queue_control WHERE paused").await, 1, "the tape job out of attempts paused the queue");
    scratch.drop().await;
}

/// C-066: KEDA's trigger counts, as its own login in `vyreon_scaler`, exactly what a worker of the
/// build can claim, by kind, and the daily runs' apart; the login reads nothing else.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_scaler_counts_what_a_worker_of_the_build_can_claim_and_reads_nothing_else() {
    let (scratch, mut client, b, d) = ready("scaler").await;
    let login = format!("scaler_test_{}", std::process::id());
    client.batch_execute(&format!("DROP ROLE IF EXISTS {login}; CREATE ROLE {login} LOGIN PASSWORD 'scaler-test' IN ROLE vyreon_scaler")).await.unwrap();
    let mut config = scratch.config.clone();
    config.user(&login).password("scaler-test");
    let scaler = test_support::connect(&config).await;
    // what the trigger of each ScaledJob asks (deploy/k8s/50-keda.yaml)
    async fn counted(scaler: &Client, kind: &str, build: &str, daily: Option<bool>) -> i64 {
        scaler.query_one("SELECT core.claimable_count($1, $2, $3)", &[&kind, &build, &daily]).await.unwrap().get(0)
    }
    async fn claimable(client: &Client) -> Value {
        work::status(client).await.unwrap()["claimable"].clone()
    }

    work::schedule(&mut client, Kind::Tape, PROFILE, &b, "2025-12-10", "2025-12-12", 1).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-11", 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, "2025-12-10", "2025-12-11", 1).await.unwrap();
    // not admitted: nothing to start a pod for
    assert_eq!(counted(&scaler, "TAPE", &b, None).await, 0);
    admit(&client, &b).await;
    admit(&client, &d).await;
    // the block waits for the tapes up to its last session, the passes for their days
    assert_eq!(claimable(&client).await, json!({"TAPE": 3}));
    assert_eq!(counted(&scaler, "TAPE", &b, None).await, 3);
    assert_eq!(counted(&scaler, "DAY_BLOCK", &b, None).await, 0);
    assert_eq!(counted(&scaler, "ORDERED_PASS", &d, None).await, 0);
    // another build's ScaledJob starts nothing for this build's work
    assert_eq!(counted(&scaler, "TAPE", &d, None).await, 0);
    assert_eq!(counted(&scaler, "TAPE", "another-build", None).await, 0);
    // a daily tape (priority 1, as `daily::queue` sets it) is counted apart
    let tape12: i64 = client.query_one("SELECT job_id FROM core.job WHERE kind = 'TAPE' AND first_session = '2025-12-12'", &[]).await.unwrap().get(0);
    client.execute("UPDATE core.job SET priority = 1 WHERE job_id = $1", &[&tape12]).await.unwrap();
    assert_eq!((counted(&scaler, "TAPE", &b, Some(true)).await, counted(&scaler, "TAPE", &b, Some(false)).await), (1, 2));
    // a claimed job is no longer counted: the pod that holds it is KEDA's running Job
    let mut tapes = Vec::new();
    for worker in ["t1", "t2"] {
        tapes.push(work::claim(&mut client, Kind::Tape, &b, worker, HOUR).await.unwrap().unwrap());
    }
    assert_eq!(tapes[0].job_id, tape12, "the daily tape first");
    assert_eq!(counted(&scaler, "TAPE", &b, None).await, 1);
    for tape in &tapes {
        done(&mut client, tape, &tape.sessions[0]).await.unwrap();
        work::finish(&mut client, &tape.token).await.unwrap();
    }
    let last = work::claim(&mut client, Kind::Tape, &b, "t3", HOUR).await.unwrap().unwrap();
    done(&mut client, &last, &last.sessions[0]).await.unwrap();
    work::finish(&mut client, &last.token).await.unwrap();
    assert_eq!(claimable(&client).await, json!({"DAY_BLOCK": 1}));
    assert_eq!((counted(&scaler, "TAPE", &b, None).await, counted(&scaler, "DAY_BLOCK", &b, None).await), (0, 1));
    assert_eq!(counted(&scaler, "DAY_BLOCK", &b, Some(true)).await, 0, "the block is the backfill's, not a daily run's");

    // the block publishes the 10th (the real slice): its pass is claimable, counted through the
    // view's functions under the definer's rights and fixed search_path
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    assert_eq!(counted(&scaler, "DAY_BLOCK", &b, None).await, 0);
    done(&mut client, &block, "2025-12-09").await.unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle(&unique("scaler-base"), &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(counted(&scaler, "ORDERED_PASS", &d, None).await, 0, "its block is still on the 10th");
    day_local(&mut client, &b, "2025-12-10", base.revision_id, Some(&block.token)).await;
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    assert_eq!(claimable(&client).await, json!({"ORDERED_PASS": 1}));
    assert_eq!(counted(&scaler, "ORDERED_PASS", &d, None).await, 1);
    assert_eq!(counted(&scaler, "ORDERED_PASS", &b, None).await, 0, "the pass is the derived build's");
    // a paused queue starts nothing
    work::pause(&client, "scaler test").await.unwrap();
    assert_eq!(counted(&scaler, "ORDERED_PASS", &d, None).await, 0);

    // the login can run the count and read nothing else
    for sql in ["SELECT count(*) FROM core.claimable_job", "SELECT count(*) FROM core.job", "SELECT count(*) FROM vyreon.day_revision", "SELECT core.build_runs('x')"] {
        let refused = scaler.query_one(sql, &[]).await.unwrap_err();
        assert!(refused.as_db_error().is_some_and(|e| e.message().contains("permission denied")), "{sql}: {refused}");
    }
    assert!(scaler.execute("UPDATE core.queue_control SET paused = false", &[]).await.is_err());
    drop(scaler);
    scratch.drop().await;
    let admin = test_support::connect(&std::env::var("VYREON_CORE_TEST_PG").unwrap().parse().unwrap()).await;
    admin.batch_execute(&format!("DROP ROLE {login}")).await.unwrap();
}

/// C-070 (audit 2026-11-23, Codex 1, Gemini 11): a bundle of a day before the lease's job begins is
/// not this job's to send again. It was answered "already published" when another job of the same
/// build had published that day the same way (retry recognition checked the job's last session, not
/// its first), and the worker took that answer for its own day's.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_before_the_job_is_not_answered_as_already_published() {
    let (scratch, mut client, b, _) = ready("before_job").await;
    admit(&client, &b).await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let first = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle("before-job-base", &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &first.token).await.unwrap();
    day_local(&mut client, &b, "2025-12-10", base.revision_id, Some(&first.token)).await;
    work::day_done(&mut client, &first.token, "2025-12-10").await.unwrap();
    work::finish(&mut client, &first.token).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-11", "2025-12-11", 30).await.unwrap();
    let next = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    let r = publish_for_lease(&mut client, &path, &next.token).await;
    assert!(matches!(&r, Err(LeaseError::Refused(m)) if m.contains("expects 2025-12-11")), "{r:?}");
    std::fs::remove_file(&path).unwrap();
    scratch.drop().await;
}
