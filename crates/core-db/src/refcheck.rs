//! The reference check's reports (migration 0025, C-054; engine D-370): what a worker found when
//! it recomputed a day it published with the independent reference suite (QuantLib, TA-Lib), run
//! between publishing the day and deleting its raw inputs.
//!
//! The worker sends one document per lease and session: a gzip whose text is one JSON line, the
//! account (`schema`, `session`, `outcome`, `why`, times, `counts`, `summary`), and after it the
//! suite's report as JSON, or nothing (the check did not complete, or the worker sends the account
//! alone after core refused the report). As with failure evidence (C-049), the worker removes
//! credentials first and core refuses a document in which a credential pattern survives. Core
//! stores the document as sent, with the account's counts as columns.
//!
//! A report is never the day's outcome: the day stays published whatever it says. Its problems
//! (`watch`), judged on each published day's latest check of its current base revision:
//! REFCHECK_DISAGREES while one found failures the rulings table does not explain, and
//! REFCHECK_NOT_RUN while one timed out, crashed or did not run, or (from
//! `VYREON_CORE_REFCHECK_FROM`) a published day has no report at all. A completed check from a
//! suite without rulings (no `failed_unexplained`) or with reference errors is not fully judged:
//! counted in the digest, never an alert (C-054 amended 2026-11-20).
//!
//! C-058: the tape worker's verdicts on the tape caches (`tape::put_verdict`) are the tape owners'
//! half of the check. A FAIL on an asset-session's newest cache is a disagreement to look at
//! (REFCHECK_DISAGREES, beside the days'); a cache with no verdict, or a NOT_RUN one, is not verified:
//! counted in the digest and in `show`, never an alert (every cache stored before the verdicts has
//! none, and its raw tape is gone).
use crate::evidence::unredacted;
use crate::work::{self, LeaseError};
use core_contract::sha256_hex;
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::io::Read;
use tokio_postgres::{Client, GenericClient};

/// The account's schema, the document's first line.
pub const SCHEMA: &str = "VYREON-REFCHECK@1";
/// The most bytes a document may hold as sent: a real day's report is about 42 MB of JSON, 3.5 MB
/// gzipped (the B0 census, 2025-12-16).
pub const MAX_DOCUMENT_BYTES: usize = 32 << 20;
/// The most bytes a document may unpack to.
pub const MAX_UNPACKED_BYTES: u64 = 256 << 20;
pub const OUTCOMES: [&str; 4] = ["COMPLETED", "TIMEOUT", "CRASHED", "NOT_RUN"];
/// A published day with no report is counted not run once its base revision is this old (the
/// worker's deadline, 2 h by default, after the archive phase's 15 min, with room to spare).
pub const GRACE_HOURS: f64 = 4.0;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The report's counts, summed over its measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub checked: i32,
    pub passed: i32,
    pub failed: i32,
    pub skipped: i32,
    pub reference_errors: i32,
    pub failure_classes: i32,
    pub measurements_failing: i32,
    /// C-056: the failures the worker's rulings table does not explain (None: an account from
    /// before the field; every failure counts).
    pub failed_unexplained: Option<i32>,
}

/// A document that passed `check`: only `check` makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub session: String,
    pub outcome: String,
    pub why: Option<String>,
    pub counts: Option<Counts>,
    pub summary: Option<Value>,
    pub wall_seconds: f64,
    pub suite_seconds: Option<f64>,
    pub cpu_seconds: Option<f64>,
    pub cpu_complete: bool,
    pub jobs: i32,
    pub deadline_seconds: i32,
    pub report_trimmed: Option<String>,
    pub report_bytes: u64,
    document: Vec<u8>,
    document_sha256: String,
}

