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
    /// The revisions the worker checked, as its account states them (`revisions`: the base, then
    /// the day-local revision); None where not stated (round cc, 5).
    pub base_revision: Option<i64>,
    pub day_local_revision: Option<i64>,
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
    // round cc, 5: the revisions checked, base first (engine `refcheck::Checked::revisions`)
    let (base_revision, day_local_revision) = match &a["revisions"] {
        Value::Null => (None, None),
        Value::Array(ids) if ids.len() <= 2 => {
            let id = |i: usize| -> Result<Option<i64>, String> {
                match ids.get(i) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => v.as_i64().filter(|n| *n > 0).map(Some).ok_or_else(|| format!("revisions[{i}] {v} is not a revision id")),
                }
            };
            (id(0)?, id(1)?)
        }
        v => return Err(format!("revisions {v} is not the base and day-local revision ids")),
    };
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
        base_revision,
        day_local_revision,
        document_sha256: sha256_hex(&document),
        document,
    })
}

/// Stores the check of `session` under the live lease the token names, once per lease and
/// session: (true, id) when stored, (false, id of the first) when this lease already sent it.
/// Refused unless the lease is a day block's whose job has published the session (its base, of
/// its build). The report is bound to the revisions its account says it checked (round cc, 5):
/// the base must be a base revision of this day, profile and build, the day-local one a day-local
/// revision on that base, published or since superseded (a day published again during the check
/// leaves the report bound to what it read, not current); refused otherwise. An account that
/// states no base is kept bound to nothing (it judges no day).
pub async fn put(client: &mut Client, token: &str, c: &Checked) -> Result<(bool, i64), LeaseError> {
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    // C-073: a re-run checks the revisions its job names, of whichever build published them
    let rerun = lease.kind == "REFCHECK";
    if rerun {
        rerun_of(&tx, &lease, c).await?;
    } else {
        crate::archive::published_by(&tx, &lease, &c.session).await?;
    }
    let row = tx
        .query_one(
            "SELECT (SELECT count(*)::int FROM core.lease o WHERE o.job_id = l.job_id AND o.lease_id <= l.lease_id),
                    $5::bigint IS NULL OR EXISTS (SELECT FROM vyreon.day_revision d WHERE d.revision_id = $5 AND d.represented_date = $2::text::date
                                                    AND d.profile_id = $3 AND ($7 OR d.engine_build = $4) AND d.base_revision IS NULL
                                                    AND d.state IN ('PUBLISHED', 'SUPERSEDED')),
                    $6::bigint IS NULL OR EXISTS (SELECT FROM vyreon.day_revision d WHERE d.revision_id = $6 AND d.represented_date = $2::text::date
                                                    AND d.profile_id = $3 AND d.base_revision = $5 AND d.day_local
                                                    AND d.state IN ('PUBLISHED', 'SUPERSEDED'))
             FROM core.lease l WHERE l.lease_id = $1",
            &[&lease.lease_id, &c.session, &lease.profile_id, &lease.engine_build, &c.base_revision, &c.day_local_revision, &rerun],
        )
        .await
        .map_err(failed)?;
    let (attempt, base_ok, local_ok): (i32, bool, bool) = (row.get(0), row.get(1), row.get(2));
    // one per lease and session, whatever the next says: the first is answered before anything else
    if let Some(first) = tx
        .query_opt("SELECT refcheck_report_id FROM core.refcheck_report WHERE lease_id = $1 AND session = $2::text::date", &[&lease.lease_id, &c.session])
        .await
        .map_err(failed)?
    {
        if rerun {
            work::session_done(&tx, &lease, &c.session).await?;
        }
        tx.commit().await.map_err(failed)?;
        return Ok((false, first.get(0)));
    }
    if !base_ok || !local_ok || (c.base_revision.is_none() && c.day_local_revision.is_some()) {
        return Err(LeaseError::Refused(format!(
            "the account's revisions {:?} are not a base revision of {} for build {} and a day-local revision on it",
            [c.base_revision, c.day_local_revision],
            c.session,
            lease.engine_build
        )));
    }
    let base = c.base_revision;
    let counts = c.counts;
    let get = |f: fn(&Counts) -> i32| counts.as_ref().map(f);
    let inserted = tx
        .query_opt(
            "INSERT INTO core.refcheck_report (lease_id, job_id, attempt, worker, engine_build, session, base_revision, outcome, why,
                                               checked, passed, failed, skipped, reference_errors, failure_classes, measurements_failing, summary,
                                               wall_seconds, suite_seconds, cpu_seconds, cpu_complete, jobs, deadline_seconds, report_trimmed,
                                               report_bytes, document_sha256, document, failed_unexplained, day_local_revision)
             VALUES ($1, $2, $3, $4, $5, $6::text::date, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17::text::jsonb,
                     $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29)
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
                &c.day_local_revision,
            ],
        )
        .await
        .map_err(failed)?;
    // C-073: a re-run's one session is done once its report is kept; the worker then finishes
    if rerun {
        work::session_done(&tx, &lease, &c.session).await?;
    }
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


