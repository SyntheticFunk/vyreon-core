//! The reference check's reports (C-054; engine D-370): a day block's worker sends what the
//! reference suite found on a day it published; core checks the document, keeps it once per lease
//! and session bound to the day's published base revision, and judges the published days on their
//! latest reports: a disagreement is one problem, a check that did not run (or no report at all)
//! another. The report is a real one (`test_support::REFCHECK_REPORT_2025_12_16`, the B0 census);
//! the day is the real slice of 2025-12-10. Run the database test with `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::refcheck::{self, SCHEMA};
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use std::io::{Read, Write};
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
use test_support::{REFCHECK_REPORT_2025_12_16, Scratch};

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

fn report_text() -> String {
    let mut text = String::new();
    GzDecoder::new(REFCHECK_REPORT_2025_12_16).read_to_string(&mut text).unwrap();
    text
}

/// The account a worker writes for a completed check of `session` with the real report: its
/// counts summed over the report's measurements, as the engine's `refcheck::counts` sums them.
fn completed(session: &str) -> Value {
    let report: Value = serde_json::from_str(&report_text()).unwrap();
    let summary = report["summary"].as_object().unwrap();
    let sum = |k: &str| summary.values().map(|m| m[k].as_i64().unwrap_or(0)).sum::<i64>();
    json!({
        "schema": SCHEMA, "session": session, "outcome": "COMPLETED", "why": null,
        "wall_seconds": 2460.5, "suite_seconds": 2400.25, "cpu_seconds": 16800.0, "cpu_complete": true, "jobs": 8, "deadline_seconds": 7200,
        "revisions": [1, 2],
        "counts": {"checked": sum("checked"), "passed": sum("passed"), "failed": sum("failed"), "skipped": sum("skipped"),
                   "reference_errors": sum("reference_errors"), "unit_mismatch": sum("unit_mismatch"),
                   "failure_classes": report["failure_classes"].as_array().unwrap().len(), "measurements": summary.len(),
                   "measurements_failing": summary.values().filter(|m| m["failed"].as_i64().unwrap_or(0) > 0).count()},
        "summary": report["summary"], "report_trimmed": null, "versions": {"QuantLib": "1.43", "TA-Lib": "0.8.2"},
    })
}

/// A check that timed out, as the worker says it (the suite's log's end as the reason).
fn timed_out(session: &str) -> Value {
    json!({
        "schema": SCHEMA, "session": session, "outcome": "TIMEOUT",
        "why": "not done in 120 min: its process group was killed; its log ends:\nstarted",
        "wall_seconds": 7203.0, "suite_seconds": 7200.0, "cpu_seconds": null, "cpu_complete": false, "jobs": 8, "deadline_seconds": 7200,
        "revisions": [1, 2], "counts": null, "summary": null, "report_trimmed": null, "versions": null,
    })
}

fn document(account: &Value, report: &str) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    let text = if report.is_empty() { account.to_string() } else { format!("{account}\n{report}") };
    gz.write_all(text.as_bytes()).unwrap();
    gz.finish().unwrap()
}