/// Checks a document sent for `session`: a gzip read to its end within the size bounds, UTF-8,
/// with no unredacted credential; its first line the account (of `session`, a known outcome, a
/// reason exactly when it did not complete, counts exactly when it did, times not negative); and
/// after it nothing, or the report as JSON (only beside a completed check).
pub fn check(document: Vec<u8>, session: &str) -> Result<Checked, String> {
    if document.is_empty() {
        return Err("an empty document".into());
    }
    if document.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("a document is at most {MAX_DOCUMENT_BYTES} bytes; this one is {}", document.len()));
    }
    let mut text = Vec::new();
    GzDecoder::new(document.as_slice()).take(MAX_UNPACKED_BYTES + 1).read_to_end(&mut text).map_err(|e| format!("not a gzip: {e}"))?;
    if text.len() as u64 > MAX_UNPACKED_BYTES {
        return Err(format!("a document unpacks to at most {MAX_UNPACKED_BYTES} bytes"));
    }
    let text = String::from_utf8(text).map_err(|_| "a document is UTF-8 text".to_string())?;
    if let Some(found) = unredacted(&text) {
        return Err(format!("{found}: the worker's redaction left a credential; nothing was stored"));
    }
    let (line, report) = text.split_once('\n').unwrap_or((&text, ""));
    let a: Value = serde_json::from_str(line).map_err(|e| format!("the first line is not the account: {e}"))?;
    if a["schema"] != SCHEMA {
        return Err(format!("the first line is not a {SCHEMA} account"));
    }
    if a["session"].as_str() != Some(session) {
        return Err(format!("the account is of {}, not of {session}", a["session"]));
    }
    let outcome = a["outcome"].as_str().filter(|o| OUTCOMES.contains(o)).ok_or_else(|| format!("outcome {} is not one of {}", a["outcome"], OUTCOMES.join(", ")))?.to_owned();
    let completed = outcome == "COMPLETED";
    let why = a["why"].as_str().map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned);
    if completed != why.is_none() {
        return Err(if completed { "a completed check gives no reason".into() } else { format!("a check that is {outcome} says why") });
    }
    let seconds = |k: &str| -> Result<Option<f64>, String> {
        match &a[k] {
            Value::Null => Ok(None),
            v => v.as_f64().filter(|s| s.is_finite() && *s >= 0.0).map(Some).ok_or_else(|| format!("{k} {v} is not a number of seconds")),
        }
    };
    let whole = |v: &Value, k: &str| -> Result<i32, String> { v[k].as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| *n >= 0).ok_or_else(|| format!("{k} {} is not a count", v[k])) };
    let counts = match (&a["counts"], completed) {
        (Value::Null, false) => None,
        (Value::Null, true) => return Err("a completed check states its counts".into()),
        (_, false) => return Err(format!("a check that is {outcome} has no counts")),
        (c, true) => Some(Counts {
            checked: whole(c, "checked")?,
            passed: whole(c, "passed")?,
            failed: whole(c, "failed")?,
            skipped: whole(c, "skipped")?,
            reference_errors: whole(c, "reference_errors")?,
            failure_classes: whole(c, "failure_classes")?,
            measurements_failing: whole(c, "measurements_failing")?,
            failed_unexplained: match &c["failed_unexplained"] {
                Value::Null => None,
                _ => {
                    let (u, f) = (whole(c, "failed_unexplained")?, whole(c, "failed")?);
                    if u > f {
                        return Err(format!("failed_unexplained {u} is more than failed {f}"));
                    }
                    Some(u)
                }
            },
        }),
    };
    let summary = Some(a["summary"].clone()).filter(|s| !s.is_null());
    if summary.as_ref().is_some_and(|s| !s.is_object()) {
        return Err("the summary is an object of measurements".into());
    }
    if !report.is_empty() {
        if !completed {
            return Err(format!("a check that is {outcome} has no report"));
        }
        serde_json::from_str::<&serde_json::value::RawValue>(report).map_err(|e| format!("the report is not JSON: {e}"))?;
    }
    let positive = |k: &str| -> Result<i32, String> { a[k].as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| format!("{k} {} is not a whole number from 1", a[k])) };
    Ok(Checked {
        session: session.to_owned(),
        outcome,
        why,
        counts,
        summary,
        wall_seconds: seconds("wall_seconds")?.ok_or("the account has no wall_seconds")?,
        suite_seconds: seconds("suite_seconds")?,
        cpu_seconds: seconds("cpu_seconds")?,
        cpu_complete: a["cpu_complete"].as_bool().unwrap_or(false),
        jobs: positive("jobs")?,
        deadline_seconds: positive("deadline_seconds")?,
        report_trimmed: a["report_trimmed"].as_str().map(str::to_owned),
        report_bytes: report.len() as u64,
        document_sha256: sha256_hex(&document),
        document,
    })
}