/// C-073: the revisions a re-run's job names (`scope`: `base_revision`, `day_local_revision`).
fn scoped(scope: &Value) -> (Option<i64>, Option<i64>) {
    (scope["base_revision"].as_i64(), scope["day_local_revision"].as_i64())
}

/// C-073: a re-run's report is of its job's one session and binds to the revisions the job names,
/// or to none (an account that states none judges nothing, and the day then stands not run).
async fn rerun_of(tx: &impl GenericClient, lease: &work::Held, c: &Checked) -> Result<(), LeaseError> {
    if c.session != lease.first_session {
        return Err(LeaseError::Refused(format!("reference-check re-run job {} is of {}, not of {}", lease.job_id, lease.first_session, c.session)));
    }
    let scope: Value = tx
        .query_one("SELECT COALESCE(scope, '{}'::jsonb)::text FROM core.job WHERE job_id = $1", &[&lease.job_id])
        .await
        .map_err(|e| LeaseError::Failed(db(e)))
        .map(|r| serde_json::from_str(r.get::<_, &str>(0)).unwrap_or(Value::Null))?;
    let named = scoped(&scope);
    if c.base_revision.is_some() && (c.base_revision, c.day_local_revision) != named {
        return Err(LeaseError::Refused(format!(
            "the account's revisions {:?} are not the ones re-run job {} checks ({:?})",
            [c.base_revision, c.day_local_revision],
            lease.job_id,
            [named.0, named.1]
        )));
    }
    Ok(())
}

/// C-073: hours after its day was published, and its publishing worker stopped, that a day whose
/// current revisions have no completed reference check gets a re-run (`VYREON_CORE_REFCHECK_RERUN_HOURS`
/// overrides it; `off` queues none). The worker's own check runs beside its next day and is joined
/// when that day is done (engine D-405), so a check normally lands within a day's compute of the
/// publish; a worker whose lease is still live is never second-guessed. Six hours: past the 4-hour
/// grace `GRACE_HOURS` (the old alert's) and inside the provider's five sessions of perishable prints.
pub const RERUN_AFTER_HOURS: f64 = 6.0;

/// The days due for a re-run at `now`: from `from` (`VYREON_CORE_REFCHECK_FROM`), the profile's
/// published bases, published `hours` ago or more, whose publishing lease has ended, with no completed
/// report on their current (base, day-local) pair and no re-run of that pair (any state but
/// CANCELLED). `$1` profile, `$2` from, `$3` now, `$4` hours.
/// The day-local revision published on base `d` (none: the day is not complete, C-070).
const LOCAL_OF_D: &str = "(SELECT l.revision_id FROM vyreon.day_revision l
                           WHERE l.base_revision = d.revision_id AND l.day_local AND l.state = 'PUBLISHED' ORDER BY l.revision_id DESC LIMIT 1)";
/// The worker that published base `d` still holds its lease and has not yet reported the day's check
/// (release audit, Gemini 3: one that reported a check that did not complete cannot report another
/// under the same lease, so its day is due however long its block runs on).
const CHECKING_D: &str = "EXISTS (SELECT FROM core.lease_revision lr JOIN core.lease l USING (lease_id)
                                  WHERE lr.revision_id = d.revision_id AND l.ended_at IS NULL
                                    AND NOT EXISTS (SELECT FROM core.refcheck_report r WHERE r.lease_id = l.lease_id AND r.session = d.represented_date))";

