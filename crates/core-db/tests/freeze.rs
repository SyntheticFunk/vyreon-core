//! The freeze release's core side (2026-11-25, engine 93a04676): the option prints' retention counted from
//! each session's open (C-081, engine D-512 contract a), every tape captured again under the new reader
//! identity (C-083, engine D-472/D-480), and the owner's phase-1 stop rule (C-084). On the engine's calendar,
//! the universe and the real slice of 2025-12-10. Run with `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::watch::{self, Settings};
use core_db::work::{self, Claim, Kind};
use core_db::stoprule::Tally;
use core_db::{build, migrate, progress, stoprule, tape, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use std::io::Write;
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

async fn ready(test: &str) -> (Scratch, Client, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "freeze test"})).await.unwrap();
    (scratch, client, b)
}

fn settings() -> Settings {
    Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None,
        spend: None,
        recompute: None,
        refcheck_rerun_hours: None,
    }
}

/// C-081 (engine D-512 contract a): a session counts toward the prints' five once it has opened, at 09:30
/// New York, as the engine's `fill::prints_retained` counts it. On Monday 2026-11-09 Monday's session opens
/// at 14:30Z (EST): before it, the five are 11-02..11-06 and 11-02's prints are still served; from it, 11-02
/// is past them. The date form (the planners') counts the sessions before the UTC date: 11-02 still served
/// all of Monday.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_session_counts_toward_the_prints_retention_once_it_has_opened() {
    let (scratch, client, _) = ready("freeze_prints").await;
    let at = async |day: &str, instant: &str| -> bool {
        client.query_one("SELECT core.prints_perished_at($1::text::date, $2::text::timestamptz)", &[&day, &instant]).await.unwrap().get(0)
    };
    // Sunday 20:00 New York (Monday 01:00Z): Monday has not opened, so it does not push 11-02 out
    assert!(!at("2026-11-02", "2026-11-09T01:00:00Z").await);
    assert!(!at("2026-11-02", "2026-11-09T14:29:59Z").await);
    assert!(at("2026-11-02", "2026-11-09T14:30:00Z").await, "Monday opened: 11-03..11-09 are the five");
    assert!(!at("2026-11-03", "2026-11-09T14:30:00Z").await);
    // in summer, New York is UTC-4: the open is 13:30Z
    assert!(!at("2027-06-01", "2027-06-08T13:29:00Z").await);
    assert!(at("2027-06-01", "2027-06-08T13:30:00Z").await);
    // before the prints were kept at all (core.source_coverage 'option_prints', 2026-10-23): never perished
    assert!(!at("2026-10-01", "2026-11-25T18:00:00Z").await);
    let dated: bool = client.query_one("SELECT core.prints_perished('2026-11-02', '2026-11-09')", &[]).await.unwrap().get(0);
    assert!(!dated, "the date form: the five before the UTC date");
    scratch.drop().await;
}

