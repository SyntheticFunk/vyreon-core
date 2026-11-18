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
use test_support::slice::{BASE, bundle, inventories, parts};
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
    let base_build = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &base_build, &json!({"checklist": "refcheck test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &base_build, "w1", HOUR).await.unwrap().unwrap();
    let report = report_text();
    let full = || refcheck::check(document(&completed("2025-12-10"), &report), "2025-12-10").unwrap();
    let refused = |r: Result<(bool, i64), LeaseError>, says: &str| assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains(says)), "{r:?} should say {says}");

    // a day not yet published, or published and not yet done, keeps no check
    refused(refcheck::put(&mut client, &block.token, &full()).await, "has published");
    let (header, scalars, files) = parts(BASE);
    let path = bundle("refcheck-base", &header, &scalars, &files);
    let published = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    refused(refcheck::put(&mut client, &block.token, &full()).await, "has published");
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
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
    let mut settings = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: Some("2025-12-10".into()) };
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

    // a report that disagrees: one problem, counted; the day without a report is resolved
    let change = watch::check(&mut client, &settings, &after(6)).await.unwrap();
    assert_eq!(ours(change.opened), ["Reference check disagrees with 1 published day: 3958 comparisons failed, the oldest 2025-12-10"]);
    assert_eq!(ours(change.resolved), ["Reference check did not run on 1 published day (1 with no report), the oldest 2025-12-10"]);
    assert!(ours(watch::check(&mut client, &settings, &after(7)).await.unwrap().opened).is_empty(), "alerted once");
    let alerts: Vec<String> = client
        .query("SELECT message FROM core.notice WHERE post AND message LIKE '%Reference check%' ORDER BY notice_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(alerts.len(), 3, "an alert for each, one resolved: {alerts:?}");
    assert!(alerts.iter().any(|a| a.starts_with("🔴 Reference check disagrees") && a.ends_with("To see more: `vyreon-core refcheck list`")), "{alerts:?}");
    assert!(alerts.iter().any(|a| a.starts_with("✅ Resolved: Reference check did not run")), "{alerts:?}");
    assert_eq!(problems(&client).await.len(), 1);
    // the digest counts the day's checks and what they cost
    let line = refcheck::digest_line(&client, &after(6), true).await.unwrap().unwrap();
    assert_eq!(line, "Reference checks in the last 24 hours: 1 (0 agree, 1 disagree, 0 did not run) · typical 40 min, 280 CPU min per day");
    assert!(watch::digest(&client, &settings, &after(6)).await.unwrap().contains(&line));
    settings.refcheck_from = None;
    assert_eq!(refcheck::digest_line(&client, &after(30), false).await.unwrap(), None, "none, and none expected: no line");

    // the day published again (its base superseded, as a republish does): the report judges
    // nothing published any more; the disagreement resolves
    client.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = $1", &[&published.revision_id]).await.unwrap();
    let change = watch::check(&mut client, &settings, &after(8)).await.unwrap();
    assert_eq!(ours(change.resolved), ["Reference check disagrees with 1 published day: 3958 comparisons failed, the oldest 2025-12-10"]);
    assert!(problems(&client).await.is_empty());
    assert_eq!(refcheck::list(&client, None, None, 10).await.unwrap()[0]["current"], false);
    scratch.drop().await;
}