/// `DUE`'s columns, of the profile's published bases from `$2`: base, day, build, its day-local revision,
/// whether its worker is still checking it.
fn current_days() -> String {
    format!(
        "SELECT d.revision_id AS base, d.represented_date AS day, d.engine_build, d.published_at, {LOCAL_OF_D} AS local, {CHECKING_D} AS checking
         FROM vyreon.day_revision d
         WHERE d.profile_id = $1 AND d.state = 'PUBLISHED' AND d.base_revision IS NULL AND d.represented_date >= $2::text::date"
    )
}

/// A re-run job `j` of the pair (`base`, `local`).
fn job_of_pair(base: &str, local: &str) -> String {
    format!("j.kind = 'REFCHECK' AND j.scope->>'base_revision' = {base}::text AND COALESCE(j.scope->>'day_local_revision', '') = COALESCE({local}::text, '')")
}

/// A COMPLETED report `r` of exactly the pair (`base`, `local`) (release audit, Codex 17: a base-only
/// report matched every later day-local revision).
fn completed_for_pair(base: &str, local: &str) -> String {
    format!("r.base_revision = {base} AND r.outcome = 'COMPLETED' AND r.day_local_revision IS NOT DISTINCT FROM {local}")
}

/// C-073: queues a re-run (a REFCHECK job of the day's one session) for each day due (`DUE`), for the
/// day's own build if it runs, else the running build admitted last; a day with no running build gets
/// none (it stays not run: REFCHECK_NOT_RUN). Once per (base, day-local) pair: a unique index refuses
/// a second (migration 0055), so two watches never queue one twice. Returns (session, job), oldest
/// first, with one log line when any was queued.
pub async fn queue_reruns(c: &impl GenericClient, profile: &str, from: &str, hours: f64, now: &str) -> Result<Vec<(String, i64)>, String> {
    // release audit (Gemini 2): nothing is queued while the queue is paused (a release's hand-over
    // runs paused: a re-run queued between its preview and its hand-over changed what it moves), nor
    // while a schedule or hand-over holds the scheduler's lock (Codex 14: a re-run queued under a build
    // being handed over was missed by it); tried, never waited for (the watch holds the problems' lock,
    // which a retirement takes after the scheduler's), next minute again
    let paused: bool = c.query_one("SELECT paused FROM core.queue_control", &[]).await.map_err(db)?.get(0);
    let locked: bool = c.query_one("SELECT pg_try_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?.get(0);
    if paused || !locked {
        return Ok(Vec::new());
    }
    // a re-run that can no longer judge the day (its revisions superseded, Codex 3, Gemini 5) or that
    // no worker will take (its build does not run) is cancelled; the day's current pair is then queued
    // again below, for a running build
    let cancelled: Vec<i64> = c
        .query(
            &format!(
                "UPDATE core.job j SET state = 'CANCELLED', finished_at = clock_timestamp()
                 WHERE j.kind = 'REFCHECK' AND j.state IN ('QUEUED', 'FAILED')
                   AND (NOT core.build_runs(j.engine_build)
                        OR NOT EXISTS (SELECT FROM vyreon.day_revision d
                                       WHERE d.revision_id = (j.scope->>'base_revision')::bigint AND d.state = 'PUBLISHED'
                                         AND COALESCE(j.scope->>'day_local_revision', '') = COALESCE({LOCAL_OF_D}::text, '')))
                 RETURNING j.job_id"
            ),
            &[],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if !cancelled.is_empty() {
        let ids: Vec<String> = cancelled.iter().map(i64::to_string).collect();
        crate::notice::log(c, &format!("Reference-check re-runs cancelled (their revisions superseded, or their build does not run): jobs {}.", ids.join(" "))).await?;
    }
    let rows = c
        .query(
            &format!(
                "WITH due AS (SELECT * FROM ({current}) d
                              WHERE d.local IS NOT NULL AND NOT d.checking
                                AND d.published_at <= $3::text::timestamptz - make_interval(secs => $4::float8 * 3600)),
                      todo AS (
                          SELECT due.*, COALESCE(CASE WHEN core.build_runs(due.engine_build) THEN due.engine_build END,
                                                 (SELECT b.engine_build FROM core.engine_build b WHERE core.build_runs(b.engine_build)
                                                  ORDER BY b.admitted_at DESC, b.engine_build LIMIT 1)) AS runner
                          FROM due
                          WHERE NOT EXISTS (SELECT FROM core.refcheck_report r WHERE {completed} AND r.stored_at <= $3::text::timestamptz)
                            AND NOT EXISTS (SELECT FROM core.job j WHERE {pair} AND j.state <> 'CANCELLED'))
                 INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, max_attempts)
                 SELECT 'REFCHECK', $1, runner, day, day, day, jsonb_build_object('base_revision', base, 'day_local_revision', local), {RERUN_ATTEMPTS}
                 FROM todo WHERE runner IS NOT NULL ORDER BY day
                 ON CONFLICT DO NOTHING
                 RETURNING first_session::text, job_id",
                current = current_days(),
                completed = completed_for_pair("due.base", "due.local"),
                pair = job_of_pair("due.base", "due.local"),
            ),
            &[&profile, &from, &now, &hours],
        )
        .await
        .map_err(db)?;
    let mut queued: Vec<(String, i64)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    queued.sort();
    if let (Some(first), Some(last)) = (queued.first(), queued.last()) {
        let days = if queued.len() == 1 { first.0.clone() } else { format!("{} days, {} to {}", queued.len(), first.0, last.0) };
        crate::notice::log(c, &format!("Reference check re-run queued for {days}: no completed check {hours:.0} h after publishing (C-073).")).await?;
    }
    Ok(queued)
}