#[test]
fn a_document_is_checked_before_anything_is_stored() {
    let report = report_text();
    let c = refcheck::check(document(&completed("2025-12-10"), &report), "2025-12-10").unwrap();
    let k = c.counts.unwrap();
    assert_eq!((k.checked, k.passed, k.failed, k.skipped, k.reference_errors, k.failure_classes), (91066, 87108, 3958, 3751, 0, 384));
    assert_eq!((c.outcome.as_str(), c.why.as_deref(), c.jobs, c.report_bytes), ("COMPLETED", None, 8, report.len() as u64));
    assert_eq!(c.summary.as_ref().and_then(Value::as_object).map(|s| s.len()), Some(108));
    // the account alone (core refused the report, or the check did not complete)
    let t = refcheck::check(document(&timed_out("2025-12-10"), ""), "2025-12-10").unwrap();
    assert_eq!((t.outcome.as_str(), t.counts, t.report_bytes, t.cpu_seconds), ("TIMEOUT", None, 0, None));
    assert!(refcheck::check(document(&completed("2025-12-10"), ""), "2025-12-10").is_ok());
    let refused = |doc: Vec<u8>, says: &str| {
        let e = refcheck::check(doc, "2025-12-10").unwrap_err();
        assert!(e.contains(says), "{e} should say {says}");
    };
    refused(document(&completed("2025-12-11"), &report), "is of \"2025-12-11\"");
    refused(document(&timed_out("2025-12-10"), &report), "has no report");
    let mut a = completed("2025-12-10");
    a["why"] = json!("it agreed");
    refused(document(&a, ""), "gives no reason");
    let mut a = timed_out("2025-12-10");
    a["why"] = json!(" ");
    refused(document(&a, ""), "says why");
    let mut a = timed_out("2025-12-10");
    a["counts"] = completed("2025-12-10")["counts"].clone();
    refused(document(&a, ""), "has no counts");
    let mut a = completed("2025-12-10");
    a["counts"] = Value::Null;
    refused(document(&a, ""), "states its counts");
    let mut a = completed("2025-12-10");
    a["outcome"] = json!("AGREED");
    refused(document(&a, ""), "is not one of");
    let mut a = completed("2025-12-10");
    a["jobs"] = json!(0);
    refused(document(&a, ""), "jobs");
    let mut a = completed("2025-12-10");
    a["wall_seconds"] = json!(-1);
    refused(document(&a, ""), "wall_seconds");
    let mut a = completed("2025-12-10");
    a["schema"] = json!("VYREON-JOB-EVIDENCE@1");
    refused(document(&a, ""), "is not a VYREON-REFCHECK@1 account");
    refused(document(&completed("2025-12-10"), &report[..report.len() / 2]), "the report is not JSON");
    // a credential left in the report: refused, nothing kept
    let leaked = report.replacen("\"dump\": \"", "\"dump\": \"postgresql://vyreon:hunter2pass@db/vyreon ", 1);
    refused(document(&completed("2025-12-10"), &leaked), "nothing was stored");
    let whole = document(&completed("2025-12-10"), &report);
    refused(whole[..whole.len() - 4].to_vec(), "not a gzip");
    refused(Vec::new(), "empty");
    refused(vec![0u8; refcheck::MAX_DOCUMENT_BYTES + 1], "at most");
    // C-056: the failures the rulings table does not explain; an account without them counts every
    // failure (None), one with them is checked against `failed`
    assert_eq!(c.counts.unwrap().failed_unexplained, None);
    let mut a = completed("2025-12-10");
    a["counts"]["failed_unexplained"] = json!(67);
    assert_eq!(refcheck::check(document(&a, &report), "2025-12-10").unwrap().counts.unwrap().failed_unexplained, Some(67));
    let mut a = completed("2025-12-10");
    a["counts"]["failed_unexplained"] = json!(3959);
    refused(document(&a, ""), "is more than failed");
    let mut a = completed("2025-12-10");
    a["counts"]["failed_unexplained"] = json!(-1);
    refused(document(&a, ""), "failed_unexplained");
}

/// The reference check's lines of a check's change (the calendar's end and the others are not ours).
fn ours(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().filter(|l| l.starts_with("Reference check")).collect()
}

