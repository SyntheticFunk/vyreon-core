//! C-073 (owner, 2026-11-24: the automation debts ride the next release): a published day whose
//! reference check never completed is checked again by core's own REFCHECK job, against the revisions
//! core holds; KEDA's count holds back the work that calls the provider while the request budget made
//! a draw wait; a job that keeps losing its lease without moving on is one alert. On the real slice of
//! 2025-12-10 (its base and day-local revisions as a day block publishes them) and the engine's
//! calendar. Run with `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::refcheck::{self, SCHEMA};
use core_db::watch::{self, Settings};
use core_db::work::{self, Claim, Kind, LeaseError};
use core_db::{budget, build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::{Read, Write};
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
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
    build::admit(&client, &base, &json!({"checklist": "automation test"})).await.unwrap();
    (scratch, client, base)
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

fn document(account: &Value) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(account.to_string().as_bytes()).unwrap();
    gz.finish().unwrap()
}

/// A re-run that timed out, as the engine's worker says it, bound to `revisions`.
fn timed_out(revisions: Value) -> refcheck::Checked {
    let account = json!({
        "schema": SCHEMA, "session": DAY, "outcome": "TIMEOUT",
        "why": "not done in 600 min: its process group was killed; its log ends:\nstarted",
        "wall_seconds": 36003.0, "suite_seconds": 36000.0, "cpu_seconds": null, "cpu_complete": false, "jobs": 6, "deadline_seconds": 36000,
        "revisions": revisions, "counts": null, "summary": null, "report_trimmed": null, "versions": null,
    });
    refcheck::check(document(&account), DAY).unwrap()
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

/// The moment `hours` after the day's base revision was published.
async fn after(client: &Client, base_revision: i64, hours: i32) -> String {
    client
        .query_one("SELECT (published_at + make_interval(hours => $2))::text FROM vyreon.day_revision WHERE revision_id = $1", &[&base_revision, &hours])
        .await
        .unwrap()
        .get(0)
}

/// C-073: a day whose worker stopped before its check reported (hand-over 18 stopped three such
/// workers on 2026-11-23) is not an alert at once: `RERUN_AFTER_HOURS` after publishing, once its
/// worker's lease has ended, core queues one REFCHECK job for the day's revisions (never twice), its
/// worker reads them as the suite's dump from core and reports under its own lease; REFCHECK_NOT_RUN
/// holds only when the re-run also failed to complete. A re-run out of attempts pauses nothing and is
/// not JOB_FAILED (the day's check not run is the one problem).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_check_that_never_reported_is_run_again_by_core_and_alerts_only_if_that_fails() {
    let (scratch, mut client, build) = ready("auto_rerun").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &build, "w1", HOUR).await.unwrap().unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle("auto-base", &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(&build, DAY, base);
    let path = bundle("auto-day-local", &lh, &ls, &lf);
    let local = publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    work::day_done(&mut client, &block.token, DAY).await.unwrap();
    let settings = Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: Some(DAY.into()),
        backup_days: None, spend: None, recompute: None,
        refcheck_rerun_hours: Some(refcheck::RERUN_AFTER_HOURS),
    };
    let reruns = "SELECT count(*) FROM core.job WHERE kind = 'REFCHECK'";

    // its worker still holds its lease (its check runs beside its next day): past the old grace and
    // past the re-run's hours, nothing is queued and nothing alerted
    for hours in [5, 7] {
        let at = after(&client, base, hours).await;
        watch::check(&mut client, &settings, &at).await.unwrap();
        assert_eq!(count(&client, reruns).await, 0);
        assert!(open(&client, "REFCHECK_NOT_RUN").await.is_empty(), "{hours} h");
    }
    // the worker gone (the block finished; a hand-over's withdrawal ends a lease the same way) with no
    // report: not due before RERUN_AFTER_HOURS, queued after, once
    work::finish(&mut client, &block.token).await.unwrap();
    { let at = after(&client, base, 5).await; watch::check(&mut client, &settings, &at).await }.unwrap();
    assert_eq!(count(&client, reruns).await, 0, "not due within the re-run's hours");
    assert!(open(&client, "REFCHECK_NOT_RUN").await.is_empty(), "not due yet is no check not run");
    let at7 = after(&client, base, 7).await;
    watch::check(&mut client, &settings, &at7).await.unwrap();
    watch::check(&mut client, &settings, &at7).await.unwrap();
    let job: (i64, String, String, i32) = {
        let r = client.query_one("SELECT job_id, engine_build, scope::text, max_attempts FROM core.job WHERE kind = 'REFCHECK'", &[]).await.unwrap();
        (r.get(0), r.get(1), r.get(2), r.get(3))
    };
    assert_eq!(count(&client, reruns).await, 1, "queued once");
    assert_eq!((job.1.as_str(), serde_json::from_str::<Value>(&job.2).unwrap(), job.3), (build.as_str(), json!({"base_revision": base, "day_local_revision": local}), refcheck::RERUN_ATTEMPTS));
    assert!(open(&client, "REFCHECK_NOT_RUN").await.is_empty(), "a re-run queued is not a check not run");
    assert_eq!(work::schedule(&mut client, Kind::Refcheck, PROFILE, &build, DAY, DAY, 1).await.unwrap_err(), "reference-check re-runs are queued by core's watch (C-073), not scheduled");

    // a re-run out of attempts: nothing paused, no JOB_FAILED; the day's check stands not run
    client.execute("UPDATE core.job SET max_attempts = 1 WHERE job_id = $1", &[&job.0]).await.unwrap();
    let first = work::claim(&mut client, Kind::Refcheck, &build, "rc-1", HOUR).await.unwrap().unwrap();
    assert_eq!((first.job_id, first.sessions.as_slice(), first.scope.as_deref().map(|s| serde_json::from_str::<Value>(s).unwrap())), (job.0, [DAY.to_owned()].as_slice(), Some(json!({"base_revision": base, "day_local_revision": local}))));
    assert!(!work::fail(&mut client, &first.token, "refcheck", "fill: the provider refused", "log").await.unwrap());
    assert_eq!(count(&client, "SELECT count(*) FROM core.queue_control WHERE paused").await, 0, "a re-run holds nothing back");
    let at8 = after(&client, base, 8).await;
    let change = watch::check(&mut client, &settings, &at8).await.unwrap();
    assert!(open(&client, "JOB_FAILED").await.is_empty(), "{change:?}");
    assert_eq!(open(&client, "REFCHECK_NOT_RUN").await, ["Reference check did not run on 1 published day (1 with no report), the oldest 2025-12-10"]);
    assert_eq!(count(&client, reruns).await, 1, "never queued again");

    // retried (`queue retry-failed`, the cause fixed): pending again while it runs
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    let rerun = work::claim(&mut client, Kind::Refcheck, &build, "rc-2", HOUR).await.unwrap().unwrap();
    { let at = after(&client, base, 9).await; watch::check(&mut client, &settings, &at).await }.unwrap();
    assert!(open(&client, "REFCHECK_NOT_RUN").await.is_empty(), "a re-run under way");

    // a re-run publishes nothing, keeps no capture, archives nothing, marks no day done
    let (h, sc, f) = parts(BASE);
    let path = bundle("auto-rerun-publish", &h, &sc, &f);
    let refused = publish_for_lease(&mut client, &path, &rerun.token).await.unwrap_err().to_string();
    std::fs::remove_file(&path).unwrap();
    assert!(refused.contains("a reference-check re-run publishes nothing"), "{refused}");
    assert!(matches!(work::day_done(&mut client, &rerun.token, DAY).await, Err(LeaseError::Refused(_))));

    // the dump: the revisions as the suite reads a store's dump, from what core holds
    let dumped = refcheck::dump(&mut client, &rerun.token, DAY).await.unwrap();
    assert!(matches!(refcheck::dump(&mut client, &rerun.token, "2025-12-11").await, Err(LeaseError::Refused(ref m)) if m.contains("is of 2025-12-10")));
    assert!(matches!(refcheck::dump(&mut client, "not-a-lease", DAY).await, Err(LeaseError::Gone)));
    let mut tar = tar::Archive::new(dumped.as_slice());
    let (mut store, mut revisions, mut streams) = (String::new(), String::new(), Vec::new());
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        match name.as_str() {
            "store.jsonl" => store = String::from_utf8(bytes).unwrap(),
            "revisions.jsonl" => revisions = String::from_utf8(bytes).unwrap(),
            s if s.starts_with("streams/") && s.ends_with(".jsonl.gz") => streams.push((name.clone(), bytes)),
            other => panic!("unexpected entry {other}"),
        }
    }
    let slots: Vec<Value> = store.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    // (live, 2026-11-25: report 33) each line's keys in the engine's order (`refcheck::write_dump`), its
    // measurement id within the first 400 characters, where the reference suite looks for it
    const ENGINE_ORDER: [&str; 11] = ["profile", "measurement_id", "object_id", "revision_id", "slot_state", "method_version", "science_version", "body_sha256", "body_text", "receipt", "stream"];
    for line in store.lines() {
        let at: Vec<usize> = ENGINE_ORDER.iter().map(|k| line.find(&format!("\"{k}\":")).unwrap_or_else(|| panic!("{k}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]) && at[0] == 1, "keys in the engine's order: {}", &line[..120.min(line.len())]);
        assert!(line[..400.min(line.len())].contains("\"measurement_id\":"), "{}", &line[..120]);
    }
    let rev_order = ["revision_id", "date", "profile", "state", "base_revision", "engine_build", "snapshot_cutoff", "slots"];
    for line in revisions.lines() {
        let at: Vec<usize> = rev_order.iter().map(|k| line.find(&format!("\"{k}\":")).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{line}");
    }
    // the bytes core serves, through the reference suite's own loader: every slot found
    let suite = std::env::var("VYREON_REFERENCE_SUITE").expect("VYREON_REFERENCE_SUITE: the engine's tools/reference, whose loader reads the dump");
    let dir = std::env::temp_dir().join(format!("vyreon-dump-load-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("store.jsonl"), &store).unwrap();
    // every measurement whose slots all carry roles (the bodies the suite compares; the others it never asks for)
    let script = r#"import sys, json
sys.path.insert(0, sys.argv[1])
from refcheck.published import load_store, body_value
slots = [json.loads(l) for l in open(sys.argv[2] + "/store.jsonl", encoding="utf-8")]
rolled = {}
for s in slots:
    m = s["measurement_id"]
    rolled[m] = rolled.get(m, True) and "roles" in body_value(json.loads(s["body_text"]))
want = {m for m, ok in rolled.items() if ok}
print(len(load_store(sys.argv[2], want)), sum(s["measurement_id"] in want for s in slots), len(want))"#;
    let out = std::process::Command::new("python3").args(["-I", "-c", script, &suite, dir.to_str().unwrap()]).output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = String::from_utf8_lossy(&out.stdout);
    let n: Vec<usize> = said.split_whitespace().map(|x| x.parse().unwrap()).collect();
    // the slice's role-carrying bodies are over 2,000 characters: with the keys sorted, each line's
    // measurement id came after its body, past the suite's 400, and none of these was found
    assert!(n[2] >= 2, "the slice's role-carrying slots are asked for: {said}");
    assert_eq!(n[0], n[1], "the suite finds every slot of core's dump it asks for: {said}");
    let held = count(&client, &format!("SELECT count(*) FROM vyreon.measurement WHERE revision_id IN ({base}, {local})")).await;
    assert_eq!(slots.len() as i64, held);
    for s in &slots {
        assert_eq!(core_contract::sha256_hex(s["body_text"].as_str().unwrap().as_bytes()), s["body_sha256"].as_str().unwrap(), "{}", s["measurement_id"]);
        assert!(s["receipt"].is_object() && s["profile"].is_string() && s["slot_state"].is_string());
    }
    // every stream named by a slot is there under its committed events digest, and unpacks to it
    let named: Vec<&Value> = slots.iter().filter(|s| !s["stream"].is_null()).collect();
    assert_eq!(named.len() as i64, count(&client, &format!("SELECT count(*) FROM vyreon.measurement_stream WHERE revision_id IN ({base}, {local})")).await);
    assert_eq!(named.len(), streams.len());
    assert!(!streams.is_empty(), "the slice's M0 slots carry their streams");
    for s in named {
        let file = format!("streams/{}.{}.{}.jsonl.gz", s["measurement_id"].as_str().unwrap(), s["object_id"].as_str().unwrap(), s["stream"]["events_sha256"].as_str().unwrap());
        let (_, gz) = streams.iter().find(|(n, _)| *n == file).unwrap_or_else(|| panic!("{file}"));
        let mut events = Vec::new();
        flate2::read::GzDecoder::new(gz.as_slice()).read_to_end(&mut events).unwrap();
        assert_eq!(core_contract::sha256_hex(&events), s["stream"]["events_sha256"].as_str().unwrap());
        assert_eq!(events.len() as i64, s["stream"]["byte_length"].as_i64().unwrap());
    }
    let revs: Vec<Value> = revisions.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(revs.len(), 2);
    assert_eq!((revs[0]["revision_id"].as_i64(), revs[0]["base_revision"].clone(), revs[0]["date"].as_str(), revs[0]["state"].as_str()), (Some(base), Value::Null, Some(DAY), Some("PUBLISHED")));
    assert_eq!((revs[1]["revision_id"].as_i64(), revs[1]["base_revision"].as_i64(), revs[1]["engine_build"].as_str()), (Some(local), Some(base), Some(build.as_str())));

    // its report binds to the job's revisions (or to none), of its one session; kept, the session is
    // done and the lease finishes
    assert!(matches!(refcheck::put(&mut client, &rerun.token, &timed_out(json!([base]))).await, Err(LeaseError::Refused(ref m)) if m.contains("are not the ones re-run job")));
    assert!(matches!(work::finish(&mut client, &rerun.token).await, Err(LeaseError::Refused(_))), "not done before its report");
    let (new, id) = refcheck::put(&mut client, &rerun.token, &timed_out(json!([base, local]))).await.unwrap();
    assert!(new);
    assert_eq!(refcheck::put(&mut client, &rerun.token, &timed_out(json!([base, local]))).await.unwrap(), (false, id));
    assert_eq!(work::finish(&mut client, &rerun.token).await.unwrap(), job.0);
    // release audit (Codex 8, Gemini 4): said as what it is, never as the ordered passes
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE message = 'All scheduled reference-check re-runs are done (the last: 2025-12-10).'").await, 1);
    // the re-run failed too: now the day's check did not run, said as what the re-run found
    { let at = after(&client, base, 10).await; watch::check(&mut client, &settings, &at).await }.unwrap();
    assert_eq!(open(&client, "REFCHECK_NOT_RUN").await, ["Reference check did not run on 1 published day (1 timed out), the oldest 2025-12-10"]);
    assert_eq!(count(&client, reruns).await, 1);
    // a day block's lease never sends a re-run's report for another job's day, nor a re-run's lease a
    // day block's (the old rule: a day block's own published day)
    scratch.drop().await;
}

/// C-073 (approved by the owner 2026-11-23): KEDA's count (`core.scaler_count`, its own login) holds
/// back the work that calls the provider while a draw waited within `budget::PRESSURE_MINUTES`; the
/// daily session's work and the ordered passes always count; the claim itself is not gated. And the
/// watch does not call a kind held back so stalled.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_scaler_starts_no_provider_work_while_the_budget_made_a_draw_wait() {
    let (scratch, mut client, build) = ready("auto_budget").await;
    let login = format!("scaler_budget_{}", std::process::id());
    client.batch_execute(&format!("DROP ROLE IF EXISTS {login}; CREATE ROLE {login} LOGIN PASSWORD 'scaler-test' IN ROLE vyreon_scaler")).await.unwrap();
    let mut config = scratch.config.clone();
    config.user(&login).password("scaler-test");
    let scaler = test_support::connect(&config).await;
    async fn counted(scaler: &Client, kind: &str, build: &str, daily: Option<bool>) -> i64 {
        scaler.query_one("SELECT core.scaler_count($1, $2, $3)", &[&kind, &build, &daily]).await.unwrap().get(0)
    }
    work::schedule(&mut client, Kind::Tape, PROFILE, &build, "2025-12-10", "2025-12-11", 1).await.unwrap();
    // a daily session's tape (priority 1, C-051)
    client.execute("UPDATE core.job SET priority = 1 WHERE kind = 'TAPE' AND first_session = '2025-12-11'", &[]).await.unwrap();
    assert_eq!((counted(&scaler, "TAPE", &build, Some(false)).await, counted(&scaler, "TAPE", &build, Some(true)).await), (1, 1));
    // the budget saturated: a draw waits, and is recorded
    let tight = budget::Budget { per_minute: 10, lease_seconds: 30.0, max_lease: 600 };
    assert!(matches!(budget::draw(&mut client, tight, "w1", 10).await.unwrap(), budget::Grant::Granted { requests: 10, .. }));
    assert!(matches!(budget::draw(&mut client, tight, "w2", 5).await.unwrap(), budget::Grant::Wait { .. }));
    let now = watch::now(&client).await.unwrap();
    assert!(matches!(budget::pressed(&client, &now).await.unwrap(), Some((_, 1))));
    assert_eq!(counted(&scaler, "TAPE", &build, Some(false)).await, 0, "the backfill's tape waits for the budget");
    assert_eq!(counted(&scaler, "TAPE", &build, Some(true)).await, 1, "the daily session's never does");
    assert_eq!(count(&client, &format!("SELECT core.claimable_count('TAPE', '{build}')")).await, 2, "claimable is unchanged");
    assert!(work::claim(&mut client, Kind::Tape, &build, "t1", HOUR).await.unwrap().is_some(), "a pod already started still claims");
    // quiet for the pressure's minutes: counted again
    client.execute("UPDATE core.budget_wait SET last_at = last_at - make_interval(secs => $1::float8 * 60 + 1)", &[&budget::PRESSURE_MINUTES]).await.unwrap();
    assert_eq!(counted(&scaler, "TAPE", &build, Some(false)).await, 1);
    assert_eq!(budget::pressed(&client, &watch::now(&client).await.unwrap()).await.unwrap(), None);

    // the watch: claimable tape work with no lease is no stall while the budget holds it back
    client.execute("DELETE FROM core.lease", &[]).await.ok();
    client.execute("UPDATE core.job SET state = 'QUEUED', priority = 0 WHERE kind = 'TAPE'", &[]).await.unwrap();
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
    let at: Vec<String> = client
        .query("SELECT (now() + make_interval(mins => m))::text FROM unnest(ARRAY[20, 30, 50]) m", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    let (t20, t30, t50) = (at[0].clone(), at[1].clone(), at[2].clone());
    client
        .execute("INSERT INTO core.budget_wait (minute, waits, last_at) VALUES (date_trunc('minute', $1::text::timestamptz), 3, $1::text::timestamptz - interval '1 minute')", &[&t20])
        .await
        .unwrap();
    watch::check(&mut client, &settings, &t20).await.unwrap();
    assert!(open(&client, "STALLED").await.is_empty(), "held back by the budget, not stalled");
    watch::check(&mut client, &settings, &t30).await.unwrap();
    assert!(open(&client, "STALLED").await.is_empty(), "the stall's clock starts again when the budget is quiet");
    watch::check(&mut client, &settings, &t50).await.unwrap();
    assert_eq!(open(&client, "STALLED").await.len(), 1, "quiet and still no pod: a stall");
    scratch.drop().await;
}

/// C-073 (MILESTONES' automation debt of 2026-11-23: tapes OOM-killed again and again, each a lost
/// lease re-queued quietly): a job that loses `THRASH_LEASES` leases on the session it is still on is
/// JOB_THRASHING, one alert per kind of work naming the job, its session and its last error; it
/// resolves when the job moves on. A day block that lost leases on an earlier day is not thrashing.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_job_that_keeps_losing_its_lease_is_one_alert_with_its_last_error() {
    let (scratch, mut client, build) = ready("auto_thrash").await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &build, "2025-12-10", "2025-12-10", 1).await.unwrap();
    // room for every attempt below before the tape is set aside (JOB_FAILED's)
    client.execute("UPDATE core.job SET max_attempts = 5", &[]).await.unwrap();
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
    async fn lose(client: &mut Client, build: &str) -> Claim {
        let claim = work::claim(client, Kind::Tape, build, "tape-1", HOUR).await.unwrap().unwrap();
        client.execute("UPDATE core.lease SET expires_at = now() - interval '1 second' WHERE job_id = $1 AND ended_at IS NULL", &[&claim.job_id]).await.unwrap();
        work::sweep(client).await.unwrap();
        claim
    }
    let job = lose(&mut client, &build).await.job_id;
    lose(&mut client, &build).await;
    { let at = watch::now(&client).await.unwrap(); watch::check(&mut client, &settings, &at).await }.unwrap();
    assert!(open(&client, "JOB_THRASHING").await.is_empty(), "two lost leases: not yet");
    lose(&mut client, &build).await;
    let now = watch::now(&client).await.unwrap();
    let change = watch::check(&mut client, &settings, &now).await.unwrap();
    let alert = format!(
        "Tape jobs thrashing: job {job} (2025-12-10) lost 3 leases without moving on, the last: its lease expired with no stop recorded (the pod killed: out of memory, its deadline, or its node lost)"
    );
    assert_eq!(change.opened, [alert.clone()]);
    assert_eq!(watch::check(&mut client, &settings, &now).await.unwrap().opened, Vec::<String>::new(), "alerted once");
    // a failure the worker states is the last error said
    let claim = work::claim(&mut client, Kind::Tape, &build, "tape-1", HOUR).await.unwrap().unwrap();
    work::fail(&mut client, &claim.token, "tape 2025-12-10", "replay reduce: the worker's disk is full", "log").await.unwrap();
    // an attempt the provider stopped is PROVIDER_ERRORS's (the real transport error of 2024-07-16's log), not counted here
    let claim = work::claim(&mut client, Kind::Tape, &build, "tape-1", HOUR).await.unwrap().unwrap();
    let provider = r#"tape 2025-12-10: Retryable("exit 3: == failed capture: replay:Transport(\"HttpStatus(502)\")")"#;
    work::fail(&mut client, &claim.token, "tape 2025-12-10", provider, "log").await.unwrap();
    let change = { let at = watch::now(&client).await.unwrap(); watch::check(&mut client, &settings, &at).await }.unwrap();
    assert!(change.opened.is_empty());
    assert_eq!(open(&client, "JOB_THRASHING").await, [format!("Tape jobs thrashing: job {job} (2025-12-10) lost 4 leases without moving on, the last: tape 2025-12-10: replay reduce: the worker's disk is full")]);
    let alerts = count(&client, "SELECT count(*) FROM core.notice WHERE post AND message LIKE '%thrashing%'").await;
    assert_eq!(alerts, 1, "restated, not posted again");
    // the job finishes: resolved
    let claim = work::claim(&mut client, Kind::Tape, &build, "tape-1", HOUR).await.unwrap().unwrap();
    let tx = client.transaction().await.unwrap();
    let held = work::held(&tx, &claim.token).await.unwrap();
    work::session_done(&tx, &held, "2025-12-10").await.unwrap();
    tx.commit().await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();
    let change = { let at = watch::now(&client).await.unwrap(); watch::check(&mut client, &settings, &at).await }.unwrap();
    assert_eq!(change.resolved.len(), 1, "{change:?}");
    assert!(open(&client, "JOB_THRASHING").await.is_empty());
    scratch.drop().await;
}

/// C-073 (live investigation 2026-11-24: an hour's deliberate EXQ01 cutoff wait looked like a stall):
/// a worker that reports its steps and shows no progress for `NO_PROGRESS_MINUTES` is NO_PROGRESS,
/// one alert per job; a declared wait covers its time and alerts once it overstays its grace;
/// progress (a request drawn) resolves it. A worker that never reported a step is not judged.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_worker_making_no_progress_is_one_alert_unless_it_declared_its_wait() {
    let (scratch, mut client, build) = ready("auto_progress").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &build, "day-1", 24.0 * HOUR).await.unwrap().unwrap();
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
    async fn later(client: &Client, minutes: i32) -> String {
        client.query_one("SELECT (now() + make_interval(mins => $1))::text", &[&minutes]).await.unwrap().get(0)
    }
    // no step ever reported: an image before the endpoint is not judged
    let t25 = later(&client, 25).await;
    watch::check(&mut client, &settings, &t25).await.unwrap();
    assert!(open(&client, "NO_PROGRESS").await.is_empty());
    work::report_step(&mut client, &block.token, "day 2025-12-10: plan and surfaces", None, None).await.unwrap();
    let t10 = later(&client, 10).await;
    watch::check(&mut client, &settings, &t10).await.unwrap();
    assert!(open(&client, "NO_PROGRESS").await.is_empty(), "within its minutes");
    let t25 = later(&client, 25).await;
    let change = watch::check(&mut client, &settings, &t25).await.unwrap();
    assert_eq!(change.opened.len(), 1, "{change:?}");
    assert!(change.opened[0].starts_with(&format!("Day block job {} (2025-12-10) on day-1: no progress since ", block.job_id)) && change.opened[0].ends_with("MT, at step \"day 2025-12-10: plan and surfaces\""), "{change:?}");
    assert!(watch::check(&mut client, &settings, &t25).await.unwrap().opened.is_empty(), "alerted once");
    // a deliberate wait declared: covered (resolved), until its end plus the grace
    let until = later(&client, 60).await;
    work::report_step(&mut client, &block.token, "EXQ01 cutoff", Some(&until), Some("the provider's 16:15 ET cutoff")).await.unwrap();
    let t70 = later(&client, 70).await;
    let change = watch::check(&mut client, &settings, &t70).await.unwrap();
    assert_eq!(change.resolved.len(), 1, "{change:?}");
    let t80 = later(&client, 80).await;
    let change = watch::check(&mut client, &settings, &t80).await.unwrap();
    assert_eq!(change.opened.len(), 1, "the wait overstayed its grace: {change:?}");
    assert!(change.opened[0].contains("at step \"EXQ01 cutoff\"; its declared wait (the provider's 16:15 ET cutoff) was to end"), "{change:?}");
    // a request drawn after it: progress, resolved
    client
        .execute("INSERT INTO core.budget_lease (worker, granted, granted_at, valid_until) VALUES ('day-1', 100, $1::text::timestamptz - interval '1 minute', $1::text::timestamptz)", &[&t80])
        .await
        .unwrap();
    let change = watch::check(&mut client, &settings, &t80).await.unwrap();
    assert_eq!(change.resolved.len(), 1, "{change:?}");
    // the same step again is no progress; another step is
    // release audit (Gemini 12): not an instant, refused without being repeated; a wait already over
    // (a second of skew) is kept with its step
    let e = work::report_step(&mut client, &block.token, "x", Some("tomorrow api_key=OmRlYWRiZWVm"), Some("r")).await.unwrap_err();
    assert!(matches!(&e, LeaseError::Refused(m) if !m.contains("OmRlYWRiZWVm")), "{e:?}");
    work::report_step(&mut client, &block.token, "after the cutoff", Some("2020-01-01T00:00:00Z"), Some("the cutoff")).await.unwrap();
    let kept: String = client.query_one("SELECT step FROM core.lease_step", &[]).await.unwrap().get(0);
    assert_eq!(kept, "after the cutoff");
    assert!(matches!(work::report_step(&mut client, &block.token, "x api_key=OmRlYWRiZWVm", None, None).await, Err(LeaseError::Refused(_))));
    scratch.drop().await;
}

/// C-073 hotfix (live, 2026-11-24: seven blocks and a re-run alerted "no progress" in their bulk option
/// download, which makes no request core counts): a download step is a wait of its own, alerted only
/// past `DOWNLOAD_STEP_MAX_MINUTES` and the grace; a report whose progress changed is progress, the same
/// report again is not.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_download_step_is_its_own_wait_and_any_new_report_is_progress() {
    let (scratch, mut client, build) = ready("auto_download").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &build, "day-1", 48.0 * HOUR).await.unwrap().unwrap();
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
    async fn at(client: &Client, minutes: f64) -> String {
        client.query_one("SELECT (now() + make_interval(secs => $1::float8 * 60))::text", &[&minutes]).await.unwrap().get(0)
    }
    // the worker's own step names (engine 1ae946f7): the download, mirrored with its fill's step
    work::report_step(&mut client, &block.token, "download 2025-12-10: fill 2025-12-10: options", None, None).await.unwrap();
    let bound = watch::DOWNLOAD_STEP_MAX_MINUTES + watch::WAIT_GRACE_MINUTES;
    for minutes in [25.0, 180.0, bound - 1.0] {
        let t = at(&client, minutes).await;
        watch::check(&mut client, &settings, &t).await.unwrap();
        assert!(open(&client, "NO_PROGRESS").await.is_empty(), "within the download's bound at {minutes} min");
    }
    let t = at(&client, bound + 1.0).await;
    let change = watch::check(&mut client, &settings, &t).await.unwrap();
    assert_eq!(change.opened.len(), 1, "{change:?}");
    assert!(change.opened[0].contains(&format!("its declared wait (a download step is allowed {} min) was to end", watch::DOWNLOAD_STEP_MAX_MINUTES as i64)), "{change:?}");
    // the same step reporting how far it is: progress
    work::report_step_with(&mut client, &block.token, "download 2025-12-10: fill 2025-12-10: options", None, None, Some("chains 41 of 64")).await.unwrap();
    let since: String = client.query_one("SELECT step_since::text FROM core.lease_step", &[]).await.unwrap().get(0);
    work::report_step_with(&mut client, &block.token, "download 2025-12-10: fill 2025-12-10: options", None, None, Some("chains 41 of 64")).await.unwrap();
    let again: String = client.query_one("SELECT step_since::text FROM core.lease_step", &[]).await.unwrap().get(0);
    assert_eq!(since, again, "the same report again is no progress");
    work::report_step_with(&mut client, &block.token, "download 2025-12-10: fill 2025-12-10: options", None, None, Some("chains 42 of 64")).await.unwrap();
    let moved: String = client.query_one("SELECT step_since::text FROM core.lease_step", &[]).await.unwrap().get(0);
    assert_ne!(since, moved, "a changed progress is progress");
    // a step that is no download keeps the 20 minutes
    work::report_step(&mut client, &block.token, "day 2025-12-10: plan and surfaces", None, None).await.unwrap();
    let t = at(&client, 25.0).await;
    watch::check(&mut client, &settings, &t).await.unwrap();
    assert_eq!(open(&client, "NO_PROGRESS").await.len(), 1);
    assert!(watch::is_download_step("fill 2025-12-10: carry") && watch::is_download_step("download 2025-12-11") && !watch::is_download_step("day 2025-12-10: plan"));
    assert!(matches!(work::report_step_with(&mut client, &block.token, "x", None, None, Some("api_key=OmRlYWRiZWVm")).await, Err(LeaseError::Refused(_))));
    scratch.drop().await;
}