/// C-073: a re-run's attempts: the lease losses C-035 forgives, then two charged attempts (a second
/// try covers a transient provider or node fault; a check that fails twice is the owner's to see).
pub const RERUN_ATTEMPTS: i32 = 2;

/// C-073: the dump a re-run's worker gives the reference suite (`--dump`), of the revisions its job
/// names, from what core holds published: a tar of `store.jsonl` (one line per slot: `profile`,
/// `measurement_id`, `object_id`, `revision_id`, `slot_state`, `method_version`, `science_version`,
/// `body_sha256`, `body_text`, `receipt`, `stream`), `revisions.jsonl` (the base, then the day-local
/// revision on it, each `PUBLISHED` as the suite reads a day under check) and
/// `streams/<measurement>.<object>.<events sha256>.jsonl.gz` (each stream's events as core keeps them,
/// gzip), exactly the engine's `refcheck::write_dump` layout. Only under a live REFCHECK lease.
pub async fn dump(client: &mut Client, token: &str, session: &str) -> Result<Vec<u8>, LeaseError> {
    let path = std::env::temp_dir().join(format!("vyreon-dump-{}-{}.tar", std::process::id(), sha256_hex(token.as_bytes()).get(..16).unwrap_or("x")));
    let result = dump_to(client, token, session, &path).await;
    let bytes = result.and_then(|()| std::fs::read(&path).map_err(|e| LeaseError::Failed(e.to_string())));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("store"));
    bytes
}