/// Stores the check of `session` under the live lease the token names, once per lease and
/// session: (true, id) when stored, (false, id of the first) when this lease already sent it.
/// Refused unless the lease is a day block's whose job has published the session (its base, of
/// its build): the report is bound to that base revision.
pub async fn put(client: &mut Client, token: &str, c: &Checked) -> Result<(bool, i64), LeaseError> {
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    crate::archive::published_by(&tx, &lease, &c.session).await?;
    let row = tx
        .query_one(
            "SELECT (SELECT revision_id FROM vyreon.day_revision
                     WHERE represented_date = $2::text::date AND profile_id = $3 AND engine_build = $4 AND state = 'PUBLISHED' AND base_revision IS NULL),
                    (SELECT count(*)::int FROM core.lease o WHERE o.job_id = l.job_id AND o.lease_id <= l.lease_id)
             FROM core.lease l WHERE l.lease_id = $1",
            &[&lease.lease_id, &c.session, &lease.profile_id, &lease.engine_build],
        )
        .await
        .map_err(failed)?;
    let (base, attempt): (Option<i64>, i32) = (row.get(0), row.get(1));
    let counts = c.counts;
    let get = |f: fn(&Counts) -> i32| counts.as_ref().map(f);
    let inserted = tx
        .query_opt(
            "INSERT INTO core.refcheck_report (lease_id, job_id, attempt, worker, engine_build, session, base_revision, outcome, why,
                                               checked, passed, failed, skipped, reference_errors, failure_classes, measurements_failing, summary,
                                               wall_seconds, suite_seconds, cpu_seconds, cpu_complete, jobs, deadline_seconds, report_trimmed,
                                               report_bytes, document_sha256, document, failed_unexplained)
             VALUES ($1, $2, $3, $4, $5, $6::text::date, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17::text::jsonb,
                     $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
             ON CONFLICT (lease_id, session) DO NOTHING RETURNING refcheck_report_id",
            &[
                &lease.lease_id,
                &lease.job_id,
                &attempt,
                &lease.worker,
                &lease.engine_build,
                &c.session,
                &base,
                &c.outcome,
                &c.why,
                &get(|k| k.checked),
                &get(|k| k.passed),
                &get(|k| k.failed),
                &get(|k| k.skipped),
                &get(|k| k.reference_errors),
                &get(|k| k.failure_classes),
                &get(|k| k.measurements_failing),
                &c.summary.as_ref().map(Value::to_string),
                &c.wall_seconds,
                &c.suite_seconds,
                &c.cpu_seconds,
                &c.cpu_complete,
                &c.jobs,
                &c.deadline_seconds,
                &c.report_trimmed,
                &(c.report_bytes as i64),
                &c.document_sha256,
                &c.document,
                &counts.as_ref().and_then(|k| k.failed_unexplained),
            ],
        )
        .await
        .map_err(failed)?;
    let answer = match inserted {
        Some(r) => (true, r.get(0)),
        None => {
            let first: i64 = tx
                .query_one("SELECT refcheck_report_id FROM core.refcheck_report WHERE lease_id = $1 AND session = $2::text::date", &[&lease.lease_id, &c.session])
                .await
                .map_err(failed)?
                .get(0);
            (false, first)
        }
    };
    tx.commit().await.map_err(failed)?;
    Ok(answer)
}