async fn problems(client: &tokio_postgres::Client) -> Vec<(String, String)> {
    client
        .query("SELECT kind, summary FROM core.problem WHERE resolved_at IS NULL AND kind LIKE 'REFCHECK%' ORDER BY kind", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_published_days_check_is_kept_once_and_judged_until_the_day_is_published_again() {
    let scratch = Scratch::new("refcheck").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    // C-070: a day block's day is done with its base and its day-local revision (C-031)
    let base_build = build::register(&mut client, &with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    build::admit(&client, &base_build, &json!({"checklist": "refcheck test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &base_build, "w1", HOUR).await.unwrap().unwrap();
    let report = report_text();
    // the account names the revisions it checked (round cc, 5): here the base this block publishes
    let checked_revisions = std::cell::Cell::new(json!([1]));
    let full = || {
        let mut a = completed("2025-12-10");
        a["revisions"] = checked_revisions.take();
        checked_revisions.set(a["revisions"].clone());
        refcheck::check(document(&a, &report), "2025-12-10").unwrap()
    };
    let refused = |r: Result<(bool, i64), LeaseError>, says: &str| assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains(says)), "{r:?} should say {says}");

    // a day not yet published, or published and not yet done, keeps no check
    refused(refcheck::put(&mut client, &block.token, &full()).await, "has published");
    let (header, scalars, files) = parts(BASE);
    let path = bundle("refcheck-base", &header, &scalars, &files);
    let published = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(&base_build, "2025-12-10", published.revision_id);
    let path = bundle("refcheck-day-local", &lh, &ls, &lf);
    let day_local = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    refused(refcheck::put(&mut client, &block.token, &full()).await, "has published");
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    // round cc, 5: revisions that are not this day's base (and a day-local one on it) are refused
    for bad in [json!([published.revision_id + 1]), json!([published.revision_id, published.revision_id]), json!([null, published.revision_id])] {
        checked_revisions.set(bad);
        refused(refcheck::put(&mut client, &block.token, &full()).await, "are not a base revision of 2025-12-10");
    }
    checked_revisions.set(json!([published.revision_id]));
    // moments after the base revision was published
    let mut at = std::collections::BTreeMap::new();
    for hours in [1, 5, 6, 7, 8, 30] {
        let t: String = client
            .query_one("SELECT (published_at + make_interval(hours => $2))::text FROM vyreon.day_revision WHERE revision_id = $1", &[&published.revision_id, &hours])
            .await
            .unwrap()
            .get(0);
        at.insert(hours, t);
    }
    let after = |hours: i32| at[&hours].clone();

    // expected from 2025-12-10: within the grace nothing is wrong; past it, a day with no report
    // is a check that did not run
    let mut settings = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: Some("2025-12-10".into()), backup_days: None, spend: None, recompute: None, refcheck_rerun_hours: None };
    assert!(ours(watch::check(&mut client, &settings, &after(1)).await.unwrap().opened).is_empty());
    let opened = ours(watch::check(&mut client, &settings, &after(5)).await.unwrap().opened);
    assert_eq!(opened, ["Reference check did not run on 1 published day (1 with no report), the oldest 2025-12-10"]);

    // the worker's report: kept once, bound to the published base revision
    let (new, id) = refcheck::put(&mut client, &block.token, &full()).await.unwrap();
    assert!(new);
    assert_eq!(refcheck::put(&mut client, &block.token, &full()).await.unwrap(), (false, id), "the same lease and day again: the first is kept");
    let timeout = refcheck::check(document(&timed_out("2025-12-10"), ""), "2025-12-10").unwrap();
    assert_eq!(refcheck::put(&mut client, &block.token, &timeout).await.unwrap(), (false, id), "one per lease and session, whatever it says");
    assert_eq!(refcheck::put(&mut client, "not-a-lease", &full()).await, Err(LeaseError::Gone));
    let listed = refcheck::list(&client, Some("2025-12-01"), None, 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    let r = &listed[0];
    assert_eq!((r["report_id"].as_i64(), r["outcome"].as_str(), r["failed"].as_i64(), r["base_revision"].as_i64()), (Some(id), Some("COMPLETED"), Some(3958), Some(published.revision_id)));
    assert_eq!((r["job_id"].as_i64(), r["attempt"].as_i64(), r["worker"].as_str(), r["current"].as_bool()), (Some(block.job_id), Some(1), Some("w1"), Some(true)));
    assert_eq!((r["cpu_seconds"].as_f64(), r["jobs"].as_i64(), r["report_bytes"].as_i64()), (Some(16800.0), Some(8), Some(report.len() as i64)));
    let (record, summary, kept) = refcheck::show(&client, "2025-12-10", None).await.unwrap().unwrap();
    assert_eq!((record["report_id"].as_i64(), summary.as_object().map(|s| s.len()), kept == report), (Some(id), Some(108), true));
    assert!(refcheck::show(&client, "2025-12-11", None).await.unwrap().is_none());

    // the B0 census's account carries no `failed_unexplained` (a suite without rulings): its
    // failures are not yet triaged, never a disagreement (C-054 amended 2026-11-20); the day
    // without a report is resolved
    let change = watch::check(&mut client, &settings, &after(6)).await.unwrap();
    assert!(ours(change.opened).is_empty());
    assert_eq!(ours(change.resolved), ["Reference check did not run on 1 published day (1 with no report), the oldest 2025-12-10"]);
    let alerts: Vec<String> = client
        .query("SELECT message FROM core.notice WHERE post AND message LIKE '%Reference check%' ORDER BY notice_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(alerts.len(), 2, "an alert, one resolved: {alerts:?}");
    assert!(alerts[0].starts_with("🔴 Reference check did not run") && alerts[0].ends_with("To do: see which: `vyreon-core refcheck list`"), "{alerts:?}");
    assert!(alerts[1].starts_with("✅ Resolved: Reference check did not run"), "{alerts:?}");
    assert!(problems(&client).await.is_empty());
    // the digest counts the day's checks and what they cost, and says the day is not fully judged
    let line = refcheck::digest_line(&client, &after(6), true).await.unwrap().unwrap();
    assert_eq!(line, "Reference checks in the last 24 hours: 1 (0 agree, 0 disagree, 1 not triaged against rulings, 0 did not run) · typical 40 min, 280 CPU min per day");
    let digest = watch::digest(&mut client, &settings, &after(6)).await.unwrap();
    assert!(digest.contains(&line) && digest.starts_with("🟡 Running; not fully judged: reference checks of 1 published day"), "{digest}");
    assert!(digest.contains("\nReference checks not fully judged: 1 published day (1 with 3958 failed comparisons from a suite without rulings, not yet triaged against them), the oldest 2025-12-10\n"), "{digest}");
    settings.refcheck_from = None;
    assert_eq!(refcheck::digest_line(&client, &after(30), false).await.unwrap(), None, "none, and none expected: no line");

    // the day published again (its base superseded, as a republish does): the report judges
    // nothing published any more
    client.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id IN ($1, $2)", &[&published.revision_id, &day_local.revision_id]).await.unwrap();
    assert_eq!(refcheck::state(&client, PROFILE, None, &after(8)).await.unwrap(), refcheck::State::default());
    assert_eq!(refcheck::list(&client, None, None, 10).await.unwrap()[0]["current"], false);

    // round cc, 5: the same build publishes the day again (a hand publish) while a later lease's
    // check of the earlier revision is under way: its report, arriving after, stays bound to the
    // revision it read, and the new one is a published day with no report
    let republished: i64 = client
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members)
             SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, 'PUBLISHED', published_at + interval '7 hours', listed_members
             FROM vyreon.day_revision WHERE revision_id = $1 RETURNING revision_id",
            &[&published.revision_id],
        )
        .await
        .unwrap()
        .get(0);
    client
        .execute(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members,
                 base_revision, day_local)
             SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, 'PUBLISHED', published_at + interval '7 hours', listed_members,
                 $2, true
             FROM vyreon.day_revision WHERE revision_id = $1",
            &[&day_local.revision_id, &republished],
        )
        .await
        .unwrap();
    work::finish(&mut client, &block.token).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let second = work::claim(&mut client, Kind::DayBlock, &base_build, "w2", HOUR).await.unwrap().unwrap();
    work::day_done(&mut client, &second.token, "2025-12-10").await.unwrap();
    let (_, late) = refcheck::put(&mut client, &second.token, &full()).await.unwrap();
    let listed = refcheck::list(&client, None, None, 10).await.unwrap();
    let r = listed.iter().find(|r| r["report_id"].as_i64() == Some(late)).unwrap();
    assert_eq!((r["base_revision"].as_i64(), r["current"].as_bool()), (Some(published.revision_id), Some(false)), "bound to what it read, not to {republished}");
    let state = refcheck::state(&client, PROFILE, Some("2025-12-10"), &after(30)).await.unwrap();
    assert_eq!((state.unreported, state.not_run_summary().as_deref()), (1, Some("Reference check did not run on 1 published day (1 with no report), the oldest 2025-12-10")));
    scratch.drop().await;
}

/// The cluster's reports as recorded (`core.refcheck_report`, `vyreon.day_revision`, `core.lease`,
/// read-only 2026-11-20): (report, session, outcome, why, [checked, passed, failed, skipped,
/// reference_errors, failure_classes, measurements_failing], failed_unexplained, wall, suite, cpu,
/// stored_at), and the base revisions they checked with their publication times.
type Recorded = (i64, &'static str, &'static str, Option<&'static str>, Option<[i32; 7]>, Option<i32>, f64, f64, f64, &'static str);
const CRASH_WHY: &str = "exit status 1; its log ends:\nFileNotFoundError: [Errno 2] No such file or directory: '/work/h5-acq/AAPL/consolidated/2026-11-03/capture/trades.raw'";
const REPORTS: [Recorded; 5] = [
    (1, "2026-11-03", "CRASHED", Some(CRASH_WHY), None, None, 3572.679674439, 3557.407796902, 19069.454093, "2026-11-19 14:30:01.71045+00"),
    (2, "2026-11-02", "COMPLETED", None, Some([92767, 90785, 1982, 7155, 9, 167, 26]), None, 1850.791514949, 1845.397573266, 10011.314382, "2026-11-20 00:26:17.313585+00"),
    (3, "2026-11-03", "COMPLETED", None, Some([93087, 90598, 2489, 7152, 9, 160, 27]), None, 1863.402150624, 1858.330308374, 9940.343715, "2026-11-20 04:52:27.989221+00"),
    (4, "2026-11-04", "COMPLETED", None, Some([92791, 90645, 2146, 7235, 9, 174, 28]), None, 1918.827688356, 1913.455197512, 10144.628057, "2026-11-20 09:47:36.931315+00"),
    (5, "2026-10-28", "COMPLETED", None, Some([92530, 90302, 2228, 7507, 0, 396, 31]), Some(254), 1691.54262158, 1686.714364819, 9287.9682, "2026-11-20 14:47:35.342737+00"),
];
/// The 10:00 MT digest of 2026-11-20.
const DIGEST_AT: &str = "2026-11-20 16:00:00+00";

async fn revision(client: &tokio_postgres::Client, build: &str, day: &str, at: &str) -> i64 {
    client
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at, listed_members)
             VALUES ($1::text::date, $2, 'VYREON-RESEARCH-PROFILE-S64-3.0.0', $3, '2026-11-08T13:49:56.940739Z', 'PUBLISHED', $4::text::timestamptz, $5) RETURNING revision_id",
            &[&day, &PROFILE, &build, &at, &test_support::slice::listed_2025_12_10()],
        )
        .await
        .unwrap()
        .get(0)
}