/// C-083: a build whose tape-reader identity is new captures every tape again: one TAPE job a session from
/// the replay files' first session to the last before today, re-runnable; day blocks wait for the tapes up
/// to their last session; the progress post says how far it is and what waits.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn every_tape_is_captured_again_for_a_new_reader_and_the_backfill_waits_for_it() {
    let (scratch, mut client, b) = ready("freeze_tapes").await;
    assert!(tape::recapture(&mut client, PROFILE, "not-a-build", None, None).await.unwrap_err().contains("not registered"));
    // the whole range by default: the replay files' first session to the last before today
    let whole = tape::recapture(&mut client, PROFILE, &b, None, Some("2023-06-30")).await.unwrap();
    assert_eq!((whole.first.as_str(), whole.last.as_str(), whole.jobs, whole.kept), ("2023-05-24", "2023-06-30", 26, 0));
    let last: String = client
        .query_one("SELECT max(session)::text FROM core.session WHERE session < (now() AT TIME ZONE 'America/New_York')::date", &[])
        .await
        .unwrap()
        .get(0);
    let again = tape::recapture(&mut client, PROFILE, &b, None, None).await.unwrap();
    assert_eq!((again.last.as_str(), again.kept), (last.as_str(), 26), "run again: only what is missing");
    let held: i64 = client.query_one("SELECT count(*) FROM core.session WHERE session BETWEEN '2023-05-24' AND $1::text::date", &[&last]).await.unwrap().get(0);
    assert_eq!(again.jobs as i64 + again.kept, held);
    assert_eq!(tape::recapture(&mut client, PROFILE, &b, None, None).await.unwrap().jobs, 0);
    // claimed oldest first; a day block waits for every tape up to its last session
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2023-06-01", "2023-06-02", 30).await.unwrap();
    assert!(work::claim(&mut client, Kind::DayBlock, &b, "day-1", HOUR).await.unwrap().is_none(), "the tapes up to 06-02 are still to capture");
    let first = work::claim(&mut client, Kind::Tape, &b, "tape-1", HOUR).await.unwrap().unwrap();
    assert_eq!(first.sessions, ["2023-05-24"]);
    let now: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    let post = progress::preview(&mut client, PROFILE, &now).await.unwrap().join("\n");
    assert!(post.contains(&format!("Tapes for the new reader: 0 of {held} sessions captured; backfill days from May 24 wait for them")), "{post}");
    // the tapes to 06-02 done (as their workers finish them): the block may run
    client.execute("UPDATE core.job SET state = 'DONE', next_session = NULL, finished_at = now() WHERE kind = 'TAPE' AND first_session <= '2023-06-02'", &[]).await.unwrap();
    client.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE job_id = $1", &[&first.job_id]).await.unwrap();
    assert_eq!(work::claim(&mut client, Kind::DayBlock, &b, "day-1", HOUR).await.unwrap().unwrap().sessions[0], "2023-06-01");
    let post = progress::preview(&mut client, PROFILE, &now).await.unwrap().join("\n");
    assert!(post.contains(&format!("Tapes for the new reader: 7 of {held} sessions captured; backfill days from Jun 5 wait for them")), "{post}");
    scratch.drop().await;
}

async fn publish_day(client: &mut Client, claim: &Claim, build: &str, day: &str) -> (i64, i64) {
    let (mut h, s, f) = parts(BASE);
    h.represented_date = day.into();
    let path = bundle(&format!("fr-base-{day}"), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(build, day, base);
    let path = bundle(&format!("fr-local-{day}"), &lh, &ls, &lf);
    let local = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    work::day_done(client, &claim.token, day).await.unwrap();
    (base, local)
}

/// A completed check of `day` stored as `put` stores one, with the counts of the cluster's recorded report of
/// 2026-11-02 (92,767 checked, 1,982 failed) and `unexplained` of them not explained by the rulings.
async fn checked(client: &Client, lease: i64, day: &str, base: i64, local: i64, unexplained: i32) {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(json!({"schema": core_db::refcheck::SCHEMA, "session": day, "outcome": "COMPLETED"}).to_string().as_bytes()).unwrap();
    let doc = gz.finish().unwrap();
    client
        .execute(
            "INSERT INTO core.refcheck_report (lease_id, job_id, attempt, worker, engine_build, session, base_revision, day_local_revision, outcome,
                                               checked, passed, failed, skipped, reference_errors, failure_classes, measurements_failing,
                                               wall_seconds, cpu_complete, jobs, deadline_seconds, report_bytes, document_sha256, document, failed_unexplained)
             SELECT l.lease_id, l.job_id, 1, l.worker, j.engine_build, $2::text::date, $3, $4, 'COMPLETED',
                    92767, 90785, 1982, 7155, 9, 167, 26, 1850.8, true, 8, 36000, 0, $5, $6, $7
             FROM core.lease l JOIN core.job j USING (job_id) WHERE l.lease_id = $1",
            &[&lease, &day, &base, &local, &core_contract::sha256_hex(&doc), &doc, &unexplained],
        )
        .await
        .unwrap();
}