/// The columns `list` and `show` read, `r` the report.
const COLUMNS: &str = "r.refcheck_report_id, r.session::text, r.outcome, r.why, r.checked, r.passed, r.failed, r.skipped, r.reference_errors,
     r.failure_classes, r.measurements_failing, r.wall_seconds, r.suite_seconds, r.cpu_seconds, r.cpu_complete, r.jobs, r.deadline_seconds,
     r.job_id, r.attempt, r.worker, r.engine_build, r.base_revision, r.report_bytes, r.report_trimmed,
     to_char(r.stored_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
     EXISTS (SELECT FROM vyreon.day_revision d WHERE d.revision_id = r.base_revision AND d.state = 'PUBLISHED'),
     r.failed_unexplained";

fn record(r: &tokio_postgres::Row) -> Value {
    json!({
        "report_id": r.get::<_, i64>(0), "session": r.get::<_, String>(1), "outcome": r.get::<_, String>(2), "why": r.get::<_, Option<String>>(3),
        "checked": r.get::<_, Option<i32>>(4), "passed": r.get::<_, Option<i32>>(5), "failed": r.get::<_, Option<i32>>(6),
        "skipped": r.get::<_, Option<i32>>(7), "reference_errors": r.get::<_, Option<i32>>(8), "failure_classes": r.get::<_, Option<i32>>(9),
        "measurements_failing": r.get::<_, Option<i32>>(10), "wall_seconds": r.get::<_, f64>(11), "suite_seconds": r.get::<_, Option<f64>>(12),
        "cpu_seconds": r.get::<_, Option<f64>>(13), "cpu_complete": r.get::<_, bool>(14), "jobs": r.get::<_, i32>(15),
        "deadline_seconds": r.get::<_, i32>(16), "job_id": r.get::<_, i64>(17), "attempt": r.get::<_, i32>(18), "worker": r.get::<_, String>(19),
        "engine_build": r.get::<_, String>(20), "base_revision": r.get::<_, Option<i64>>(21), "report_bytes": r.get::<_, i64>(22),
        "report_trimmed": r.get::<_, Option<String>>(23), "stored_at": r.get::<_, String>(24), "current": r.get::<_, bool>(25),
        "failed_unexplained": r.get::<_, Option<i32>>(26),
    })
}

/// The reports of sessions `from` to `to` (each optional), newest session first and, within one,
/// newest first: the counts and times, never the document. `current`: the report's base revision
/// is still the day's published one.
pub async fn list(client: &impl GenericClient, from: Option<&str>, to: Option<&str>, limit: i64) -> Result<Vec<Value>, String> {
    let rows = client
        .query(
            &format!(
                "SELECT {COLUMNS} FROM core.refcheck_report r
                 WHERE ($1::text IS NULL OR r.session >= $1::text::date) AND ($2::text IS NULL OR r.session <= $2::text::date)
                 ORDER BY r.session DESC, r.stored_at DESC, r.refcheck_report_id DESC LIMIT $3"
            ),
            &[&from, &to, &limit],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(record).collect())
}

/// One session's latest report (or the report `id`) in full: the record, the per-measurement
/// summary, and the report's text (empty when none was kept). Checked against its stored digest.
pub async fn show(client: &impl GenericClient, session: &str, id: Option<i64>) -> Result<Option<(Value, Value, String)>, String> {
    let row = client
        .query_opt(
            &format!(
                "SELECT {COLUMNS}, r.summary::text, r.document, r.document_sha256 FROM core.refcheck_report r
                 WHERE r.session = $1::text::date AND ($2::bigint IS NULL OR r.refcheck_report_id = $2)
                 ORDER BY r.stored_at DESC, r.refcheck_report_id DESC LIMIT 1"
            ),
            &[&session, &id],
        )
        .await
        .map_err(db)?;
    let Some(r) = row else { return Ok(None) };
    let (document, digest): (Vec<u8>, String) = (r.get(28), r.get(29));
    if sha256_hex(&document) != digest {
        return Err(format!("{session}: the stored report does not hash to its digest"));
    }
    let mut text = String::new();
    GzDecoder::new(document.as_slice()).read_to_string(&mut text).map_err(|e| format!("{session}: the stored report: {e}"))?;
    let report = text.split_once('\n').map_or("", |(_, rest)| rest).to_owned();
    let summary = r.get::<_, Option<String>>(27).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Ok(Some((record(&r), summary, report)))
}

/// The state the problems are judged on: for each session whose published base revision has a
/// report, its latest such report (a report of a revision since superseded judges nothing).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct State {
    /// Days whose latest check completed with a disagreement (C-056: a failure the rulings table
    /// does not explain); their unexplained failed comparisons; the oldest.
    pub disagree: i64,
    pub failed: i64,
    pub disagree_oldest: Option<String>,
    /// Days whose latest check completed with failures but from a suite without rulings (no
    /// `failed_unexplained`): not yet triaged, never counted as unexplained; their failures.
    pub untriaged: i64,
    pub untriaged_failed: i64,
    /// Days whose latest completed check has reference errors (comparisons the reference suite
    /// itself could not make); their count.
    pub reference_error_days: i64,
    pub reference_errors: i64,
    /// The oldest day of either of the two above (not fully judged).
    pub unjudged_days: i64,
    pub unjudged_oldest: Option<String>,
    pub timeout: i64,
    pub crashed: i64,
    pub not_run: i64,
    /// Published days (from `VYREON_CORE_REFCHECK_FROM`, published `GRACE_HOURS` ago or more) with
    /// no report at all.
    pub unreported: i64,
    pub not_run_oldest: Option<String>,
    /// C-058: asset-sessions whose newest tape cache's verdict is FAIL (the reference reader's bins
    /// differ from the engine's cache); the oldest session.
    pub tape_failed: i64,
    pub tape_failed_oldest: Option<String>,
    /// C-059: asset-sessions whose latest sample check recomputed a cache other than the stored one
    /// (while that cache is still the asset-session's newest); the oldest session.
    pub sample_differ: i64,
    pub sample_differ_oldest: Option<String>,
}