/// `dump`, written to the file `out` as it is read (release audit, Gemini's suspicion 1: a day's
/// streams are hundreds of MB; read row by row and written to disk, never held whole in memory).
/// `store.jsonl` is written beside it first (`out` with the extension `store`) and appended last
/// (a tar entry states its size before its bytes). Refused when a revision the job names is no
/// longer published (Codex 3, Gemini 5: its check would judge nothing; the worker sends NOT_RUN with
/// this reason and finishes).
pub async fn dump_to(client: &mut Client, token: &str, session: &str, out: &std::path::Path) -> Result<(), LeaseError> {
    use futures_util::TryStreamExt;
    use std::io::Write;
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let io = |e: std::io::Error| LeaseError::Failed(format!("the dump's file: {e}"));
    let tx = client.build_transaction().read_only(true).isolation_level(tokio_postgres::IsolationLevel::RepeatableRead).start().await.map_err(failed)?;
    let lease = work::identify(&tx, token).await?;
    if lease.kind != "REFCHECK" {
        return Err(LeaseError::Refused(format!("job {} is not a reference-check re-run", lease.job_id)));
    }
    if session != lease.first_session {
        return Err(LeaseError::Refused(format!("reference-check re-run job {} is of {}, not of {session}", lease.job_id, lease.first_session)));
    }
    let scope: Value = tx
        .query_one("SELECT COALESCE(scope, '{}'::jsonb)::text FROM core.job WHERE job_id = $1", &[&lease.job_id])
        .await
        .map_err(failed)
        .map(|r| serde_json::from_str(r.get::<_, &str>(0)).unwrap_or(Value::Null))?;
    let (base, local) = scoped(&scope);
    let base = base.ok_or_else(|| LeaseError::Failed(format!("re-run job {} names no base revision", lease.job_id)))?;
    let ids: Vec<i64> = std::iter::once(base).chain(local).collect();
    let store_path = out.with_extension("store");
    let mut store = std::io::BufWriter::new(std::fs::File::create(&store_path).map_err(io)?);
    let mut archive = tar::Builder::new(std::io::BufWriter::new(std::fs::File::create(out).map_err(io)?));
    let mut revisions = String::new();
    let statement = tx
        .prepare(
            "SELECT m.measurement_id, m.object_id, m.slot_state, m.method_version, m.science_version, m.body_sha256, m.body_text, m.receipt::text,
                    s.root_sha256, s.events_sha256, s.event_count, s.byte_length, s.events_gzip
             FROM vyreon.measurement m
             LEFT JOIN vyreon.measurement_stream s USING (revision_id, measurement_id, object_id, represented_date)
             WHERE m.revision_id = $1 AND m.represented_date = $2::text::date
             ORDER BY m.measurement_id COLLATE \"C\", m.object_id COLLATE \"C\"",
        )
        .await
        .map_err(failed)?;
    for (i, id) in ids.iter().enumerate() {
        let Some(r) = tx
            .query_opt(
                "SELECT represented_date::text, profile_revision, engine_build,
                        to_char(snapshot_cutoff AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), state
                 FROM vyreon.day_revision WHERE revision_id = $1 AND represented_date = $2::text::date",
                &[id, &lease.first_session],
            )
            .await
            .map_err(failed)?
        else {
            return Err(LeaseError::Refused(format!("revision {id} of {} is not held: nothing to check", lease.first_session)));
        };
        let (date, profile, build, cutoff, state): (String, String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        if state != "PUBLISHED" {
            return Err(LeaseError::Refused(format!("revision {id} of {date} is {state}, no longer published: its check would judge nothing")));
        }
        let rows = tx.query_raw(&statement, [id as &(dyn tokio_postgres::types::ToSql + Sync), &date]).await.map_err(failed)?;
        futures_util::pin_mut!(rows);
        let mut slots = 0usize;
        while let Some(m) = rows.try_next().await.map_err(failed)? {
            let (measurement, object): (String, String) = (m.get(0), m.get(1));
            let receipt: Value = serde_json::from_str(m.get::<_, &str>(7)).map_err(|e| LeaseError::Failed(e.to_string()))?;
            let stream = match m.get::<_, Option<String>>(8) {
                None => Value::Null,
                Some(root) => {
                    let events: String = m.get(9);
                    let gz: &[u8] = m.get(12);
                    let mut head = tar::Header::new_gnu();
                    head.set_size(gz.len() as u64);
                    head.set_mode(0o644);
                    head.set_cksum();
                    archive.append_data(&mut head, format!("streams/{measurement}.{object}.{events}.jsonl.gz"), gz).map_err(io)?;
                    json!({"root_sha256": root, "events_sha256": events, "event_count": m.get::<_, i64>(10), "byte_length": m.get::<_, i64>(11)})
                }
            };
            let line = json!({
                "profile": profile, "measurement_id": measurement, "object_id": object, "revision_id": id,
                "slot_state": m.get::<_, String>(2), "method_version": m.get::<_, String>(3), "science_version": m.get::<_, String>(4),
                // the body's file as the bundle carried it: core keeps it without its final newline
                // (`publish`), the digest is over the file
                "body_sha256": m.get::<_, String>(5), "body_text": format!("{}\n", m.get::<_, &str>(6)), "receipt": receipt, "stream": stream,
            });
            serde_json::to_writer(&mut store, &line).map_err(|e| LeaseError::Failed(e.to_string()))?;
            store.write_all(b"\n").map_err(io)?;
            slots += 1;
        }
        revisions.push_str(
            &json!({
                "revision_id": id, "date": date, "profile": profile, "state": "PUBLISHED", "base_revision": (i > 0).then_some(base),
                "engine_build": build, "snapshot_cutoff": cutoff, "slots": slots,
            })
            .to_string(),
        );
        revisions.push('\n');
    }
    tx.commit().await.map_err(failed)?;
    store.flush().map_err(io)?;
    drop(store);
    archive.append_path_with_name(&store_path, "store.jsonl").map_err(io)?;
    let _ = std::fs::remove_file(&store_path);
    let mut head = tar::Header::new_gnu();
    head.set_size(revisions.len() as u64);
    head.set_mode(0o644);
    head.set_cksum();
    archive.append_data(&mut head, "revisions.jsonl", revisions.as_bytes()).map_err(io)?;
    archive.into_inner().map_err(io)?.flush().map_err(io)
}

/// The columns `list` and `show` read, `r` the report.
const COLUMNS: &str = "r.refcheck_report_id, r.session::text, r.outcome, r.why, r.checked, r.passed, r.failed, r.skipped, r.reference_errors,
     r.failure_classes, r.measurements_failing, r.wall_seconds, r.suite_seconds, r.cpu_seconds, r.cpu_complete, r.jobs, r.deadline_seconds,
     r.job_id, r.attempt, r.worker, r.engine_build, r.base_revision, r.report_bytes, r.report_trimmed,
     to_char(r.stored_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
     EXISTS (SELECT FROM vyreon.day_revision d WHERE d.revision_id = r.base_revision AND d.state = 'PUBLISHED')
       AND (r.day_local_revision IS NULL OR EXISTS (SELECT FROM vyreon.day_revision d WHERE d.revision_id = r.day_local_revision AND d.state = 'PUBLISHED')),
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

/// C-058 amended 2026-11-21: every verdict a cache has, and the current one. A cache's verdicts
/// are the one kept beside it (`core.tape_verdict`, its tape job's, or the first sample that
/// matched it) and those of every later sample check that recomputed it byte for byte (MATCH: the
/// reader read the same bytes). The reader is the reference suite of the build that ran it (the
/// verdict's `engine_build`); a later reader supersedes an earlier one's verdict, ordered by the
/// build's admission (registration when never admitted; an unknown build is the earliest), never by
/// when the verdict came; for one reader, the verdict stored last. `$1`: the moment read (as text).
/// Columns of `current`: session, asset_key, method_version, outcome, why, differences, document
/// (the verdict as its reader sent it; a sample's verdict names the stored cache's bytes, MATCH).
pub const CURRENT_VERDICTS: &str = "verdicts AS (
         SELECT v.session, v.asset_key, v.method_version, v.outcome, v.why, v.differences,
                v.document->>'engine_build' AS reader, v.stored_at AS at, 0 AS source, 0::bigint AS id, v.document
         FROM core.tape_verdict v WHERE v.stored_at <= $1::text::timestamptz
         UNION ALL
         SELECT c.session, c.asset_key, c.method_version, c.verdict_outcome, c.document->'verdict'->>'why',
                CASE WHEN jsonb_typeof(c.document->'verdict'->'differences') = 'array' THEN jsonb_array_length(c.document->'verdict'->'differences') ELSE 0 END,
                c.document->'verdict'->>'engine_build', c.checked_at, 1, c.tape_check_id, c.document->'verdict'
         FROM core.tape_check c WHERE c.outcome = 'MATCH' AND c.verdict_outcome IS NOT NULL AND c.checked_at <= $1::text::timestamptz),
     current AS (
         SELECT DISTINCT ON (x.session, x.asset_key, x.method_version) x.session, x.asset_key, x.method_version, x.outcome, x.why, x.differences, x.document
         FROM verdicts x LEFT JOIN core.engine_build b ON b.engine_build = x.reader
         ORDER BY x.session, x.asset_key, x.method_version, COALESCE(b.admitted_at, b.registered_at) DESC NULLS LAST, x.at DESC, x.source DESC, x.id DESC)";

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
    /// C-058: asset-sessions whose newest tape cache's current verdict (`CURRENT_VERDICTS`: the
    /// latest reader's, kept or from a sample on the same bytes) is FAIL (the reference reader's
    /// bins differ from the engine's cache); the oldest session.
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
    state_with(c, profile, from, None, now).await
}