/// Stores recorded report `n` (1-based) as `put` stores one, the account as the document.
async fn report(client: &tokio_postgres::Client, n: usize, lease: i64, base: i64) {
    let (_, session, outcome, why, counts, unexplained, wall, suite, cpu, at) = REPORTS[n - 1];
    let k = |i: usize| counts.map(|c| c[i]);
    let account = json!({
        "schema": SCHEMA, "session": session, "outcome": outcome, "why": why, "wall_seconds": wall, "suite_seconds": suite, "cpu_seconds": cpu,
        "cpu_complete": true, "jobs": 8, "deadline_seconds": 36000,
        "counts": counts.map(|c| json!({"checked": c[0], "passed": c[1], "failed": c[2], "skipped": c[3], "reference_errors": c[4],
                                        "failure_classes": c[5], "measurements_failing": c[6], "failed_unexplained": unexplained})),
    });
    let doc = document(&account, "");
    client
        .execute(
            "INSERT INTO core.refcheck_report (lease_id, job_id, attempt, worker, engine_build, session, base_revision, outcome, why, checked, passed, failed,
                                               skipped, reference_errors, failure_classes, measurements_failing, wall_seconds, suite_seconds, cpu_seconds,
                                               cpu_complete, jobs, deadline_seconds, report_bytes, document_sha256, document, failed_unexplained, stored_at)
             SELECT l.lease_id, l.job_id, 1, l.worker, j.engine_build, $2::text::date, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, true, 8, 36000, 0,
                    $16, $17, $18, $19::text::timestamptz
             FROM core.lease l JOIN core.job j USING (job_id) WHERE l.lease_id = $1",
            &[&lease, &session, &base, &outcome, &why, &k(0), &k(1), &k(2), &k(3), &k(4), &k(5), &k(6), &wall, &suite, &cpu,
              &core_contract::sha256_hex(&doc), &doc, &unexplained, &at],
        )
        .await
        .unwrap();
}