/// The reference checks' state at `now` (`from`: the first session every published day of which
/// is expected to have a report; None: only the reports sent count).
pub async fn state(c: &impl GenericClient, profile: &str, from: Option<&str>, now: &str) -> Result<State, String> {
    let r = c
        .query_one(
            "WITH latest AS (
                 SELECT DISTINCT ON (r.session) r.* FROM core.refcheck_report r
                 JOIN vyreon.day_revision d ON d.revision_id = r.base_revision AND d.state = 'PUBLISHED' AND d.profile_id = $1
                 WHERE r.stored_at <= $3::text::timestamptz
                 ORDER BY r.session, r.stored_at DESC, r.refcheck_report_id DESC),
             unreported AS (
                 SELECT d.represented_date AS session FROM vyreon.day_revision d
                 WHERE $2::text IS NOT NULL AND d.represented_date >= $2::text::date AND d.profile_id = $1 AND d.state = 'PUBLISHED'
                   AND d.base_revision IS NULL AND d.published_at <= $3::text::timestamptz - make_interval(secs => $4::float8 * 3600)
                   AND NOT EXISTS (SELECT FROM core.refcheck_report r WHERE r.base_revision = d.revision_id AND r.stored_at <= $3::text::timestamptz)),
             disagree AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND failed_unexplained > 0),
             untriaged AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND failed_unexplained IS NULL AND failed > 0),
             referr AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND reference_errors > 0),
             unjudged AS (SELECT session FROM untriaged UNION SELECT session FROM referr)
             SELECT (SELECT count(*) FROM disagree), (SELECT COALESCE(sum(failed_unexplained), 0)::bigint FROM disagree),
                    (SELECT min(session)::text FROM disagree),
                    (SELECT count(*) FROM latest WHERE outcome = 'TIMEOUT'),
                    (SELECT count(*) FROM latest WHERE outcome = 'CRASHED'),
                    (SELECT count(*) FROM latest WHERE outcome = 'NOT_RUN'),
                    (SELECT count(*) FROM unreported),
                    (SELECT min(s)::text FROM (SELECT session AS s FROM latest WHERE outcome <> 'COMPLETED'
                                               UNION ALL SELECT session FROM unreported) x),
                    (SELECT count(*) FROM untriaged), (SELECT COALESCE(sum(failed), 0)::bigint FROM untriaged),
                    (SELECT count(*) FROM referr), (SELECT COALESCE(sum(reference_errors), 0)::bigint FROM referr),
                    (SELECT count(*) FROM unjudged), (SELECT min(session)::text FROM unjudged)",
            &[&profile, &from, &now, &GRACE_HOURS],
        )
        .await
        .map_err(db)?;
    // C-058: a verdict judged the newest cache of its asset-session (a cache redone under a newer
    // method version, with its own verdict, supersedes it)
    let t = c
        .query_one(
            "SELECT count(*), min(v.session)::text FROM core.tape_verdict v
             JOIN core.tape_cache t USING (session, asset_key, method_version)
             WHERE v.outcome = 'FAIL' AND v.stored_at <= $1::text::timestamptz
               AND NOT EXISTS (SELECT FROM core.tape_cache n WHERE n.session = v.session AND n.asset_key = v.asset_key
                               AND n.stored_at > t.stored_at AND n.stored_at <= $1::text::timestamptz)",
            &[&now],
        )
        .await
        .map_err(db)?;
    // C-059: the latest check of each asset-session, until a newer cache supersedes the one it found
    let d = c
        .query_one(
            "SELECT count(*), min(session)::text FROM (
                 SELECT DISTINCT ON (session, asset_key) session, asset_key, outcome, checked_at FROM core.tape_check
                 WHERE checked_at <= $1::text::timestamptz ORDER BY session, asset_key, checked_at DESC, tape_check_id DESC) k
             WHERE k.outcome = 'DIFFER'
               AND NOT EXISTS (SELECT FROM core.tape_cache n WHERE n.session = k.session AND n.asset_key = k.asset_key
                               AND n.stored_at > k.checked_at AND n.stored_at <= $1::text::timestamptz)",
            &[&now],
        )
        .await
        .map_err(db)?;
    Ok(State {
        sample_differ: d.get(0),
        sample_differ_oldest: d.get(1),
        tape_failed: t.get(0),
        tape_failed_oldest: t.get(1),
        disagree: r.get(0),
        failed: r.get(1),
        disagree_oldest: r.get(2),
        timeout: r.get(3),
        crashed: r.get(4),
        not_run: r.get(5),
        unreported: r.get(6),
        not_run_oldest: r.get(7),
        untriaged: r.get(8),
        untriaged_failed: r.get(9),
        reference_error_days: r.get(10),
        reference_errors: r.get(11),
        unjudged_days: r.get(12),
        unjudged_oldest: r.get(13),
    })
}

