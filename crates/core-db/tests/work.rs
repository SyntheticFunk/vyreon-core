//! The job queue on real sessions (the engine's calendar at engine-v1, `fixtures/engine-v1-sessions.json`),
//! the real builds of the droplet's 2025-12-10 and, for the ordered pass, that day's slice
//! published. Run with `cargo test -- --ignored`.
use test_support::Scratch;
use test_support::slice::{BASE, DERIVED, bundle, inventories, parts};
use core_db::publish::{publish_file, publish_for_lease};
use core_db::work::{self, Claim, Kind, LeaseError};
use core_db::{build, migrate, universe};
use serde_json::{Value, json};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// A migrated database with the calendar and the slice's two builds; returns their ids (base, derived).
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

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
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
    let second = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    assert_eq!((second.job_id, second.sessions.clone(), second.attempt), (first.job_id, vec!["2025-12-11".to_owned(), "2025-12-12".to_owned()], 2));

    // a worker that fails says where and sends its log; the job is tried a third time
    assert!(work::fail(&mut client, &second.token, "surfaces", "surface fit did not converge for XLB", "…the day's log…").await.unwrap());
    let third = work::claim(&mut client, Kind::DayBlock, &b, "w3", HOUR).await.unwrap().unwrap();
    assert_eq!((third.job_id, third.attempt), (first.job_id, 3));
    // the third failure is the last: the 11th is set aside, the 10th's job is done, the 12th is
    // queued as a block of its own, and the queue goes on (C-035)
    assert!(!work::fail(&mut client, &third.token, "intent B", "worker ran out of disk", "log").await.unwrap());
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

    // a lease that expires on the last attempt sets its day aside too; the third failed day
    // stops the queue
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE state <> 'FAILED'", &[]).await.unwrap();
    expire(&client, &rest).await;
    assert_eq!(work::sweep(&mut client).await.unwrap(), 1);
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
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, 1, "the refused bundles left nothing");

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
    client.query("SELECT message FROM core.notice ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
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
    let path = bundle(&format!("derive-{day}-{base}"), &h, &s, &f);
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

    // the 10th's base is published again: the 10th's derived revision was computed from the old
    // base and the 11th's read the 10th, so both fall, and both passes are queued again
    let (h, s, f) = parts(BASE);
    let again = publish_file(&mut client, &bundle("rederive-again", &h, &s, &f)).await.unwrap();
    assert_eq!(again.superseded, 3, "the old base, the 10th's derived revision and the 11th's");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL").await, 0);
    assert_eq!(pass_jobs(&client).await, done("QUEUED", 0));
    let said = notices(&client).await;
    assert!(said.last().unwrap().contains("the ordered pass derives 2 days again, 2025-12-10 to 2025-12-11"), "{said:?}");

    // the 10th first, the 11th behind it; the base changes again while the 10th's pass runs: the
    // pass is stopped (its lease is gone) and what it computed on the old base is not taken
    let tenth = ordered(&mut client, &d).await.unwrap();
    assert_eq!((tenth.sessions.as_slice(), ordered(&mut client, &d).await), (&["2025-12-10".to_owned()][..], None));
    let third = publish_file(&mut client, &bundle("rederive-third", &h, &s, &f)).await.unwrap();
    assert_eq!(derive(&mut client, days[0], again.revision_id, &tenth.token).await, Err(LeaseError::Gone));
    assert_eq!(count(&client, &format!("SELECT count(*) FROM core.lease WHERE job_id = {} AND outcome = 'CANCELLED'", tenth.job_id)).await, 1);
    assert_eq!(pass_jobs(&client).await, done("QUEUED", 0), "the stopped attempt is not counted against the pass");

    // and if a publish of a day it reads came after its lease was granted without stopping it, the
    // pass is refused all the same (made here by moving the grant back)
    let tenth = ordered(&mut client, &d).await.unwrap();
    client.execute("UPDATE core.lease SET granted_at = granted_at - interval '1 hour' WHERE job_id = $1 AND ended_at IS NULL", &[&tenth.job_id]).await.unwrap();
    let refused = derive(&mut client, days[0], third.revision_id, &tenth.token).await.unwrap_err();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.contains(&format!("revision {} of 2025-12-10 was published after job {}", third.revision_id, tenth.job_id))), "{refused}");
    client.execute("UPDATE core.lease SET granted_at = clock_timestamp() WHERE job_id = $1 AND ended_at IS NULL", &[&tenth.job_id]).await.unwrap();
    derive(&mut client, days[0], third.revision_id, &tenth.token).await.unwrap();
    work::finish(&mut client, &tenth.token).await.unwrap();
    let eleventh = ordered(&mut client, &d).await.unwrap();
    derive(&mut client, days[1], bases[1], &eleventh.token).await.unwrap();
    work::finish(&mut client, &eleventh.token).await.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE state = 'PUBLISHED'").await, 4, "two bases and their two derived revisions");
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
    let said: String = client.query_one("SELECT message FROM core.notice ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert_eq!(said, "Day block 2025-12-10 to 2025-12-10 done by its worker, whose finish never arrived (1 blocks to go).");
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
    assert_eq!((again.job_id, again.attempt), (t2.job_id, 2));
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