/// The cluster's state: block 2026-11-02 to 11-04 (job 1383: its lease granted 2026-11-19
/// 15:32:27Z, done) and block 2026-10-28 to 10-30 (job 1420: its lease granted 2026-11-20
/// 08:08:16Z, 10-28 published, computing 10-29), their days published and checked at the recorded
/// times; with `checks`, the watch runs as each report arrives (the recorded problem times).
async fn cluster(test: &str, checks: bool) -> (Scratch, tokio_postgres::Client, Settings, Vec<watch::Change>) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "refcheck test"})).await.unwrap();
    client.query_one("SELECT vyreon.ensure_months('2026-10-01', '2026-11-30')", &[]).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2026-11-02", "2026-11-04", 30).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2026-10-28", "2026-10-30", 30).await.unwrap();
    let late = work::claim(&mut client, Kind::DayBlock, &b, "v-block-1028-0-jxmxt", HOUR).await.unwrap().unwrap();
    let early = work::claim(&mut client, Kind::DayBlock, &b, "v-block-1102-0-wqlv9", HOUR).await.unwrap().unwrap();
    assert_eq!((late.sessions[0].as_str(), early.sessions[0].as_str()), ("2026-10-28", "2026-11-02"));
    let lease = async |c: &tokio_postgres::Client, job: i64| -> i64 { c.query_one("SELECT lease_id FROM core.lease WHERE job_id = $1", &[&job]).await.unwrap().get(0) };
    let (l_late, l_early) = (lease(&client, late.job_id).await, lease(&client, early.job_id).await);
    client.execute("UPDATE core.lease SET granted_at = '2026-11-19 15:32:27.781976+00' WHERE lease_id = $1", &[&l_early]).await.unwrap();
    client.execute("UPDATE core.lease SET granted_at = '2026-11-20 08:08:16.867582+00', expires_at = '2026-11-21 00:00:00+00' WHERE lease_id = $1", &[&l_late]).await.unwrap();
    let settings = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None, backup_days: None, spend: None, recompute: None, refcheck_rerun_hours: None };
    let mut changes = Vec::new();
    let mut at = async |client: &mut tokio_postgres::Client, now: &str| {
        if checks {
            changes.push(watch::check(client, &settings, now).await.unwrap());
        }
    };
    // 11-03 by an earlier build, its check crashed (sent under the other lease: one per lease and day)
    let r51 = revision(&client, &b, "2026-11-03", "2026-11-19 13:30:15.805052+00").await;
    report(&client, 1, l_late, r51).await;
    at(&mut client, "2026-11-19 14:30:04.325025+00").await;
    let r53 = revision(&client, &b, "2026-11-02", "2026-11-19 23:55:19.59603+00").await;
    report(&client, 2, l_early, r53).await;
    at(&mut client, "2026-11-20 00:27:04.674337+00").await;
    // 11-03 published again: the crashed check's revision superseded
    client.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = $1", &[&r51]).await.unwrap();
    let r55 = revision(&client, &b, "2026-11-03", "2026-11-20 04:21:17.88756+00").await;
    report(&client, 3, l_early, r55).await;
    at(&mut client, "2026-11-20 04:53:00+00").await;
    let r57 = revision(&client, &b, "2026-11-04", "2026-11-20 09:15:31.050349+00").await;
    report(&client, 4, l_early, r57).await;
    client.execute("UPDATE core.lease SET ended_at = '2026-11-20 09:48:39.564834+00', outcome = 'DONE' WHERE lease_id = $1", &[&l_early]).await.unwrap();
    client.execute("UPDATE core.job SET state = 'DONE', next_session = NULL, finished_at = '2026-11-20 09:48:39.564834+00' WHERE job_id = $1", &[&early.job_id]).await.unwrap();
    let r59 = revision(&client, &b, "2026-10-28", "2026-11-20 14:19:16.666791+00").await;
    report(&client, 5, l_late, r59).await;
    client.execute("UPDATE core.job SET next_session = '2026-10-29' WHERE job_id = $1", &[&late.job_id]).await.unwrap();
    at(&mut client, "2026-11-20 14:48:00+00").await;
    (scratch, client, settings, changes)
}