fn days(n: i64) -> String {
    if n == 1 { "1 published day".into() } else { format!("{n} published days") }
}

impl State {
    /// REFCHECK_DISAGREES's summary, when any day disagrees or any tape verdict failed (C-058).
    pub fn disagree_summary(&self) -> Option<String> {
        let mut tapes = Vec::new();
        if self.tape_failed > 0 {
            tapes.push(format!(
                "the tape reader disagrees with the engine's tape cache on {} asset-session{}, the oldest {}",
                self.tape_failed,
                if self.tape_failed == 1 { "" } else { "s" },
                self.tape_failed_oldest.as_deref().unwrap_or("?")
            ));
        }
        // C-059
        if self.sample_differ > 0 {
            tapes.push(format!(
                "the tape sample recomputed a cache other than the stored one on {} asset-session{}, the oldest {} (vyreon-core tape checks)",
                self.sample_differ,
                if self.sample_differ == 1 { "" } else { "s" },
                self.sample_differ_oldest.as_deref().unwrap_or("?")
            ));
        }
        let tape = (!tapes.is_empty()).then(|| tapes.join("; "));
        match (self.disagree > 0, tape) {
            (true, tape) => Some(format!(
                "Reference check disagrees with {}: {} comparisons failed unexplained, the oldest {}{}",
                days(self.disagree),
                self.failed,
                self.disagree_oldest.as_deref().unwrap_or("?"),
                tape.map_or_else(String::new, |t| format!("; {t}"))
            )),
            (false, Some(t)) => Some(format!("Reference check: {t}")),
            (false, None) => None,
        }
    }

    /// REFCHECK_NOT_RUN's summary, when any day's current check did not run (a completed check
    /// with reference errors ran: it is counted by `unjudged_line`, never here).
    pub fn not_run_summary(&self) -> Option<String> {
        let parts: Vec<String> = [(self.timeout, "timed out"), (self.crashed, "crashed"), (self.not_run, "not run"), (self.unreported, "with no report")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, w)| format!("{n} {w}"))
            .collect();
        let n = self.timeout + self.crashed + self.not_run + self.unreported;
        (n > 0).then(|| format!("Reference check did not run on {} ({}), the oldest {}", days(n), parts.join(", "), self.not_run_oldest.as_deref().unwrap_or("?")))
    }

    /// The digest's line on the published days whose current check completed but is not fully
    /// judged: failures from a suite without rulings (not yet triaged against them, never presented
    /// as unexplained), or reference errors. None when there is none; never an alert.
    pub fn unjudged_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.untriaged > 0 {
            parts.push(format!("{} with {} failed comparisons from a suite without rulings, not yet triaged against them", self.untriaged, self.untriaged_failed));
        }
        if self.reference_error_days > 0 {
            parts.push(format!("{} with {} reference errors", self.reference_error_days, self.reference_errors));
        }
        (self.unjudged_days > 0).then(|| {
            format!("Reference checks not fully judged: {} ({}), the oldest {}", days(self.unjudged_days), parts.join("; "), self.unjudged_oldest.as_deref().unwrap_or("?"))
        })
    }
}