/// `state`, with re-runs on (C-073, `rerun_hours`: `queue_reruns`' hours): a day whose check is still
/// to be run again is not counted as not run (a re-run queued or leased under a running build, or none
/// yet while the day is not due: published under `rerun_hours` ago, or its publishing worker still
/// holding its lease). REFCHECK_NOT_RUN then holds only a day whose re-run also failed to complete, or
/// that no running build could be given.
pub async fn state_with(c: &impl GenericClient, profile: &str, from: Option<&str>, rerun_hours: Option<f64>, now: &str) -> Result<State, String> {
    let r = c
        .query_one(
            &format!("WITH cur AS (SELECT * FROM ({current}) d WHERE $5::float8 IS NOT NULL AND $2::text IS NOT NULL),
             pending AS (
                 SELECT cur.day AS session FROM cur
                 WHERE EXISTS (SELECT FROM core.job j WHERE {pair} AND j.state IN ('QUEUED', 'LEASED') AND core.build_runs(j.engine_build))
                    OR (NOT EXISTS (SELECT FROM core.job j WHERE {pair} AND j.state <> 'CANCELLED')
                        AND (cur.published_at > $3::text::timestamptz - make_interval(secs => $5::float8 * 3600) OR cur.checking))),
             latest AS (
                 SELECT DISTINCT ON (r.session) r.* FROM core.refcheck_report r
                 JOIN vyreon.day_revision d ON d.revision_id = r.base_revision AND d.state = 'PUBLISHED' AND d.profile_id = $1
                 WHERE r.stored_at <= $3::text::timestamptz
                   AND (r.day_local_revision IS NULL
                        OR EXISTS (SELECT FROM vyreon.day_revision x WHERE x.revision_id = r.day_local_revision AND x.state = 'PUBLISHED'))
                 ORDER BY r.session, r.stored_at DESC, r.refcheck_report_id DESC),
             unreported AS (
                 SELECT d.represented_date AS session FROM vyreon.day_revision d
                 WHERE $2::text IS NOT NULL AND d.represented_date >= $2::text::date AND d.profile_id = $1 AND d.state = 'PUBLISHED'
                   AND d.base_revision IS NULL AND d.published_at <= $3::text::timestamptz - make_interval(secs => $4::float8 * 3600)
                   AND d.represented_date NOT IN (SELECT session FROM pending)
                   AND NOT EXISTS (SELECT FROM core.refcheck_report r WHERE r.base_revision = d.revision_id AND r.stored_at <= $3::text::timestamptz
                                     AND (r.day_local_revision IS NULL
                                          OR EXISTS (SELECT FROM vyreon.day_revision x WHERE x.revision_id = r.day_local_revision AND x.state = 'PUBLISHED')))),
             disagree AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND failed_unexplained > 0),
             untriaged AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND failed_unexplained IS NULL AND failed > 0),
             referr AS (SELECT * FROM latest WHERE outcome = 'COMPLETED' AND reference_errors > 0),
             unjudged AS (SELECT session FROM untriaged UNION SELECT session FROM referr)
             SELECT (SELECT count(*) FROM disagree), (SELECT COALESCE(sum(failed_unexplained), 0)::bigint FROM disagree),
                    (SELECT min(session)::text FROM disagree),
                    (SELECT count(*) FROM latest WHERE outcome = 'TIMEOUT' AND session NOT IN (SELECT session FROM pending)),
                    (SELECT count(*) FROM latest WHERE outcome = 'CRASHED' AND session NOT IN (SELECT session FROM pending)),
                    (SELECT count(*) FROM latest WHERE outcome = 'NOT_RUN' AND session NOT IN (SELECT session FROM pending)),
                    (SELECT count(*) FROM unreported),
                    (SELECT min(s)::text FROM (SELECT session AS s FROM latest WHERE outcome <> 'COMPLETED' AND session NOT IN (SELECT session FROM pending)
                                               UNION ALL SELECT session FROM unreported) x),
                    (SELECT count(*) FROM untriaged), (SELECT COALESCE(sum(failed), 0)::bigint FROM untriaged),
                    (SELECT count(*) FROM referr), (SELECT COALESCE(sum(reference_errors), 0)::bigint FROM referr),
                    (SELECT count(*) FROM unjudged), (SELECT min(session)::text FROM unjudged)",
                current = current_days().replace("$2::text::date", "COALESCE($2::text, '-infinity')::date"),
                pair = job_of_pair("cur.base", "cur.local"),
            ),
            &[&profile, &from, &now, &GRACE_HOURS, &rerun_hours],
        )
        .await
        .map_err(db)?;
    // C-058: the current verdict (the latest reader's, amended 2026-11-21) of the newest cache of
    // its asset-session (a cache redone under a newer method version supersedes it); a FAIL there,
    // whether kept beside the cache or a sample's on the same bytes (C-059, round cc 6)
    let t = c
        .query_one(
            &format!(
                "WITH {CURRENT_VERDICTS}
                 SELECT count(*), min(v.session)::text FROM current v
                 JOIN core.tape_cache t USING (session, asset_key, method_version)
                 WHERE v.outcome = 'FAIL' AND t.stored_at <= $1::text::timestamptz
                   AND NOT EXISTS (SELECT FROM core.tape_cache n WHERE n.session = v.session AND n.asset_key = v.asset_key
                                   AND n.stored_at > t.stored_at AND n.stored_at <= $1::text::timestamptz)"
            ),
            &[&now],
        )
        .await
        .map_err(db)?;
    // C-059 (amended 2026-11-20, round cc 8): the latest comparison (MATCH or DIFFER; a
    // NOT_COMPARED compared nothing and resolves nothing) of each cache, while that cache is its
    // asset-session's newest (no cache stored after it: C-058's supersession, by method version,
    // never by the check's time), that is a DIFFER (a MATCH's verdict is one of the cache's
    // verdicts, judged above)
    let d = c
        .query_one(
            "WITH k AS (
                 SELECT DISTINCT ON (session, asset_key, method_version) session, asset_key, method_version, outcome, verdict_outcome
                 FROM core.tape_check WHERE checked_at <= $1::text::timestamptz AND outcome IN ('MATCH', 'DIFFER')
                 ORDER BY session, asset_key, method_version, checked_at DESC, tape_check_id DESC),
             newest AS (
                 SELECT k.* FROM k JOIN core.tape_cache t USING (session, asset_key, method_version)
                 WHERE t.stored_at <= $1::text::timestamptz
                   AND NOT EXISTS (SELECT FROM core.tape_cache n WHERE n.session = k.session AND n.asset_key = k.asset_key
                                   AND n.stored_at > t.stored_at AND n.stored_at <= $1::text::timestamptz))
             SELECT (SELECT count(*) FROM newest WHERE outcome = 'DIFFER'), (SELECT min(session)::text FROM newest WHERE outcome = 'DIFFER')",
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
            &format!(
                "WITH {CURRENT_VERDICTS},
                 newest AS (
                     SELECT DISTINCT ON (asset_key) asset_key, method_version, body_sha256 FROM core.tape_cache
                     WHERE session = $2::text::date ORDER BY asset_key, stored_at DESC, method_version DESC)
                 SELECT n.asset_key, n.body_sha256 IS NULL, v.outcome, v.why, v.differences FROM newest n
                 LEFT JOIN current v ON v.session = $2::text::date AND v.asset_key = n.asset_key AND v.method_version = n.method_version
                 ORDER BY n.asset_key COLLATE \"C\""
            ),
            &[&"infinity", &session],
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
