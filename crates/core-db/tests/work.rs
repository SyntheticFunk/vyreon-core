//! The job queue on real sessions (the engine's calendar at engine-v1, `fixtures/engine-v1-sessions.json`),
//! the real builds of the droplet's 2025-12-10 and, for the ordered pass, that day's slice
//! published. Run with `cargo test -- --ignored`.
mod common;
use common::Scratch;
use common::slice::{BASE, DERIVED, bundle, inventories, parts};
use core_db::publish::publish_file;
use core_db::work::{self, Claim, Kind, LeaseError};
use core_db::{build, migrate};
use serde_json::{Value, json};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// A migrated database with the calendar and the slice's two builds; returns their ids (base, derived).
async fn ready(test: &str) -> (Scratch, Client, String, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    let calendar: Value = serde_json::from_str(include_str!("fixtures/engine-v1-sessions.json")).unwrap();
    assert_eq!(work::load_calendar(&mut client, &calendar).await.unwrap(), 2261);
    let mut builds = Vec::new();
    for inventory in inventories() {
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

async fn ordered(client: &mut Client, build: &str) -> Option<Claim> {
    work::claim(client, Kind::OrderedPass, build, "pass", HOUR).await.unwrap()
}

async fn expire(client: &Client, claim: &Claim) {
    client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1 AND ended_at IS NULL", &[&claim.job_id]).await.unwrap();
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
    assert_eq!(work::finish(&mut client, &tape10.token).await, Err(LeaseError::Gone));
    // tapes for the 10th and 11th are in: that block is free, the 12th's still waits for its tape
    let second_block = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!(second_block.sessions, ["2025-12-10", "2025-12-11"]);
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap(), None);
    assert_eq!(work::status(&client).await.unwrap()["live_leases"], 2);
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_lost_workers_block_resumes_and_a_job_out_of_attempts_pauses_the_queue() {
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
    let second = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!((second.job_id, second.sessions.clone(), second.attempt), (first.job_id, vec!["2025-12-11".to_owned(), "2025-12-12".to_owned()], 2));

    // a worker that fails says where and sends its log; the job is tried a third time
    assert!(work::fail(&mut client, &second.token, "surfaces", "surface fit did not converge for XLB", "…the day's log…").await.unwrap());
    let third = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!((third.job_id, third.attempt), (first.job_id, 3));
    // the third failure is the last: the job stays failed and the queue stops
    assert!(!work::fail(&mut client, &third.token, "intent B", "worker ran out of disk", "log").await.unwrap());
    let status = work::status(&client).await.unwrap();
    assert_eq!(status["paused"], true);
    assert_eq!(status["pause_reason"], format!("job {} failed for the last time: intent B: worker ran out of disk", first.job_id));
    assert_eq!(status["claimable"], json!({}));
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap(), None, "the other block is not claimed while paused");
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

    // once someone has looked: the first reason is reported, and the failed job starts afresh
    work::pause(&client, "a later reason").await.unwrap();
    assert!(work::resume(&client).await.unwrap().unwrap().contains("failed for the last time"));
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    let fourth = work::claim(&mut client, Kind::DayBlock, &b, "w4", HOUR).await.unwrap().unwrap();
    assert_eq!((fourth.job_id, fourth.attempt, fourth.sessions.len()), (first.job_id, 1, 2));
    // a lease that expires on the last attempt also fails the job and pauses the queue
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE job_id = $1", &[&fourth.job_id]).await.unwrap();
    expire(&client, &fourth).await;
    assert_eq!(work::sweep(&client).await.unwrap(), 1);
    assert_eq!(work::status(&client).await.unwrap()["paused"], true);
    // a withdrawn build's jobs are not claimable either
    work::resume(&client).await.unwrap();
    build::withdraw(&client, &b, "a wrong number on 2025-12-10").await.unwrap();
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "w5", HOUR).await.unwrap(), None);
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

    // the worker publishes the 10th (the real slice): the ordered pass may take that day, and only it
    let (header, scalars, files) = parts(BASE);
    let path = bundle("ordered-base", &header, &scalars, &files);
    let base = publish_file(&mut client, &path).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    done(&mut client, &block, "2025-12-10").await.unwrap();
    let tenth = ordered(&mut client, &d).await.unwrap();
    assert_eq!(tenth.sessions, ["2025-12-10"]);
    assert_eq!(ordered(&mut client, &d).await, None, "one at a time, and the 11th is not published");

    // it publishes its revision on the base it read, and is done
    let (mut dh, ds, df) = parts(DERIVED);
    dh.base_revision = Some(base.revision_id);
    let path = bundle("ordered-derived", &dh, &ds, &df);
    publish_file(&mut client, &path).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    done(&mut client, &tenth, "2025-12-10").await.unwrap();
    work::finish(&mut client, &tenth.token).await.unwrap();
    assert_eq!(ordered(&mut client, &d).await, None, "the 11th still waits for its base day");
    assert_eq!(work::status(&client).await.unwrap()["claimable"], json!({}));
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