/// The digest's line: the checks stored in the 24 hours before `now`, by outcome, and what they
/// cost (median suite wall and processor minutes per day); with none, a line only when every
/// published day is `expected` to have one. Each check is counted once: did not run (not
/// completed), disagree (unexplained failures), not triaged (failures from a suite without
/// rulings, shown only when there is one), else agree; reference errors are stated beside them
/// (the check ran), never as a check that did not run.
pub async fn digest_line(c: &impl GenericClient, now: &str, expected: bool) -> Result<Option<String>, String> {
    let r = c
        .query_one(
            "SELECT count(*), count(*) FILTER (WHERE outcome = 'COMPLETED' AND (failed = 0 OR failed_unexplained = 0)),
                    count(*) FILTER (WHERE outcome = 'COMPLETED' AND failed_unexplained > 0),
                    count(*) FILTER (WHERE outcome = 'COMPLETED' AND failed_unexplained IS NULL AND failed > 0),
                    count(*) FILTER (WHERE outcome <> 'COMPLETED'), COALESCE(sum(reference_errors), 0)::bigint,
                    percentile_cont(0.5) WITHIN GROUP (ORDER BY suite_seconds), percentile_cont(0.5) WITHIN GROUP (ORDER BY cpu_seconds) FILTER (WHERE cpu_complete)
             FROM core.refcheck_report WHERE stored_at > $1::text::timestamptz - interval '24 hours' AND stored_at <= $1::text::timestamptz",
            &[&now],
        )
        .await
        .map_err(db)?;
    let (n, agree, disagree, untriaged, not_run, reference_errors, wall, cpu): (i64, i64, i64, i64, i64, i64, Option<f64>, Option<f64>) =
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6), r.get(7));
    if n == 0 {
        return Ok(expected.then(|| "Reference checks in the last 24 hours: none".into()));
    }
    let cost = match (wall, cpu) {
        (Some(w), Some(c)) => format!(" · typical {:.0} min, {:.0} CPU min per day", w / 60.0, c / 60.0),
        (Some(w), None) => format!(" · typical {:.0} min per day", w / 60.0),
        _ => String::new(),
    };
    let untriaged = if untriaged > 0 { format!(", {untriaged} not triaged against rulings") } else { String::new() };
    let errors = if reference_errors > 0 { format!("; {reference_errors} reference errors") } else { String::new() };
    Ok(Some(format!("Reference checks in the last 24 hours: {n} ({agree} agree, {disagree} disagree{untriaged}, {not_run} did not run{errors}){cost}")))
}