const CRASHED: &str = "Reference check did not run on 1 published day (1 crashed), the oldest 2026-11-03";
const DISAGREES: &str = "Reference check disagrees with 1 published day: 254 comparisons failed unexplained, the oldest 2026-10-28";

/// C-054 amended 2026-11-20, on the cluster's recorded sequence: the crash is a check not run
/// until the day is published again and checked; the older suite's failures (no rulings) and its
/// reference errors are not fully judged, never an alert; 10-28's 254 unexplained failures are the
/// one disagreement. The digest of 10:00 MT counts each report once.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_clusters_checks_are_judged_on_each_days_current_check() {
    let (scratch, mut client, settings, changes) = cluster("refcheck_cluster", true).await;
    let said = |c: &watch::Change| (ours(c.opened.clone()), ours(c.resolved.clone()));
    assert_eq!(said(&changes[0]), (vec![CRASHED.to_string()], vec![]), "as the cluster opened it");
    assert_eq!(said(&changes[1]), (vec![], vec![]), "the cluster opened \"1982 comparisons failed\": failures without rulings are not a disagreement");
    assert_eq!(said(&changes[2]), (vec![], vec![CRASHED.to_string()]), "the crashed check's revision was superseded and the day checked again");
    assert_eq!(said(&changes[3]), (vec![DISAGREES.to_string()], vec![]));
    let digest = watch::digest(&mut client, &settings, DIGEST_AT).await.unwrap();
    let lines: Vec<&str> = digest.lines().collect();
    assert_eq!(lines[0], format!("🔴 Needs you: {DISAGREES} · Fri Nov 20"));
    assert!(lines.contains(&"Reference checks in the last 24 hours: 4 (0 agree, 1 disagree, 3 not triaged against rulings, 0 did not run; 27 reference errors) · typical 31 min, 166 CPU min per day"), "{digest}");
    assert!(lines.contains(&"Reference checks not fully judged: 3 published days (3 with 6617 failed comparisons from a suite without rulings, not yet triaged against them; 3 with 27 reference errors), the oldest 2026-11-02"), "{digest}");
    // one build here for both blocks (the cluster's were two, so it read "1 of 3")
    assert!(lines.contains(&"Backfill progress: 4 of 6 sessions done · about 6.2 h a day (a block's first day, with its download) · expected to finish about Fri 20:41 MT"), "{digest}");
    assert!(digest.ends_with(&format!("Open problems (1):\n• {DISAGREES} · open 1 h · `vyreon-core refcheck list`")), "{digest}");
    // the older suite's days alone: yellow, not red
    client.execute("UPDATE core.refcheck_report SET failed_unexplained = 0 WHERE session = '2026-10-28'", &[]).await.unwrap();
    let change = watch::check(&mut client, &settings, DIGEST_AT).await.unwrap();
    assert_eq!(ours(change.resolved), [DISAGREES]);
    let digest = watch::digest(&mut client, &settings, DIGEST_AT).await.unwrap();
    assert!(digest.starts_with("🟡 Running; not fully judged: reference checks of 3 published days · Fri Nov 20\n"), "{digest}");
    // published again: the superseded days' checks judge nothing
    client.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE represented_date IN ('2026-11-02', '2026-11-03', '2026-11-04')", &[]).await.unwrap();
    assert_eq!(refcheck::state(&client, PROFILE, None, DIGEST_AT).await.unwrap().unjudged_line(), None);
    scratch.drop().await;
}