/// C-084 (owner, 2026-11-25: MILESTONES' PHASE-1 RUN RULE), amended by C-090 (owner, 2026-11-26: stop only for
/// systematic breakage): core counts, for the newest running build, the days it failed to publish (an
/// input-plan refusal is the provider's inputs, not counted), past half of the attempted days from the 20th
/// published day, and any day with a measurement withheld for an engine fault on every asset; those raise
/// STOP_RULE. Unexplained reference failures and single-asset engine faults are local: counted and listed for
/// the fix pass apart, never alerted.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_stop_rule_stops_for_systematic_breakage_only() {
    let (scratch, mut client, b) = ready("freeze_stop").await;
    client.query_one("SELECT vyreon.ensure_months('2025-10-01', '2025-12-31')", &[]).await.unwrap();
    let days: Vec<String> = client
        .query("SELECT session::text FROM core.session WHERE session BETWEEN '2025-11-03' AND '2025-12-31' ORDER BY 1 LIMIT 23", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(stoprule::tally(&client, PROFILE).await.unwrap().unwrap().line(), None, "nothing to count yet");
    // 19 days published; 12 checked with unexplained failures, 7 without
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, &days[0], &days[18], 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "day-a", 24.0 * HOUR).await.unwrap().unwrap();
    let lease: i64 = client.query_one("SELECT lease_id FROM core.lease WHERE job_id = $1", &[&block.job_id]).await.unwrap().get(0);
    let mut revisions = Vec::new();
    for (i, day) in days[..19].iter().enumerate() {
        let (base, local) = publish_day(&mut client, &block, &b, day).await;
        checked(&client, lease, day, base, local, if i < 12 { 254 } else { 0 }).await;
        revisions.push((base, local));
    }
    work::finish(&mut client, &block.token).await.unwrap();
    // day 20 fails on the engine's stop; day 21 the input plan refuses (the provider's inputs: not counted)
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, &days[19], &days[19], 30).await.unwrap();
    let failing = work::claim(&mut client, Kind::DayBlock, &b, "day-b", HOUR).await.unwrap().unwrap();
    let stop = format!("stop: {} not published: h5-surface: the lattice solver did not converge", days[19]);
    work::fail(&mut client, &failing.token, &format!("day {}", days[19]), &stop, "log").await.unwrap();
    // its last attempt (a retry would only be claimed before the next block here)
    client.execute("UPDATE core.job SET state = 'FAILED' WHERE job_id = $1", &[&failing.job_id]).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, &days[20], &days[20], 30).await.unwrap();
    let refused = work::claim(&mut client, Kind::DayBlock, &b, "day-c", HOUR).await.unwrap().unwrap();
    let plan = format!("stop: {d} not published: {d} refused by the input plan (plan.json)", d = days[20]);
    work::fail(&mut client, &refused.token, &format!("day {}", days[20]), &plan, "log").await.unwrap();
    client.execute("UPDATE core.job SET state = 'FAILED' WHERE job_id = $1", &[&refused.job_id]).await.unwrap();

    let t = stoprule::tally(&client, PROFILE).await.unwrap().unwrap();
    assert_eq!((t.published, t.unpublished, t.attempted(), t.checked, t.flagged, t.systematic, t.faulted), (19, 1, 20, 19, 12, 0, 0));
    assert!(!t.over());
    assert_eq!(
        t.line().unwrap(),
        "Stop rule: so far 1 of 20 days not published after a failure, 0 with a whole measurement faulted; for the fix pass: 0 days with single-asset engine faults, 12 with unexplained reference failures; it judges failures from the 20th published day (19 so far)"
    );
    let now: String = client.query_one("SELECT now()::text", &[]).await.unwrap().get(0);
    watch::check(&mut client, &settings(), &now).await.unwrap();
    async fn open(c: &Client) -> Vec<String> {
        c.query("SELECT summary FROM core.problem WHERE kind = 'STOP_RULE' AND resolved_at IS NULL", &[]).await.unwrap().iter().map(|r| r.get::<_, String>(0)).collect()
    }
    assert!(open(&client).await.is_empty());

    // the 20th published day, 13 of 20 with unexplained reference failures: local, no alert (before C-090, past half)
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, &days[21], &days[21], 30).await.unwrap();
    let more = work::claim(&mut client, Kind::DayBlock, &b, "day-d", HOUR).await.unwrap().unwrap();
    let (base, local) = publish_day(&mut client, &more, &b, &days[21]).await;
    let more_lease: i64 = client.query_one("SELECT lease_id FROM core.lease WHERE job_id = $1", &[&more.job_id]).await.unwrap().get(0);
    checked(&client, more_lease, &days[21], base, local, 31).await;
    revisions.push((base, local));
    // one slot of a day withheld for an engine fault (the engine's real faulted slot of 2025-12-10): local too
    let fault = test_support::ENGINE_FAULT_BODY_XLB.trim_end();
    let fault_json: serde_json::Value = serde_json::from_str(fault).unwrap();
    let (b1, _) = revisions[1];
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             SELECT revision_id, 'SAI-EXQ01-T', 'XLB', represented_date, 'DELIVERED', 'engine-fault', 'engine-fault', $2, $3, $4::text::jsonb, '{}'
             FROM vyreon.day_revision WHERE revision_id = $1",
            &[&b1, &core_contract::sha256_hex(fault.as_bytes()), &fault, &fault_json.to_string()],
        )
        .await
        .unwrap();
    let all: Vec<i64> = revisions.iter().flat_map(|(b, l)| [*b, *l]).collect();
    watch::tally_batches(&mut client, &all, None).await.unwrap();
    let t = stoprule::tally(&client, PROFILE).await.unwrap().unwrap();
    assert_eq!((t.published, t.attempted(), t.flagged, t.faulted, t.systematic, t.over()), (20, 21, 13, 1, 0, false));
    let (_, listed) = stoprule::days(&client, PROFILE, &b).await.unwrap();
    assert_eq!(listed.stop, [(days[19].clone(), "not published: h5-surface: the lattice solver did not converge".to_owned())]);
    assert_eq!(listed.local.len(), 14, "{listed:?}");
    assert!(listed.local.contains(&(days[1].clone(), "1 single-asset engine-fault slots".to_owned())), "{listed:?}");
    assert!(!listed.stop.iter().chain(&listed.local).any(|(d, _)| *d == days[20]), "an input-plan refusal is not counted");
    assert_eq!(
        t.line().unwrap(),
        "Stop rule: 1 of 21 days not published after a failure (5%), 0 with a whole measurement faulted; for the fix pass: 1 days with single-asset engine faults, 13 with unexplained reference failures · the alert is at over half not published, or any whole measurement faulted"
    );
    watch::check(&mut client, &settings(), &now).await.unwrap();
    assert!(open(&client).await.is_empty(), "local value errors raise nothing");

    // one measurement withheld for the engine on every asset of a day (as M0-ANALYTICS, INT-IVSC01): systematic
    let (b2, _) = revisions[2];
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version, method_version,
                                             body_sha256, body_text, body, receipt)
             SELECT revision_id, 'SAI-EXQ01-T', o, represented_date, 'DELIVERED', 'engine-fault', 'engine-fault', $2, $3, $4::text::jsonb, '{}'
             FROM vyreon.day_revision, unnest(ARRAY['XLB', 'XLE', 'XLF']) o WHERE revision_id = $1",
            &[&b2, &core_contract::sha256_hex(fault.as_bytes()), &fault, &fault_json.to_string()],
        )
        .await
        .unwrap();
    client.execute("DELETE FROM core.revision_tally WHERE revision_id = $1", &[&b2]).await.unwrap();
    watch::tally_batches(&mut client, &[b2], None).await.unwrap();
    let t = stoprule::tally(&client, PROFILE).await.unwrap().unwrap();
    assert_eq!((t.systematic, t.faulted, t.over(), t.oldest.as_deref()), (1, 1, true, Some(days[2].as_str())));
    let (_, listed) = stoprule::days(&client, PROFILE, &b).await.unwrap();
    assert!(listed.stop.contains(&(days[2].clone(), "a whole measurement faulted on every asset: SAI-EXQ01-T".to_owned())), "{listed:?}");
    watch::check(&mut client, &settings(), &now).await.unwrap();
    let summary = open(&client).await;
    assert_eq!(summary.len(), 1);
    assert!(
        summary[0].starts_with("Phase-1 stop rule, systematic breakage: 1 days with a measurement withheld for an engine fault on every asset")
            && summary[0].contains("the coordinator's classification"),
        "{summary:?}"
    );
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE post AND message LIKE '%stop rule%' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(alert.contains("Published numbers affected") && alert.contains("`vyreon-core stoprule FREE_REPORT_64`"), "{alert}");

    // failures to publish past half of the attempted days, from the 20th published day: systematic too
    client.execute("DELETE FROM vyreon.measurement WHERE measurement_id = 'SAI-EXQ01-T' AND revision_id = $1", &[&b2]).await.unwrap();
    client.execute("DELETE FROM core.revision_tally WHERE revision_id = $1", &[&b2]).await.unwrap();
    watch::tally_batches(&mut client, &[b2], None).await.unwrap();
    watch::check(&mut client, &settings(), &now).await.unwrap();
    assert!(open(&client).await.is_empty(), "resolved once no day is systematic");
    let t = Tally { published: 20, unpublished: 21, ..Tally::default() };
    assert!(t.over() && t.summary().unwrap().contains("21 of 41 days not published after a failure (51%), over half"), "{:?}", t.summary());
    assert!(!Tally { published: 19, unpublished: 30, ..Tally::default() }.over(), "before the 20th published day");
    assert!(!Tally { published: 20, unpublished: 20, ..Tally::default() }.over(), "half is not past half");
    scratch.drop().await;
}