/// C-058: the digest's line on the tape verdicts: the tape caches stored in the 24 hours before
/// `now` (stated absences are no cache), by their verdict; none when no cache was stored.
pub async fn tape_digest_line(c: &impl GenericClient, now: &str) -> Result<Option<String>, String> {
    let r = c
        .query_one(
            "SELECT count(*), count(*) FILTER (WHERE v.outcome = 'PASS'), count(*) FILTER (WHERE v.outcome = 'FAIL'),
                    count(*) FILTER (WHERE v.outcome = 'WITHHELD'), count(*) FILTER (WHERE v.outcome = 'NOT_RUN'),
                    count(*) FILTER (WHERE v.outcome IS NULL)
             FROM core.tape_cache t LEFT JOIN core.tape_verdict v USING (session, asset_key, method_version)
             WHERE t.body_sha256 IS NOT NULL AND t.stored_at > $1::text::timestamptz - interval '24 hours' AND t.stored_at <= $1::text::timestamptz",
            &[&now],
        )
        .await
        .map_err(db)?;
    let (n, pass, fail, withheld, not_run, none): (i64, i64, i64, i64, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
    Ok((n > 0).then(|| {
        format!("Tape caches stored in the last 24 hours: {n} ({pass} verified by the tape reader, {fail} disagree, {withheld} withheld whole, {} not verified)", not_run + none)
    }))
}

/// C-058: a session's tape verdicts under the newest method version stored for it: the count of
/// each outcome, the caches without one, and each verdict that did not pass (asset, outcome, why).
pub async fn tape_show(c: &impl GenericClient, session: &str) -> Result<Value, String> {
    let rows = c
        .query(
            "WITH newest AS (
                 SELECT DISTINCT ON (asset_key) asset_key, method_version, body_sha256 FROM core.tape_cache
                 WHERE session = $1::text::date ORDER BY asset_key, stored_at DESC, method_version DESC)
             SELECT n.asset_key, n.body_sha256 IS NULL, v.outcome, v.why, v.differences FROM newest n
             LEFT JOIN core.tape_verdict v ON v.session = $1::text::date AND v.asset_key = n.asset_key AND v.method_version = n.method_version
             ORDER BY n.asset_key COLLATE \"C\"",
            &[&session],
        )
        .await
        .map_err(db)?;
    let mut counts = serde_json::Map::new();
    let mut not_passed = Vec::new();
    for r in &rows {
        let (asset, absent, outcome, why, differences): (String, bool, Option<String>, Option<String>, Option<i32>) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        let key = if absent { "stated absence".to_owned() } else { outcome.clone().unwrap_or_else(|| "none".into()) };
        let n = counts.get(&key).and_then(Value::as_i64).unwrap_or(0);
        counts.insert(key, json!(n + 1));
        if !absent && outcome.as_deref() != Some("PASS") && outcome.as_deref() != Some("WITHHELD") {
            not_passed.push(json!({"asset": asset, "outcome": outcome, "why": why, "differences": differences}));
        }
    }
    Ok(json!({"session": session, "caches": rows.len(), "counts": counts, "not_passed": not_passed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summaries_count_never_list() {
        let s = State { disagree: 2, failed: 41, disagree_oldest: Some("2025-04-10".into()), timeout: 1, unreported: 3, not_run_oldest: Some("2025-04-11".into()), ..State::default() };
        assert_eq!(s.disagree_summary().unwrap(), "Reference check disagrees with 2 published days: 41 comparisons failed unexplained, the oldest 2025-04-10");
        assert_eq!(s.not_run_summary().unwrap(), "Reference check did not run on 4 published days (1 timed out, 3 with no report), the oldest 2025-04-11");
        assert_eq!((State::default().disagree_summary(), State::default().not_run_summary()), (None, None));
        // C-058: a failed tape verdict beside the days, and alone
        let t = State { tape_failed: 3, tape_failed_oldest: Some("2025-04-14".into()), ..s.clone() };
        assert_eq!(t.disagree_summary().unwrap(), "Reference check disagrees with 2 published days: 41 comparisons failed unexplained, the oldest 2025-04-10; the tape reader disagrees with the engine's tape cache on 3 asset-sessions, the oldest 2025-04-14");
        let t = State { tape_failed: 1, tape_failed_oldest: Some("2025-04-14".into()), ..State::default() };
        assert_eq!(t.disagree_summary().unwrap(), "Reference check: the tape reader disagrees with the engine's tape cache on 1 asset-session, the oldest 2025-04-14");
        assert_eq!(t.not_run_summary(), None, "a verdict is never a check not run");
        // C-059: a sample check whose recomputed cache differs, alone and beside a failed verdict
        let d = State { sample_differ: 2, sample_differ_oldest: Some("2024-04-09".into()), ..State::default() };
        assert_eq!(d.disagree_summary().unwrap(), "Reference check: the tape sample recomputed a cache other than the stored one on 2 asset-sessions, the oldest 2024-04-09 (vyreon-core tape checks)");
        let d = State { tape_failed: 1, tape_failed_oldest: Some("2025-04-14".into()), ..d };
        assert_eq!(
            d.disagree_summary().unwrap(),
            "Reference check: the tape reader disagrees with the engine's tape cache on 1 asset-session, the oldest 2025-04-14; the tape sample recomputed a cache other than the stored one on 2 asset-sessions, the oldest 2024-04-09 (vyreon-core tape checks)"
        );
        assert_eq!(d.not_run_summary(), None);
    }

    /// C-054 amended 2026-11-20, on the cluster's reports as they stood at the 10:00 MT digest of
    /// 2026-11-20 (`refcheck list`): 11-02, 11-03 (its crash superseded) and 11-04 by a suite
    /// without rulings, 1,982, 2,489 and 2,146 failed and 9 reference errors each; 10-28 with 254
    /// unexplained of 2,228. The live digest said "did not run on 1 published day (1 crashed)" and
    /// "1982 comparisons failed"; the state now says what holds.
    #[test]
    fn a_check_without_rulings_or_with_reference_errors_is_not_fully_judged_never_not_run() {
        let s = State {
            disagree: 1,
            failed: 254,
            disagree_oldest: Some("2026-10-28".into()),
            untriaged: 3,
            untriaged_failed: 1982 + 2489 + 2146,
            reference_error_days: 3,
            reference_errors: 27,
            unjudged_days: 3,
            unjudged_oldest: Some("2026-11-02".into()),
            ..State::default()
        };
        assert_eq!(s.disagree_summary().unwrap(), "Reference check disagrees with 1 published day: 254 comparisons failed unexplained, the oldest 2026-10-28");
        assert_eq!(s.not_run_summary(), None, "a completed check with reference errors ran");
        assert_eq!(
            s.unjudged_line().unwrap(),
            "Reference checks not fully judged: 3 published days (3 with 6617 failed comparisons from a suite without rulings, not yet triaged against them; 3 with 27 reference errors), the oldest 2026-11-02"
        );
        // the untriaged days alone open no problem
        let alone = State { disagree: 0, failed: 0, disagree_oldest: None, ..s };
        assert_eq!((alone.disagree_summary(), alone.not_run_summary()), (None, None));
        assert_eq!(State::default().unjudged_line(), None);
    }
}