/// C-050 amended 2026-11-20, the deploy itself: the cluster's two problems as the earlier core
/// stored them; the first check of this core resolves the one that no longer holds (one
/// "Resolved", with what it said) and restates the other with what holds, posting nothing.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_open_problem_says_what_holds_now_and_resolves_when_it_no_longer_holds() {
    let (scratch, mut client, settings, _) = cluster("refcheck_restated", false).await;
    let old_disagrees = "Reference check disagrees with 1 published day: 1982 comparisons failed, the oldest 2026-11-02";
    for (kind, at, summary) in [("REFCHECK_NOT_RUN", "2026-11-19 14:30:04.325025+00", CRASHED), ("REFCHECK_DISAGREES", "2026-11-20 00:27:04.674337+00", old_disagrees)] {
        client
            .execute(
                "INSERT INTO core.problem (kind, key, opened_at, summary, command) VALUES ($1, 'refcheck', $2::text::timestamptz, $3, 'vyreon-core refcheck list')",
                &[&kind, &at, &summary],
            )
            .await
            .unwrap();
    }
    let before = watch::digest(&mut client, &settings, DIGEST_AT).await.unwrap();
    assert!(before.starts_with(&format!("🔴 Needs you: {CRASHED} (and 1 more problem) · Fri Nov 20\n")), "{before}");
    let change = watch::check(&mut client, &settings, DIGEST_AT).await.unwrap();
    assert_eq!((ours(change.opened), ours(change.resolved), ours(change.restated)), (vec![], vec![CRASHED.to_string()], vec![DISAGREES.to_string()]));
    let posts: Vec<String> = client.query("SELECT message FROM core.notice WHERE post ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(posts, [watch::posted_resolved(CRASHED, 25 * 3600)]);
    let after = watch::digest(&mut client, &settings, DIGEST_AT).await.unwrap();
    assert!(after.starts_with(&format!("🔴 Needs you: {DISAGREES} · Fri Nov 20\n")), "{after}");
    assert!(after.ends_with(&format!("Open problems (1):\n• {DISAGREES} · open 15 h · `vyreon-core refcheck list`")), "{after}");
    assert!(ours(watch::check(&mut client, &settings, DIGEST_AT).await.unwrap().restated).is_empty(), "restated once");
    scratch.drop().await;
}
