//! The owner's channel (C-050): "I want to open it and immediately know if I have to go look at it
//! or it's running ok." Three kinds of post, nothing else:
//!
//!   * an alert when a problem opens, and one "resolved" when it clears; nothing in between. A
//!     problem is a row of `core.problem` (migration 0018), found by `check` from the state core
//!     already holds, so a restart, a missed minute or a second service never alerts twice;
//!   * the digest, every day at 09:00 in the owner's zone, always sent (it is the heartbeat):
//!     a verdict first (🟢 / 🟡 / 🔴), then counts, never ticker lists;
//!   * the weekly check, Saturday at 09:00: the week's sessions and what Sunday's report has.
//!
//! What is normal is not alerted: a session not published before its deadline, an attempt
//! retried that then succeeds, a provider absence the engine states, a build admitted, a block
//! done. Those go to the log (`notice::log`) and into the digest's counts.
//!
//! Times are computed by PostgreSQL in `ZONE` (Mountain Time, with its daylight saving), from a
//! `now` the caller passes, so a check can be run for any moment.
//!
//! Not here (C-065): spend against the ceiling needs DigitalOcean's billing, which core cannot
//! read without a cloud credential, and what core holds (leases) misses the idle nodes that cost
//! most, so the operator keeps it (`deploy/k8s/ops/health.sh`); idle worker nodes are the
//! autoscaler's (KEDA and the pool's minimum of 0).
use crate::daily::{self, Daily};
use crate::{archive, evidence, notice, refcheck};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_postgres::{Client, GenericClient};

/// The owner's zone: Calgary, Mountain Time (UTC−7, UTC−6 in summer).
pub const ZONE: &str = "America/Edmonton";
/// What a post may hold: Discord's limit is 2,000 characters; this leaves a margin.
pub const LIMIT: usize = 1900;
/// The provider is failing when more than this many attempts stop on a provider error (HTTP 429
/// or 5xx) within `PROVIDER_WINDOW_MINUTES`; several at once look like the provider, not a day.
pub const PROVIDER_ERROR_ATTEMPTS: usize = 2;
pub const PROVIDER_WINDOW_MINUTES: f64 = 30.0;
/// An open provider-errors problem clears after this long without a provider error.
pub const PROVIDER_QUIET_HOURS: f64 = 2.0;
/// The database is too full above this share of its volume, and clears below the second.
pub const DATABASE_FULL: f64 = 0.80;
pub const DATABASE_CLEAR: f64 = 0.78;
/// The trading calendar must reach this many days ahead (C-014f).
pub const CALENDAR_DAYS: i32 = 45;
/// The digest's typical coverage is the median over this many earlier published sessions.
pub const TYPICAL_SESSIONS: i64 = 20;
/// A session's publishing deadline: this hour in the owner's zone on the next calendar day
/// (`VYREON_CORE_DEADLINE_HOUR`). C-051: a session is queued at 07:30 New York the next day
/// (05:30 MT, or 06:30 in New York's winter now that Alberta stays on UTC−6); one clean day on
/// the cluster measured 5 h 50 min from an empty disk, so 20:00 MT (13 h 30 min or more later)
/// leaves room for the tape job and two attempts before a late session is alerted.
pub const DEADLINE_HOUR: i32 = 20;
/// The digest names at most this many days set aside or omitted, one line each (C-061).
pub const MISSING_LINES: usize = 10;
/// Sunday's report is at risk this many hours after Friday's deadline (Saturday).
pub const AT_RISK_AFTER_HOURS: i32 = 2;
/// C-065: work claimable and no worker holding a lease for this long is a stall (the owner's 15
/// minutes, 2026-11-22: longer than a worker node takes to start and its pod to claim).
pub const STALL_MINUTES: f64 = 15.0;
/// The same when every claimable job is a daily run's: CronJob `daily` starts its worker at 07:45
/// New York, 15 minutes after the 07:30 queueing (C-051), and its node then has to start.
pub const DAILY_STALL_MINUTES: f64 = 30.0;
/// C-065: claims that found nothing while work of their kind was claimable for their build are a
/// problem at this many within `MISS_WINDOW_MINUTES`, while such work is still claimable at the
/// check (the race of 2026-11-22 made about 12 a minute); it stays open while any recurs.
pub const MISS_CLAIMS: i64 = 3;
pub const MISS_WINDOW_MINUTES: f64 = 10.0;
/// Misses are kept this long (the watch prunes older ones).
pub const MISS_KEEP_DAYS: f64 = 7.0;

/// Taken, for the transaction, by whatever opens or resolves a problem, and by the digest and the
/// weekly check only while they read the open problems again and queue their post (never while
/// they count): a post that says what is open is never queued beside a problem opening at the same
/// moment (round v, 7; round w, 4).
const PROBLEM_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended('core.problem check', 0))";

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// What the checks are told.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The profile whose sessions must publish (`FREE_REPORT_64`).
    pub profile: String,
    /// The first session that has a publishing deadline; None: no deadline is checked (the
    /// backfill's historical sessions never alert).
    pub deadlines_from: Option<String>,
    /// The database volume's size; None: the database's size is reported, not checked.
    pub volume_bytes: Option<i64>,
    /// The deadline's hour in the owner's zone, the next calendar day (0 to 21; `DEADLINE_HOUR`).
    pub deadline_hour: i32,
    /// The daily runs (C-051); None: core queues nothing by itself.
    pub daily: Option<Daily>,
    /// The perishable archive (C-052); None: it is off and not watched.
    pub archive: Option<ArchiveWatch>,
    /// C-054: from this session every published day is expected to have a reference-check report
    /// (`VYREON_CORE_REFCHECK_FROM`); None: only the reports workers send are judged.
    pub refcheck_from: Option<String>,
}

/// What the watch checks of the perishable archive (C-052).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveWatch {
    /// From this session every published day is expected to have each category archived
    /// (`VYREON_CORE_ARCHIVE_FROM`, by default the daily runs' first session); None: only an
    /// archive requested and never kept counts as missing.
    pub expected_from: Option<String>,
    /// A missing archive older than this many days is a problem (`ARCHIVE_MISSING`); None: it is
    /// only counted in the digest.
    pub alert_days: Option<i32>,
}

impl Settings {
    /// Sunday's report's at-risk hour on Saturday.
    pub fn at_risk_hour(&self) -> i32 {
        (self.deadline_hour + AT_RISK_AFTER_HOURS).min(23)
    }
}

/// A problem as a check finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub kind: &'static str,
    pub key: String,
    pub summary: String,
    pub command: String,
    pub detail: Option<Value>,
}

/// An open problem.
#[derive(Debug, Clone, PartialEq)]
pub struct Open {
    pub id: i64,
    pub kind: String,
    pub key: String,
    pub summary: String,
    pub command: String,
    /// Seconds since it opened, at the `now` it was read for.
    pub age: i64,
}

/// What a check changed.
#[derive(Debug, Default, PartialEq)]
pub struct Change {
    pub opened: Vec<String>,
    pub resolved: Vec<String>,
    /// Open problems whose summary changed (restated without a post).
    pub restated: Vec<String>,
}

/// One line of at most `max` characters, with nothing a credential pattern matches (a stop
/// message is the worker's text; the evidence was checked when stored, a lease's failure was not).
pub fn one_line(text: &str, max: usize) -> String {
    if evidence::unredacted(text).is_some() {
        return "(its message is not shown: it may hold a credential)".into();
    }
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_owned();
    if line.chars().count() > max { line.chars().take(max.saturating_sub(1)).chain(['…']).collect() } else { line }
}

/// How long, in words: "12 min", "5 h", "3 days".
pub fn age(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    if minutes < 60 {
        format!("{minutes} min")
    } else if minutes < 48 * 60 {
        format!("{} h", minutes / 60)
    } else {
        format!("{} days", minutes / (24 * 60))
    }
}

/// The HTTP statuses the engine's provider transport reports in a text (`HttpStatus(502)`,
/// h2-engine `transport.rs`, the client every Intrinio request goes through).
pub fn provider_statuses(text: &str) -> Vec<u16> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("HttpStatus(") {
        let start = from + i + "HttpStatus(".len();
        let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(code) = digits.parse::<u16>()
            && text[start + digits.len()..].starts_with(')')
        {
            found.push(code);
        }
        from = start;
    }
    found
}

fn is_auth(code: u16) -> bool {
    code == 401 || code == 403
}

fn is_provider_error(code: u16) -> bool {
    code == 429 || code >= 500
}

/// The open problems, oldest first, with their ages at `now`.
pub async fn open(c: &impl GenericClient, now: &str) -> Result<Vec<Open>, String> {
    let rows = c
        .query(
            "SELECT problem_id, kind, key, summary, command, GREATEST(extract(epoch FROM $1::text::timestamptz - opened_at), 0)::bigint
             FROM core.problem WHERE resolved_at IS NULL ORDER BY opened_at, problem_id",
            &[&now],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| Open { id: r.get(0), kind: r.get(1), key: r.get(2), summary: r.get(3), command: r.get(4), age: r.get(5) }).collect())
}

/// The database's own clock, as `now` for a check.
pub async fn now(c: &impl GenericClient) -> Result<String, String> {
    Ok(c.query_one("SELECT now()::text", &[]).await.map_err(db)?.get(0))
}

fn is_open(open: &[Open], kind: &str, key: &str) -> bool {
    open.iter().any(|o| o.kind == kind && o.key == key)
}

/// Every problem that holds at `now`.
async fn current(c: &impl GenericClient, s: &Settings, now: &str, open: &[Open]) -> Result<Vec<Found>, String> {
    let mut found = Vec::new();
    if let Some(from) = &s.deadlines_from {
        deadlines(c, s, from, now, open, &mut found).await?;
    }
    if let Some(row) = c
        .query_opt("SELECT reason, to_char(changed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM core.queue_control WHERE paused", &[])
        .await
        .map_err(db)?
    {
        let reason: String = row.get(0);
        found.push(Found {
            kind: "QUEUE_PAUSED",
            key: row.get(1),
            summary: format!("Work queue paused, nothing is handed out: {}", one_line(&reason, 160)),
            command: "vyreon-core queue".into(),
            detail: None,
        });
    }
    failed_jobs(c, &mut found).await?;
    provider(c, now, open, &mut found).await?;
    stalled(c, now, &mut found).await?;
    claims_empty(c, now, open, &mut found).await?;
    // C-051: daily runs that cannot be given work are a problem until they can; and (round x, 11)
    // so is a session due and not queued: the tick queues before it checks, so one is left only
    // when queueing failed, and no digest of that minute is green
    if let Some(d) = &s.daily {
        let why = match daily::build_stopped(c, d).await? {
            Some(why) => Some(format!("build {} {why}, so no new session is queued", d.build)),
            None => daily::due(c, d, now).await?.first().map(|oldest| format!("a session due is not queued (the oldest {oldest}): queueing failed, the service's log says why")),
        };
        if let Some(why) = why {
            found.push(Found { kind: "DAILY_STOPPED", key: d.build.clone(), summary: format!("Daily runs stopped: {why}"), command: "vyreon-core daily".into(), detail: None });
        }
    }
    // C-052: perishable archives missing past the configured age
    if let Some(a) = &s.archive
        && let Some(days) = a.alert_days
    {
        let missing = archive::missing(c, &s.profile, a.expected_from.as_deref(), now).await?;
        let old: Vec<&archive::Missing> = missing.iter().filter(|m| m.age >= i64::from(days) * 86_400).collect();
        if let Some(first) = old.first() {
            found.push(Found {
                kind: "ARCHIVE_MISSING",
                key: "perishables".into(),
                summary: format!(
                    "Perishable archive: {} missing for more than {days} {}, the oldest {} {} (the provider keeps option prints about five sessions)",
                    old.len(),
                    if days == 1 { "day" } else { "days" },
                    first.session,
                    first.category
                ),
                command: "vyreon-core archive list".into(),
                detail: None,
            });
        }
    }
    // C-054: a published day whose reference check disagrees, and one whose check did not run, are
    // two problems, each one alert however many days it holds
    let checks = refcheck::state(c, &s.profile, s.refcheck_from.as_deref(), now).await?;
    if let Some(summary) = checks.disagree_summary() {
        found.push(Found { kind: "REFCHECK_DISAGREES", key: "refcheck".into(), summary, command: "vyreon-core refcheck list".into(), detail: None });
    }
    if let Some(summary) = checks.not_run_summary() {
        found.push(Found { kind: "REFCHECK_NOT_RUN", key: "refcheck".into(), summary, command: "vyreon-core refcheck list".into(), detail: None });
    }
    if let Some(volume) = s.volume_bytes.filter(|v| *v > 0) {
        let size: i64 = c.query_one("SELECT pg_database_size(current_database())", &[]).await.map_err(db)?.get(0);
        let share = size as f64 / volume as f64;
        let line = if is_open(open, "DATABASE_FULL", "volume") { share > DATABASE_CLEAR } else { share > DATABASE_FULL };
        if line {
            found.push(Found {
                kind: "DATABASE_FULL",
                key: "volume".into(),
                summary: format!("Database is {:.0}% of its volume: {} of {}", share * 100.0, gigabytes(size), gigabytes(volume)),
                command: "vyreon-core digest".into(),
                detail: None,
            });
        }
    }
    let row = c
        .query_one(
            "SELECT max(session)::text, (max(session) - ($1::text::timestamptz AT TIME ZONE $2)::date)::int FROM core.session",
            &[&now, &ZONE],
        )
        .await
        .map_err(db)?;
    if let (Some(last), Some(left)) = (row.get::<_, Option<String>>(0), row.get::<_, Option<i32>>(1))
        && left < CALENDAR_DAYS
    {
        found.push(Found {
            kind: "CALENDAR_END",
            key: last.clone(),
            summary: format!("The trading calendar ends on {last}, in {left} days: daily runs stop there until the engine's later calendar is loaded"),
            command: "vyreon-core calendar load <sessions.json>".into(),
            detail: None,
        });
    }
    // what no job derives again stays a problem until each of its days is derived again: a
    // revision of its kind published after the problem opened (the fallen ones are superseded).
    // A job does not resolve it (round w, 8 and 9: a block already past the day, a repair job
    // cancelled after the problem resolved, derive nothing)
    for o in open.iter().filter(|o| o.kind == "NOT_DERIVED") {
        let derived: Option<bool> = c
            .query_one(
                "SELECT bool_and(EXISTS (SELECT FROM vyreon.day_revision r
                                         WHERE r.profile_id = d->>'profile_id' AND r.represented_date = (d->>'day')::date AND r.state = 'PUBLISHED'
                                           AND r.base_revision IS NOT NULL AND r.day_local = (d->>'day_local')::boolean
                                           AND r.published_at >= p.opened_at))
                 FROM core.problem p, jsonb_array_elements(p.detail->'days') d WHERE p.problem_id = $1",
                &[&o.id],
            )
            .await
            .map_err(db)?
            .get(0);
        if derived != Some(true) {
            found.push(Found { kind: "NOT_DERIVED", key: o.key.clone(), summary: o.summary.clone(), command: o.command.clone(), detail: None });
        }
    }
    Ok(found)
}

/// In GiB, the unit the volume is sized in (`120Gi`).
fn gigabytes(bytes: i64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
}

/// (a) Each session from `from` must have its profile's base revision published by the deadline
/// hour in the owner's zone on the next calendar day; the week's last session (Friday, or the day
/// before a Friday holiday) is also Sunday's report at risk if still missing on Saturday two hours
/// after that (the deadline hour of Friday's session, plus `AT_RISK_AFTER_HOURS`). The
/// at-risk problem is raised only before the week's Sunday is over; once raised it stays until
/// the session publishes.
async fn deadlines(c: &impl GenericClient, s: &Settings, from: &str, now: &str, open: &[Open], found: &mut Vec<Found>) -> Result<(), String> {
    const PUBLISHED: &str = "EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.profile_id = $3
                                             AND d.state = 'PUBLISHED' AND d.base_revision IS NULL)";
    let late = c
        .query(
            &format!(
                "SELECT s.session::text, to_char(s.session, 'Dy Mon FMDD'), to_char(s.session + 1, 'Dy')
                 FROM core.session s
                 WHERE s.session >= $1::text::date AND ((s.session + 1) + make_time($5, 0, 0)) AT TIME ZONE $4 <= $2::text::timestamptz
                   AND NOT {PUBLISHED}
                 ORDER BY 1"
            ),
            &[&from, &now, &s.profile, &ZONE, &s.deadline_hour],
        )
        .await
        .map_err(db)?;
    for r in &late {
        let (session, day, next): (String, String, String) = (r.get(0), r.get(1), r.get(2));
        found.push(Found {
            kind: "SESSION_LATE",
            key: session,
            summary: format!("{day} not published by its {next} {:02}:00 MT deadline", s.deadline_hour),
            command: "vyreon-core queue".into(),
            detail: None,
        });
    }
    let at_risk = c
        .query(
            &format!(
                "SELECT s.session::text, to_char(s.session, 'Dy Mon FMDD'),
                        $2::text::timestamptz < ((date_trunc('week', s.session)::date + 7)::timestamp AT TIME ZONE $4)
                 FROM core.session s
                 WHERE s.session >= $1::text::date
                   AND date_trunc('week', s.session) < date_trunc('week', COALESCE((SELECT min(n.session) FROM core.session n WHERE n.session > s.session), s.session + 7))
                   AND ((date_trunc('week', s.session)::date + 5) + make_time($5, 0, 0)) AT TIME ZONE $4 <= $2::text::timestamptz
                   AND NOT {PUBLISHED}
                 ORDER BY 1"
            ),
            &[&from, &now, &s.profile, &ZONE, &s.at_risk_hour()],
        )
        .await
        .map_err(db)?;
    for r in &at_risk {
        let (session, day, before_monday): (String, String, bool) = (r.get(0), r.get(1), r.get(2));
        if before_monday || is_open(open, "REPORT_AT_RISK", &session) {
            found.push(Found {
                kind: "REPORT_AT_RISK",
                key: session,
                summary: format!("Sunday's report at risk: {day} still not published at Sat {:02}:00 MT", s.at_risk_hour()),
                command: "vyreon-core queue".into(),
                detail: None,
            });
        }
    }
    Ok(())
}

/// (c) A job out of attempts: its last stop message, from its evidence (C-049) or its lease.
async fn failed_jobs(c: &impl GenericClient, found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "SELECT j.job_id, j.kind, j.first_session::text, j.last_session::text,
                    (SELECT e.message FROM core.job_evidence e WHERE e.job_id = j.job_id ORDER BY e.attempt DESC LIMIT 1),
                    (SELECT COALESCE(l.failed_stage || ': ', '') || COALESCE(l.failure, 'no reason given') FROM core.lease l
                     WHERE l.job_id = j.job_id AND l.outcome IN ('FAILED', 'EXPIRED') ORDER BY l.lease_id DESC LIMIT 1),
                    (SELECT count(*) FROM core.lease l WHERE l.job_id = j.job_id)
             FROM core.job j WHERE j.state = 'FAILED' ORDER BY j.job_id",
            &[],
        )
        .await
        .map_err(db)?;
    for r in &rows {
        let (job, kind, first, last, evidence, lease, leases): (i64, String, String, String, Option<String>, Option<String>, i64) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
        let what = match kind.as_str() {
            "DAY_BLOCK" if first == last => format!("Day {first}"),
            "DAY_BLOCK" => format!("Days {first} to {last}"),
            "TAPE" => format!("Tape job for {first}"),
            _ => format!("Ordered pass for {first}"),
        };
        let why = evidence.or(lease).unwrap_or_else(|| "no reason recorded".into());
        found.push(Found {
            kind: "JOB_FAILED",
            key: format!("job {job} after {leases} attempts"),
            summary: format!("{what} failed for the last time (job {job}): {}", one_line(&why, 160)),
            command: format!("vyreon-core evidence show {job}"),
            detail: None,
        });
    }
    Ok(())
}

/// (d) The provider failing, from the failure evidence's decisive text (its stop message and the
/// input plan's refusals, never the log excerpt, where retried requests of good attempts also
/// show): an authentication refusal (HTTP 401 or 403) not followed by any success, or provider
/// errors on more than `PROVIDER_ERROR_ATTEMPTS` attempts in `PROVIDER_WINDOW_MINUTES`.
async fn provider(c: &impl GenericClient, now: &str, open: &[Open], found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "SELECT e.job_id, e.lease_id, extract(epoch FROM $1::text::timestamptz - e.stored_at)::float8, e.message, COALESCE(e.refusals::text, ''),
                    to_char(e.stored_at AT TIME ZONE $2, 'Dy HH24:MI'),
                    EXISTS (SELECT FROM vyreon.day_revision d WHERE d.published_at > e.stored_at AND d.published_at <= $1::text::timestamptz)
                    OR EXISTS (SELECT FROM core.job j WHERE j.state = 'DONE' AND j.finished_at > e.stored_at AND j.finished_at <= $1::text::timestamptz)
             FROM core.job_evidence e
             WHERE e.stored_at > $1::text::timestamptz - interval '7 days' AND e.stored_at <= $1::text::timestamptz
             ORDER BY e.stored_at DESC, e.job_evidence_id DESC",
            &[&now, &ZONE],
        )
        .await
        .map_err(db)?;
    let mut auth = None;
    let mut errors = std::collections::BTreeSet::new();
    let mut codes = std::collections::BTreeSet::new();
    let mut quiet = true;
    for r in &rows {
        let (job, lease, ago, message, refusals, at, after): (i64, i64, f64, String, String, String, bool) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
        let statuses = provider_statuses(&format!("{message}\n{refusals}"));
        if auth.is_none() && !after && let Some(code) = statuses.iter().copied().find(|c| is_auth(*c)) {
            auth = Some((job, code, at));
        }
        if statuses.iter().any(|c| is_provider_error(*c)) {
            if ago <= PROVIDER_QUIET_HOURS * 3600.0 {
                quiet = false;
            }
            if ago <= PROVIDER_WINDOW_MINUTES * 60.0 {
                errors.insert(lease);
                codes.extend(statuses.iter().copied().filter(|c| is_provider_error(*c)));
            }
        }
    }
    if let Some((job, code, at)) = auth {
        found.push(Found {
            kind: "PROVIDER_AUTH",
            key: "intrinio".into(),
            summary: format!("The provider (Intrinio) refuses our API key: HTTP {code} on job {job} at {at} MT, and nothing has succeeded since"),
            command: format!("vyreon-core evidence show {job}"),
            detail: None,
        });
    }
    let was_open = is_open(open, "PROVIDER_ERRORS", "intrinio");
    if errors.len() > PROVIDER_ERROR_ATTEMPTS || (was_open && !quiet) {
        let codes: Vec<String> = codes.iter().map(u16::to_string).collect();
        let summary = if errors.len() > PROVIDER_ERROR_ATTEMPTS {
            format!("Provider (Intrinio) errors stopped {} attempts in the last 30 minutes (HTTP {})", errors.len(), codes.join(", "))
        } else {
            "Provider (Intrinio) errors are still stopping attempts".into()
        };
        found.push(Found { kind: "PROVIDER_ERRORS", key: "intrinio".into(), summary, command: "vyreon-core evidence list".into(), detail: None });
    }
    Ok(())
}

/// The kinds of work, in the order the watch states them, with the ScaledJob that starts their pods.
const KINDS: [(&str, &str); 3] = [("TAPE", "tape"), ("DAY_BLOCK", "day-block"), ("ORDERED_PASS", "ordered-pass")];

/// C-065 (health.sh's stall), per kind (C-068, audit opsurface ops.md 8 and 5): claimable work of a
/// kind and no live lease of that kind, continuously for `STALL_MINUTES` (`DAILY_STALL_MINUTES` when
/// every claimable job of the kind is a daily run's): no pod of that kind runs (its ScaledJob not
/// starting pods, its pods Pending for want of a node or failing before they claim, a Job out of
/// completions). A lease of another kind no longer masks it: a day block renewing its lease for a
/// day said nothing of tapes waiting with no pod. One problem per kind (key: the kind). The clock is
/// the later of when the watch first saw the condition for the kind (`core.stall_seen`, written
/// here), the last lease of the kind granted or ended, so a pod that claims and stops within a minute
/// restarts it, and when the kind's work became claimable (the queue's resume, its oldest claimable
/// job's queueing, the earliest admission of a build with such work), which the watch may not have
/// seen.
///
/// And the chain (key `chain`): nothing claimable of any kind, no live lease, the queue not paused,
/// and jobs of a running build still queued, for `STALL_MINUTES`: the queued work waits for something
/// nothing will do (a day not scheduled, a block of a withdrawn build: `build hand-over`). Not while a
/// pass of a running build stands FAILED: it holds the later passes by the claim's rule and is its own
/// problem (`JOB_FAILED`), one alert for one cause. A paused queue or a build not admitted makes
/// nothing claimable: those are their own problems.
async fn stalled(c: &impl GenericClient, now: &str, found: &mut Vec<Found>) -> Result<(), String> {
    let leased: Vec<String> = c
        .query(
            "SELECT DISTINCT j.kind FROM core.lease l JOIN core.job j USING (job_id) WHERE l.ended_at IS NULL AND l.expires_at >= $1::text::timestamptz",
            &[&now],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // every kind leased: no stall, and the claimability view (the queue's longest query) is not
    // read (audit round 2 of core-watch, the suspicion: the check holds the problem lock a publish
    // may wait for)
    if KINDS.iter().all(|(k, _)| leased.iter().any(|l| l == k)) {
        c.execute("DELETE FROM core.stall_seen", &[]).await.map_err(db)?;
        return Ok(());
    }
    // (kind, claimable, not daily, oldest queued, earliest admission of a build with such work)
    let rows = c
        .query(
            "WITH k AS (SELECT kind, priority, created_at, engine_build FROM core.claimable_job)
             SELECT kind, count(*), count(*) FILTER (WHERE priority <> 1), min(created_at)::text,
                    (SELECT min(b.admitted_at) FROM core.engine_build b WHERE b.engine_build IN (SELECT engine_build FROM k x WHERE x.kind = k.kind))::text
             FROM k GROUP BY kind",
            &[],
        )
        .await
        .map_err(db)?;
    let mut any_claimable = false;
    for (kind, pod) in KINDS {
        let row = rows.iter().find(|r| r.get::<_, String>(0) == kind);
        let claimable: i64 = row.map_or(0, |r| r.get(1));
        any_claimable |= claimable > 0;
        if claimable == 0 || leased.iter().any(|l| l == kind) {
            c.execute("DELETE FROM core.stall_seen WHERE kind = $1", &[&kind]).await.map_err(db)?;
            continue;
        }
        let row = row.expect("a kind with claimable work has its row");
        let (backfill, queued, admitted): (i64, Option<String>, Option<String>) = (row.get(2), row.get(3), row.get(4));
        c.execute("INSERT INTO core.stall_seen (kind, since) VALUES ($1, $2::text::timestamptz) ON CONFLICT (kind) DO NOTHING", &[&kind, &now]).await.map_err(db)?;
        let limit = if backfill == 0 { DAILY_STALL_MINUTES } else { STALL_MINUTES };
        // audit round 2 of core-watch, 7: the clock never starts before the work could be claimed,
        // whatever the watch did not see (a restart over an overnight pause)
        let (minutes, since) = stall_clock(
            c,
            now,
            kind,
            "(SELECT max(GREATEST(l.granted_at, COALESCE(l.ended_at, l.granted_at))) FROM core.lease l JOIN core.job j USING (job_id)
              WHERE j.kind = $5 AND l.granted_at <= $1::text::timestamptz)",
            &queued,
            &admitted,
        )
        .await?;
        if minutes >= limit {
            let words = kind_words(kind);
            let mut title = words.to_owned();
            title[..1].make_ascii_uppercase();
            found.push(Found {
                kind: "STALLED",
                key: kind.into(),
                summary: format!("{title} stalled since {since} MT: {claimable} claimable and none leased, so no `{pod}` pod is running"),
                command: "vyreon-core queue".into(),
                detail: None,
            });
        }
    }
    // the chain: nothing claimable, nothing leased, queued work of a running build left
    let r = c
        .query_one(
            "SELECT (SELECT count(*) FROM core.job j WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL AND core.build_runs(j.engine_build)),
                    (SELECT string_agg(n || ' ' || CASE kind WHEN 'DAY_BLOCK' THEN 'day blocks' WHEN 'TAPE' THEN 'tape jobs' ELSE 'ordered passes' END, ', ' ORDER BY kind)
                     FROM (SELECT kind, count(*) AS n FROM core.job j WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL AND core.build_runs(j.engine_build)
                           GROUP BY kind) g),
                    (SELECT paused FROM core.queue_control),
                    EXISTS (SELECT FROM core.job j WHERE j.kind = 'ORDERED_PASS' AND j.state = 'FAILED' AND core.build_runs(j.engine_build))",
            &[],
        )
        .await
        .map_err(db)?;
    let (queued, kinds, paused, failed_pass): (i64, Option<String>, bool, bool) = (r.get(0), r.get(1), r.get(2), r.get(3));
    if any_claimable || !leased.is_empty() || paused || failed_pass || queued == 0 {
        c.execute("DELETE FROM core.stall_seen WHERE kind = 'CHAIN'", &[]).await.map_err(db)?;
        return Ok(());
    }
    c.execute("INSERT INTO core.stall_seen (kind, since) VALUES ('CHAIN', $1::text::timestamptz) ON CONFLICT (kind) DO NOTHING", &[&now]).await.map_err(db)?;
    let (minutes, since) = stall_clock(
        c,
        now,
        "CHAIN",
        "(SELECT max(GREATEST(l.granted_at, COALESCE(l.ended_at, l.granted_at))) FROM core.lease l WHERE l.granted_at <= $1::text::timestamptz AND $5::text IS NOT NULL)",
        &None,
        &None,
    )
    .await?;
    if minutes >= STALL_MINUTES {
        found.push(Found {
            kind: "STALLED",
            key: "chain".into(),
            summary: format!(
                "Work stalled since {since} MT: {queued} jobs queued ({}), none claimable and none leased: they wait for work that is not scheduled or not running",
                kinds.unwrap_or_default()
            ),
            command: "vyreon-core queue".into(),
            detail: None,
        });
    }
    Ok(())
}

/// The stall clock of `kind` at `now`: minutes since the later of when the watch first saw it
/// (`core.stall_seen`), `last` (an expression of the last lease that moved, over `$1` now and `$5`
/// the kind), the queue's last change, and `queued` and `admitted`; and that moment in the owner's
/// zone.
async fn stall_clock(c: &impl GenericClient, now: &str, kind: &str, last: &str, queued: &Option<String>, admitted: &Option<String>) -> Result<(f64, String), String> {
    let r = c
        .query_one(
            &format!(
                "WITH s AS (SELECT GREATEST((SELECT since FROM core.stall_seen WHERE kind = $5), {last},
                                            (SELECT changed_at FROM core.queue_control),
                                            $3::text::timestamptz, $4::text::timestamptz) AS t)
                 SELECT extract(epoch FROM $1::text::timestamptz - t)::float8 / 60, to_char(t AT TIME ZONE $2, 'Dy HH24:MI') FROM s"
            ),
            &[&now, &ZONE, queued, admitted, &kind],
        )
        .await
        .map_err(db)?;
    Ok((r.get(0), r.get(1)))
}

/// C-065 (health.sh's "pods that found nothing to claim"): claims that found nothing while a job
/// of their kind and build that no other claim held was claimable (`core.claim_miss`, kept by the
/// claim with that job), at least `MISS_CLAIMS` within `MISS_WINDOW_MINUTES` while the job each
/// missed is still claimable now; open while any recurs and its job is still there. A claim that
/// found only work other claims were taking keeps no miss (audit round 2 of core-watch, 6), and a
/// job queued after a miss is not the work it missed. The first of the groups (most misses, then
/// kind, build and daily, so the same rows give the same text: 16) names the work.
async fn claims_empty(c: &impl GenericClient, now: &str, open: &[Open], found: &mut Vec<Found>) -> Result<(), String> {
    c.execute("DELETE FROM core.claim_miss WHERE at < $1::text::timestamptz - make_interval(days => $2::float8::int)", &[&now, &MISS_KEEP_DAYS])
        .await
        .map_err(db)?;
    let rows = c
        .query(
            "SELECT m.kind, m.engine_build, count(*),
                    (SELECT count(*) FROM core.claimable_job j WHERE j.kind = m.kind AND j.engine_build = m.engine_build AND (NOT m.daily OR j.priority = 1))
             FROM core.claim_miss m
             WHERE m.at > $1::text::timestamptz - make_interval(secs => $2::float8 * 60) AND m.at <= $1::text::timestamptz
               AND EXISTS (SELECT FROM core.claimable_job j WHERE j.job_id = m.job_id)
             GROUP BY m.kind, m.engine_build, m.daily ORDER BY 3 DESC, 1, 2, m.daily",
            &[&now, &MISS_WINDOW_MINUTES],
        )
        .await
        .map_err(db)?;
    let was_open = is_open(open, "CLAIMS_EMPTY", "claims");
    let waiting: Vec<(String, String, i64, i64)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).filter(|w| w.3 > 0).collect();
    let misses: i64 = waiting.iter().map(|w| w.2).sum();
    if misses >= MISS_CLAIMS || (was_open && misses > 0) {
        let (kind, build, _, claimable) = &waiting[0];
        found.push(Found {
            kind: "CLAIMS_EMPTY",
            key: "claims".into(),
            summary: format!(
                "Workers left with nothing while work waited: {misses} claims found nothing in the last {MISS_WINDOW_MINUTES:.0} min, {claimable} {} of build {} still claimable",
                kind_words(kind),
                short(build)
            ),
            command: "vyreon-core queue".into(),
            detail: None,
        });
    }
    Ok(())
}

/// A build's id as a line can hold it.
fn short(build: &str) -> String {
    if build.chars().count() > 16 { build.chars().take(12).chain(['…']).collect() } else { build.to_owned() }
}

/// Opens a problem in the caller's transaction (with its alert), unless it is open already.
async fn open_one(c: &impl GenericClient, f: &Found, now: Option<&str>) -> Result<bool, String> {
    // a publish opens NOT_DERIVED in its own transaction: it waits for a digest being composed
    c.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    let n = c
        .execute(
            "INSERT INTO core.problem (kind, key, opened_at, summary, command, detail)
             VALUES ($1, $2, COALESCE($3::text::timestamptz, now()), $4, $5, $6::text::jsonb)
             ON CONFLICT (kind, key) WHERE resolved_at IS NULL DO NOTHING",
            &[&f.kind, &f.key, &now, &f.summary, &f.command, &f.detail.as_ref().map(Value::to_string)],
        )
        .await
        .map_err(db)?;
    if n == 1 {
        notice::alert(c, &alert_text(&f.summary, &f.command)).await?;
    }
    Ok(n == 1)
}

pub fn alert_text(summary: &str, command: &str) -> String {
    format!("🔴 {summary}\nTo see more: `{command}`")
}

pub fn resolved_text(summary: &str, age_seconds: i64) -> String {
    format!("✅ Resolved: {summary} (it was open {})", age(age_seconds))
}

/// Finds the problems that hold at `now`, opens the new ones and resolves the ones that cleared,
/// with their posts, in one transaction.
pub async fn check(client: &mut Client, s: &Settings, now: &str) -> Result<Change, String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    let open = open(&tx, now).await?;
    let found = current(&tx, s, now, &open).await?;
    let mut change = Change::default();
    for f in &found {
        if !is_open(&open, f.kind, &f.key) && open_one(&tx, f, Some(now)).await? {
            change.opened.push(f.summary.clone());
        }
        // C-050 amended 2026-11-20: an open problem says what holds now (its counts, its oldest
        // day), not what held when it opened; nothing is posted (one alert, one "Resolved")
        if let Some(o) = open.iter().find(|o| o.kind == f.kind && o.key == f.key)
            && (o.summary != f.summary || o.command != f.command)
        {
            tx.execute("UPDATE core.problem SET summary = $2, command = $3 WHERE problem_id = $1", &[&o.id, &f.summary, &f.command]).await.map_err(db)?;
            change.restated.push(f.summary.clone());
        }
    }
    for o in &open {
        if !found.iter().any(|f| f.kind == o.kind && f.key == o.key) {
            tx.execute("UPDATE core.problem SET resolved_at = GREATEST($2::text::timestamptz, opened_at) WHERE problem_id = $1", &[&o.id, &now])
                .await
                .map_err(db)?;
            notice::alert(&tx, &resolved_text(&o.summary, o.age)).await?;
            change.resolved.push(o.summary.clone());
        }
    }
    tx.commit().await.map_err(db)?;
    Ok(change)
}

/// A publish (or a cancelled ordered pass, audit round x, 2) left days that no job derives again
/// (C-037): a problem, opened in the caller's transaction, until each day's revision of its kind
/// is published again. `days`: (day, profile, day-local); `cause` begins the summary ("A republish left").
pub async fn not_derived(c: &impl GenericClient, days: &[(String, String, bool)], cause: &str) -> Result<(), String> {
    if days.is_empty() {
        return Ok(());
    }
    let mut listed: Vec<String> = days.iter().map(|(day, _, local)| if *local { format!("{day} (day-local)") } else { day.clone() }).collect();
    listed.sort();
    listed.dedup();
    let shown = listed.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    let more = if listed.len() > 5 { format!(" and {} more", listed.len() - 5) } else { String::new() };
    let found = Found {
        kind: "NOT_DERIVED",
        key: format!("{} {}", days[0].1, listed.join(",")),
        summary: format!("{cause} {} days that no job derives again: {shown}{more}", listed.len()),
        command: "vyreon-core log 20".into(),
        detail: Some(json!({"days": days.iter().map(|(day, profile, local)| json!({"day": day, "profile_id": profile, "day_local": local})).collect::<Vec<_>>()})),
    };
    open_one(c, &found, None).await.map(|_| ())
}

/// What a published revision holds, counted from its measurements: (revision_id, slots, withheld,
/// provider_withheld, engine_faults) of the revisions `d` that `$WHERE` names.
const COUNTED: &str = "SELECT d.revision_id, count(m.measurement_id)::int AS slots, (count(*) FILTER (WHERE m.whole))::int AS withheld,
                (count(*) FILTER (WHERE m.whole AND NOT m.fault))::int AS provider_withheld, (count(*) FILTER (WHERE m.fault))::int AS engine_faults
         FROM vyreon.day_revision d
         LEFT JOIN LATERAL (
             SELECT m.measurement_id,
                    strpos(m.body_text, m.measurement_id || '/' || m.object_id || '/' || d.represented_date::text || '/engine-fault\"') > 0 AS whole,
                    strpos(m.body_text, '\"ENGINE_FAULT\"') > 0 OR strpos(m.body_text, '\"PARENT_ENGINE_FAULT\"') > 0 AS fault
             FROM vyreon.measurement m WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date) m ON true
         WHERE d.published_at IS NOT NULL AND $WHERE
         GROUP BY d.revision_id";

/// Counts what each published revision holds, once (a published revision never changes).
pub(crate) async fn tally(c: &impl GenericClient, revisions: &[i64]) -> Result<(), String> {
    if revisions.is_empty() {
        return Ok(());
    }
    c.execute(
        &format!(
            "INSERT INTO core.revision_tally (revision_id, slots, withheld, provider_withheld, engine_faults) {}
             ON CONFLICT (revision_id) DO NOTHING",
            COUNTED.replace("$WHERE", "d.revision_id = ANY($1)")
        ),
        &[&revisions],
    )
    .await
    .map_err(db)?;
    Ok(())
}

/// A session's published revisions of the profile, counted: slots, withheld whole, withheld for
/// the provider, slots with an engine fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub slots: i64,
    pub withheld: i64,
    pub provider_withheld: i64,
    pub engine_faults: i64,
}

impl Tally {
    /// The share of slots with a measurement (not withheld whole), in percent.
    pub fn measured(&self) -> Option<f64> {
        (self.slots > 0).then(|| 100.0 * (self.slots - self.withheld) as f64 / self.slots as f64)
    }
}

/// The tallies of the profile's sessions, by date (only sessions with a published revision). One
/// statement, so one snapshot (audit round x, 7): the revisions published, and their counts (kept
/// ones, and the ones counted here for the first time, which are kept for the next time). A
/// revision replaced between two reads can no longer drop its session's count to nothing.
pub async fn session_tallies(c: &impl GenericClient, profile: &str, sessions: &[String]) -> Result<BTreeMap<String, Tally>, String> {
    // a session counts once its base revision is published; its derived revisions add their slots
    let rows = c
        .query(
            &format!(
                "WITH shown AS (SELECT d.revision_id, d.represented_date FROM vyreon.day_revision d
                            WHERE d.profile_id = $1 AND d.state = 'PUBLISHED' AND d.represented_date = ANY($2::text[]::date[])
                              AND EXISTS (SELECT FROM vyreon.day_revision b WHERE b.represented_date = d.represented_date AND b.profile_id = d.profile_id
                                            AND b.state = 'PUBLISHED' AND b.base_revision IS NULL)),
                 counted AS ({}),
                 kept AS (INSERT INTO core.revision_tally (revision_id, slots, withheld, provider_withheld, engine_faults)
                          SELECT * FROM counted ON CONFLICT (revision_id) DO NOTHING),
                 t AS (SELECT t.revision_id, t.slots, t.withheld, t.provider_withheld, t.engine_faults FROM core.revision_tally t
                       WHERE t.revision_id IN (SELECT revision_id FROM shown)
                       UNION ALL SELECT * FROM counted)
                 SELECT s.represented_date::text, sum(t.slots)::bigint, sum(t.withheld)::bigint, sum(t.provider_withheld)::bigint, sum(t.engine_faults)::bigint
                 FROM shown s JOIN t USING (revision_id)
                 GROUP BY 1",
                COUNTED.replace(
                    "$WHERE",
                    "d.revision_id IN (SELECT revision_id FROM shown) AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)"
                )
            ),
            &[&profile, &sessions],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), Tally { slots: r.get(1), withheld: r.get(2), provider_withheld: r.get(3), engine_faults: r.get(4) })).collect())
}

/// "(and 1 more problem)": the other open problems, counted as problems (never read as more days
/// of the first one's).
fn more_problems(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => " (and 1 more problem)".into(),
        n => format!(" (and {n} more problems)"),
    }
}

/// The verdict: 🔴 when anything is open (the oldest named), else 🟡 naming what was handled and
/// what is not fully judged (C-054 amended 2026-11-20), else 🟢.
pub fn verdict(open: &[Open], handled: &[String], unjudged: &[String]) -> String {
    match open {
        [first, rest @ ..] => format!("🔴 Needs you: {}{}", first.summary, more_problems(rest.len())),
        [] if handled.is_empty() && unjudged.is_empty() => "🟢 Running fine".into(),
        [] => {
            let mut text = "🟡 Running".to_string();
            if !handled.is_empty() {
                text.push_str(&format!("; handled: {}", handled.join(", ")));
            }
            if !unjudged.is_empty() {
                text.push_str(&format!("; not fully judged: {}", unjudged.join(", ")));
            }
            text
        }
    }
}

/// A post of a first line, the lines, and the open problems, cut to `LIMIT` characters: the
/// problems that do not fit are counted, with the command that lists them all. The lines are cut
/// first (audit 2026-11-22 changes-core 18): room is kept for the open problems' heading and their
/// count, so long lines (ten days set aside with their causes) never push the problems out of the
/// post; the lines that do not fit are counted.
pub fn compose(first: &str, lines: &[String], open: &[Open]) -> String {
    let first = if first.chars().count() > 300 { one_line(first, 300) } else { first.to_owned() };
    let mut text = first;
    let reserve = if open.is_empty() {
        "\nOpen problems: none".chars().count()
    } else {
        format!("\nOpen problems ({}):\n• … and {} more: `vyreon-core problems`", open.len(), open.len()).chars().count()
    };
    let cut = |n: usize| format!("\n… and {n} more lines, cut to fit");
    for (i, l) in lines.iter().enumerate() {
        let rest = lines.len() - i - 1;
        let room = if rest == 0 { 0 } else { cut(lines.len()).chars().count() };
        if text.chars().count() + 1 + l.chars().count() + room + reserve > LIMIT {
            text.push_str(&cut(rest + 1));
            break;
        }
        text.push('\n');
        text.push_str(l);
    }
    if open.is_empty() {
        text.push_str("\nOpen problems: none");
        return text;
    }
    text.push_str(&format!("\nOpen problems ({}):", open.len()));
    for (i, o) in open.iter().enumerate() {
        let line = format!("\n• {} · open {} · `{}`", one_line(&o.summary, 220), age(o.age), o.command);
        let rest = open.len() - i - 1;
        let tail = format!("\n• … and {} more: `vyreon-core problems`", rest + 1);
        let room = if rest == 0 { 0 } else { tail.chars().count() };
        if text.chars().count() + line.chars().count() + room > LIMIT {
            text.push_str(&tail);
            return text;
        }
        text.push_str(&line);
    }
    text
}

/// A running day block: its live lease's start, the times its days published under that lease
/// (epoch seconds, in order), and the sessions it has left (the one computing among them).
#[derive(Debug, Clone, PartialEq)]
pub struct Running {
    pub granted: f64,
    pub published: Vec<f64>,
    pub remaining: i64,
}

/// What the backfill line can say of its finish.
#[derive(Debug, Clone, PartialEq)]
pub enum Pace {
    NoneRunning,
    Unmeasured,
    /// `finish` in epoch seconds; `parallel`: the running blocks the estimate assumes go on
    /// running, when blocks wait for a worker.
    Measured { hours_a_day: f64, first_days_only: bool, finish: f64, parallel: Option<usize> },
}

/// The backfill's pace from the running blocks' own days: a day after a block's first takes the
/// time from the previous publication to its own (the next day downloads while one computes, and
/// the reference check runs between, so this is the whole cycle); a block's first day takes from
/// its lease to its publication and includes the block's download, so it is used only when no
/// later day has published (an overestimate, stated). The median of what was measured. Each block
/// finishes its remaining days after its last publication (or its lease), never before `now` plus
/// the days after the one computing; with `waiting` sessions in blocks not yet claimed, the whole
/// remainder at the running blocks' combined pace.
pub fn pace(running: &[Running], waiting: i64, now: f64) -> Pace {
    if running.is_empty() {
        return Pace::NoneRunning;
    }
    let mut later = Vec::new();
    let mut first = Vec::new();
    for r in running {
        let mut at = r.granted;
        for (i, p) in r.published.iter().enumerate() {
            if i == 0 { first.push(p - at) } else { later.push(p - at) }
            at = *p;
        }
    }
    let first_days_only = later.is_empty();
    let Some(day) = median(if first_days_only { first } else { later }) else {
        return Pace::Unmeasured;
    };
    let mut finish = now;
    let mut left = 0;
    for r in running {
        let since = r.published.last().copied().unwrap_or(r.granted);
        finish = finish.max(since + r.remaining as f64 * day).max(now + (r.remaining - 1).max(0) as f64 * day);
        left += r.remaining;
    }
    let parallel = (waiting > 0).then(|| {
        finish = finish.max(now + (left + waiting) as f64 * day / running.len() as f64);
        running.len()
    });
    Pace::Measured { hours_a_day: day / 3600.0, first_days_only, finish, parallel }
}

pub(crate) fn kind_words(kind: &str) -> &'static str {
    match kind {
        "DAY_BLOCK" => "day blocks",
        "ORDERED_PASS" => "ordered passes",
        _ => "tape jobs",
    }
}

/// "2025-01-10 and 2025-01-13 tapes", "tapes on 12 sessions, 2025-01-02 to 2025-01-17".
pub fn gaps_words(gaps: &[(String, String)]) -> String {
    let mut by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (session, what) in gaps {
        by.entry(what.as_str()).or_default().push(session.as_str());
    }
    let parts: Vec<String> = by
        .into_iter()
        .map(|(what, mut days)| {
            days.sort();
            days.dedup();
            match days.as_slice() {
                [one] => format!("{one} {what}"),
                [a, b] => format!("{a} and {b} {what}"),
                _ => format!("{what} on {} sessions, {} to {}", days.len(), days[0], days[days.len() - 1]),
            }
        })
        .collect();
    parts.join("; ")
}

fn faults(n: i64) -> String {
    if n == 1 { "1 engine fault".into() } else { format!("{n} engine faults") }
}

fn pct(v: f64) -> String {
    format!("{v:.1}%")
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// A session's date as the owner reads it: "Mon Nov 16" (with the year when it is not this year's).
const DAY: &str = "CASE WHEN extract(year FROM $D) = extract(year FROM ($1::text::timestamptz AT TIME ZONE 'America/Edmonton'))
                        THEN to_char($D, 'Dy Mon FMDD') ELSE to_char($D, 'Dy Mon FMDD, YYYY') END";

fn day_of(expr: &str) -> String {
    DAY.replace("$D", expr)
}

/// The daily digest at `now`, from the problems as last checked.
pub async fn digest(c: &impl GenericClient, s: &Settings, now: &str) -> Result<String, String> {
    digest_of(c, s, now, &open(c, now).await?).await
}

/// The digest at `now` for the problems `open`.
async fn digest_of(c: &impl GenericClient, s: &Settings, now: &str, open: &[Open]) -> Result<String, String> {
    let today: String = c.query_one("SELECT to_char($1::text::timestamptz AT TIME ZONE $2, 'Dy Mon FMDD')", &[&now, &ZONE]).await.map_err(db)?.get(0);
    let mut lines = Vec::new();

    // the newest published session, and the typical share before it
    let recent: Vec<(String, String)> = c
        .query(
            &format!(
                "SELECT DISTINCT represented_date::text, {} FROM vyreon.day_revision
                 WHERE profile_id = $2 AND state = 'PUBLISHED' AND base_revision IS NULL ORDER BY 1 DESC LIMIT $3",
                day_of("represented_date")
            ),
            &[&now, &s.profile, &(TYPICAL_SESSIONS + 1)],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let dates: Vec<String> = recent.iter().map(|(d, _)| d.clone()).collect();
    let tallies = session_tallies(c, &s.profile, &dates).await?;
    match recent.first() {
        None => lines.push("Latest session stored: none yet".into()),
        Some((date, words)) => {
            let t = tallies.get(date).copied().unwrap_or_default();
            let typical = median(dates[1..].iter().filter_map(|d| tallies.get(d).and_then(Tally::measured)).collect());
            let typical = typical.map_or("no earlier sessions to compare".into(), |v| format!("typical {}", pct(v)));
            let share = t.measured().map_or("no measurements".into(), |v| format!("{} of its measurements have a value", pct(v)));
            lines.push(format!("Latest session stored: {words} · {share} ({typical}) · {}", faults(t.engine_faults)));
        }
    }

    let published = c
        .query_one(
            "SELECT count(DISTINCT represented_date), min(represented_date)::text, max(represented_date)::text FROM vyreon.day_revision
             WHERE profile_id = $2 AND base_revision IS NULL AND published_at > $1::text::timestamptz - interval '24 hours' AND published_at <= $1::text::timestamptz",
            &[&now, &s.profile],
        )
        .await
        .map_err(db)?;
    let (n, first, last): (i64, Option<String>, Option<String>) = (published.get(0), published.get(1), published.get(2));
    lines.push(match (n, first, last) {
        (0, _, _) => "Sessions published in the last 24 hours: 0".into(),
        (1, Some(d), _) => format!("Sessions published in the last 24 hours: 1 ({d})"),
        (n, Some(a), Some(z)) => format!("Sessions published in the last 24 hours: {n} ({a} to {z})"),
        (n, _, _) => format!("Sessions published in the last 24 hours: {n}"),
    });

    // what was handled: provider gaps the engine or the workers stated, attempts retried
    let fresh: Vec<i64> = c
        .query(
            "SELECT d.revision_id FROM vyreon.day_revision d WHERE d.profile_id = $2 AND d.published_at > $1::text::timestamptz - interval '24 hours'
               AND d.published_at <= $1::text::timestamptz AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)",
            &[&now, &s.profile],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    tally(c, &fresh).await?;
    let gaps: Vec<(String, String)> = c
        .query(
            "SELECT session::text, 'tapes' FROM core.tape_cache
             WHERE absence IS NOT NULL AND stored_at > $1::text::timestamptz - interval '24 hours' AND stored_at <= $1::text::timestamptz
             UNION SELECT session::text, kind || ' captures' FROM core.input_capture
             WHERE absence IS NOT NULL AND stored_at > $1::text::timestamptz - interval '24 hours' AND stored_at <= $1::text::timestamptz
             UNION SELECT d.represented_date::text, 'measurements' FROM vyreon.day_revision d JOIN core.revision_tally t USING (revision_id)
             WHERE d.profile_id = $2 AND t.provider_withheld > 0
               AND d.published_at > $1::text::timestamptz - interval '24 hours' AND d.published_at <= $1::text::timestamptz
             ORDER BY 1, 2",
            &[&now, &s.profile],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    lines.push(if gaps.is_empty() {
        "Provider gaps recorded in the last 24 hours: none".into()
    } else {
        format!("Provider gaps recorded in the last 24 hours: {} ({})", gaps.len(), gaps_words(&gaps))
    });
    let retried: i64 = c
        .query_one(
            "SELECT count(*) FROM core.lease l JOIN core.job j USING (job_id)
             WHERE l.outcome IN ('FAILED', 'EXPIRED') AND l.ended_at > $1::text::timestamptz - interval '24 hours' AND l.ended_at <= $1::text::timestamptz
               AND j.state NOT IN ('FAILED', 'CANCELLED')",
            &[&now],
        )
        .await
        .map_err(db)?
        .get(0);
    if retried > 0 {
        lines.push(format!("Attempts retried in the last 24 hours: {retried}"));
    }
    // C-061: each day set aside (its day job FAILED, the day not published: no base, or no day-local
    // revision on it) and each date omitted in the last 24 hours, with its cause code, so a provider
    // day reads at a glance
    let missing = c
        .query(
            &format!(
                "SELECT s.session, {d}, 'Set aside', c->>'code',
                        COALESCE('base revision ' || core.base_without_day_local(s.session, j.profile_id) || ' published, no day-local revision on it; ', '')
                            || (c->>'detail')
                 FROM core.job j JOIN core.session s ON s.session BETWEEN j.first_session AND j.last_session
                 CROSS JOIN LATERAL core.job_failure_cause(j.job_id) AS c
                 WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $2 AND j.state = 'FAILED'
                   AND NOT core.day_published(s.session, j.profile_id)
                 UNION ALL
                 SELECT o.session, {o}, 'Omitted', 'OMITTED (' || (core.job_failure_cause(o.job_id)->>'code') || ')', o.reason
                 FROM core.omitted_session o
                 WHERE o.profile_id = $2 AND o.omitted_at > $1::text::timestamptz - interval '24 hours' AND o.omitted_at <= $1::text::timestamptz
                 ORDER BY 1, 3 DESC",
                d = day_of("s.session"),
                o = day_of("o.session")
            ),
            &[&now, &s.profile],
        )
        .await
        .map_err(db)?;
    for (i, r) in missing.iter().enumerate() {
        if i == MISSING_LINES {
            lines.push(format!("… and {} more days set aside or omitted (GET /v1/sessions states each one's cause)", missing.len() - i));
            break;
        }
        let (words, what, code, detail): (String, String, String, String) = (r.get(1), r.get(2), r.get(3), r.get(4));
        lines.push(format!("{what}: {words} · {code} · {}", one_line(&detail, 160)));
    }
    let omitted = missing.iter().filter(|r| r.get::<_, String>(2) == "Omitted").count();

    // the queue, and a backfill's progress
    let kinds = c
        .query(
            "SELECT kind, count(*) FILTER (WHERE state = 'QUEUED'), count(*) FILTER (WHERE state = 'LEASED'), count(*) FILTER (WHERE state = 'FAILED')
             FROM core.job GROUP BY kind ORDER BY CASE kind WHEN 'DAY_BLOCK' THEN 0 WHEN 'TAPE' THEN 1 ELSE 2 END",
            &[],
        )
        .await
        .map_err(db)?;
    let parts: Vec<String> = kinds
        .iter()
        .filter_map(|r| {
            let (kind, q, l, f): (String, i64, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3));
            let counts: Vec<String> = [(q, "waiting"), (l, "running"), (f, "failed")].iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}")).collect();
            (!counts.is_empty()).then(|| format!("{} {}", kind_words(&kind), counts.join(", ")))
        })
        .collect();
    lines.push(if parts.is_empty() { "Work queue: nothing waiting or running".into() } else { format!("Work queue: {}", parts.join(" · ")) });
    let backfill = c
        .query_one(
            "WITH b AS (SELECT DISTINCT engine_build FROM core.job WHERE kind = 'DAY_BLOCK' AND profile_id = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED')),
                  j AS (SELECT * FROM core.job WHERE kind = 'DAY_BLOCK' AND profile_id = $1 AND state <> 'CANCELLED' AND engine_build IN (SELECT engine_build FROM b))
             SELECT count(*), count(*) FILTER (WHERE j.next_session IS NULL OR s.session < j.next_session),
                    count(*) FILTER (WHERE j.state = 'QUEUED' AND s.session >= j.next_session)
             FROM j JOIN core.session s ON s.session BETWEEN j.first_session AND j.last_session",
            &[&s.profile],
        )
        .await
        .map_err(db)?;
    let (total, done, waiting): (i64, i64, i64) = (backfill.get(0), backfill.get(1), backfill.get(2));
    if total > done {
        // C-050 amended 2026-11-20: the pace is measured on the running blocks' own days (their
        // live leases' publications), never a 24-hour count that a block's first day dominates
        let running = c
            .query(
                "SELECT extract(epoch FROM l.granted_at)::float8,
                        ARRAY(SELECT extract(epoch FROM d.published_at)::float8 FROM vyreon.day_revision d
                              WHERE d.profile_id = j.profile_id AND d.engine_build = j.engine_build AND d.base_revision IS NULL
                                AND d.represented_date BETWEEN j.first_session AND j.last_session
                                AND d.published_at >= l.granted_at AND d.published_at <= $2::text::timestamptz
                              ORDER BY d.published_at),
                        (SELECT count(*) FROM core.session s WHERE s.session BETWEEN j.next_session AND j.last_session)
                 FROM core.job j JOIN core.lease l ON l.job_id = j.job_id AND l.ended_at IS NULL
                 WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $1 AND j.state = 'LEASED' AND j.next_session IS NOT NULL
                 ORDER BY j.job_id",
                &[&s.profile, &now],
            )
            .await
            .map_err(db)?
            .iter()
            .map(|r| Running { granted: r.get(0), published: r.get(1), remaining: r.get(2) })
            .collect::<Vec<_>>();
        let now_epoch: f64 = c.query_one("SELECT extract(epoch FROM $1::text::timestamptz)::float8", &[&now]).await.map_err(db)?.get(0);
        let eta = match pace(&running, waiting, now_epoch) {
            Pace::NoneRunning => "no block running, so no estimate".to_string(),
            Pace::Unmeasured => "finish unknown until a running block publishes a day".to_string(),
            Pace::Measured { hours_a_day, first_days_only, finish, parallel } => {
                let at: String = c
                    .query_one(
                        "SELECT CASE WHEN $2::float8 - extract(epoch FROM $1::text::timestamptz) <= 144 * 3600 THEN to_char(t, 'Dy HH24:MI') || ' MT' ELSE to_char(t, 'Mon FMDD') END
                         FROM (SELECT to_timestamp($2::float8) AT TIME ZONE 'America/Edmonton' AS t) x",
                        &[&now, &finish],
                    )
                    .await
                    .map_err(db)?
                    .get(0);
                let basis = if first_days_only { " (a block's first day, with its download)" } else { "" };
                let parallel = parallel.map_or(String::new(), |n| format!(" with {n} {} running", if n == 1 { "block" } else { "blocks" }));
                format!("about {hours_a_day:.1} h a day{basis} · expected to finish about {at}{parallel}")
            }
        };
        lines.push(format!("Backfill progress: {done} of {total} sessions done · {eta}"));
    }

    let size: i64 = c.query_one("SELECT pg_database_size(current_database())", &[]).await.map_err(db)?.get(0);
    lines.push(match s.volume_bytes.filter(|v| *v > 0) {
        Some(v) => format!("Database size: {} of {} ({:.0}%)", gigabytes(size), gigabytes(v), 100.0 * size as f64 / v as f64),
        None => format!("Database size: {} (its volume's size is not set, so it is not checked)", gigabytes(size)),
    });
    if let Some(a) = &s.archive {
        let missing = archive::missing(c, &s.profile, a.expected_from.as_deref(), now).await?;
        lines.push(match missing.first() {
            None => "Perishable archive: nothing missing".into(),
            Some(m) => format!("Perishable archive: {} missing (the oldest {} {}, {})", missing.len(), m.session, m.category, age(m.age)),
        });
    }
    if let Some(line) = refcheck::digest_line(c, now, s.refcheck_from.is_some()).await? {
        lines.push(line);
    }
    // C-054 amended 2026-11-20: published days whose current check ran but is not fully judged
    let checks = refcheck::state(c, &s.profile, s.refcheck_from.as_deref(), now).await?;
    let mut unjudged = Vec::new();
    if let Some(line) = checks.unjudged_line() {
        lines.push(line);
        unjudged.push(format!("reference checks of {}", if checks.unjudged_days == 1 { "1 published day".into() } else { format!("{} published days", checks.unjudged_days) }));
    }
    if let Some(line) = refcheck::tape_digest_line(c, now).await? {
        lines.push(line);
    }
    // C-059: what the sample says of the caches stored before the verdicts
    if let Some(line) = crate::tape::sample_line(&crate::tape::sample(c).await?) {
        lines.push(line);
    }
    if s.daily.is_some() {
        lines.push(daily::digest_line(c, s.daily.as_ref(), now, ZONE).await?);
    }
    match &s.deadlines_from {
        None => lines.push("Publishing deadlines: not checked yet (no first session is set)".into()),
        Some(from) => lines.push(format!("Publishing deadlines: from {from}, each session by {:02}:00 MT the next day", s.deadline_hour)),
    }

    let mut handled = Vec::new();
    if !gaps.is_empty() {
        handled.push(format!("{} provider {}", gaps.len(), if gaps.len() == 1 { "gap" } else { "gaps" }));
    }
    if retried > 0 {
        handled.push(format!("{retried} retried {}", if retried == 1 { "attempt" } else { "attempts" }));
    }
    if omitted > 0 {
        handled.push(format!("{omitted} {} omitted", if omitted == 1 { "date" } else { "dates" }));
    }
    Ok(compose(&format!("{} · {today}", verdict(open, &handled, &unjudged)), &lines, open))
}

/// The Saturday check at `now`: this week's sessions (Monday to the day before), each published
/// or not with its coverage and its history pass, for Sunday's report. Green only with every
/// watched session's base and history pass published (or due later) and no problem open.
pub async fn weekly(c: &impl GenericClient, s: &Settings, now: &str) -> Result<String, String> {
    weekly_of(c, s, now, &open(c, now).await?).await
}

/// The Saturday check at `now` for the problems `open`.
async fn weekly_of(c: &impl GenericClient, s: &Settings, now: &str, open: &[Open]) -> Result<String, String> {
    let rows = c
        .query(
            &format!(
                "SELECT s.session::text, {}, $3::text IS NOT NULL AND s.session >= $3::text::date,
                        EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.profile_id = $2
                                AND d.state = 'PUBLISHED' AND d.base_revision IS NULL),
                        ((s.session + 1) + make_time($4, 0, 0)) AT TIME ZONE 'America/Edmonton' <= $1::text::timestamptz,
                        to_char(s.session + 1, 'Dy'),
                        EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.profile_id = $2
                                AND d.state = 'PUBLISHED' AND d.base_revision IS NOT NULL AND NOT d.day_local)
                 FROM core.session s,
                      (SELECT ($1::text::timestamptz AT TIME ZONE 'America/Edmonton')::date AS today) t
                 WHERE s.session >= date_trunc('week', t.today)::date AND s.session < t.today
                 ORDER BY 1",
                day_of("s.session")
            ),
            &[&now, &s.profile, &s.deadlines_from, &s.deadline_hour],
        )
        .await
        .map_err(db)?;
    // (session, words, watched, published, its deadline passed, its deadline's day, its history
    // pass published)
    let week: Vec<(String, String, bool, bool, bool, String, bool)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6))).collect();
    let dates: Vec<String> = week.iter().map(|w| w.0.clone()).collect();
    let tallies = session_tallies(c, &s.profile, &dates).await?;
    let watched: Vec<_> = week.iter().filter(|w| w.2).collect();
    // C-051: a session whose deadline is still ahead (Friday's, on Saturday morning) is due, not
    // missing; round v, 6: Sunday's report reads the history-reading sensors too (the ordered
    // pass), so a session is complete only with its base and its history pass published, and one
    // without either before its deadline is due (round w, 12: never counted as complete)
    let missing: Vec<&str> = watched.iter().filter(|w| !w.3 && w.4).map(|w| w.1.as_str()).collect();
    let due: Vec<&str> = watched.iter().filter(|w| !(w.3 && w.6) && !w.4).map(|w| w.1.as_str()).collect();
    let no_history: Vec<&str> = watched.iter().filter(|w| w.3 && w.4 && !w.6).map(|w| w.1.as_str()).collect();
    let first = if !missing.is_empty() {
        format!("🔴 Weekly check: Sunday's report at risk, {} of {} sessions not published ({})", missing.len(), watched.len(), missing.join(", "))
    } else if let Some(oldest) = open.first() {
        // an open problem is red whatever the week's deadlines (C-050; round w, 12)
        format!("🔴 Weekly check: Sunday's report is not cleared while a problem is open: {}{}", one_line(&oldest.summary, 200), more_problems(open.len() - 1))
    } else if week.is_empty() {
        "🟡 Weekly check: no trading session this week".to_string()
    } else if watched.is_empty() {
        "🟡 Weekly check: this week's sessions have no publishing deadline yet, so Sunday's report is not checked".to_string()
    } else if !no_history.is_empty() {
        format!(
            "🟡 Weekly check: Sunday's report waits for the history pass of {} of {} sessions ({})",
            no_history.len(),
            watched.len(),
            no_history.join(", ")
        )
    } else if due.is_empty() {
        format!("🟢 Weekly check: all {} of this week's sessions published with their history pass; Sunday's report has what it needs", watched.len())
    } else {
        format!(
            "🟢 Weekly check: on time; {} of {} sessions published with their history pass, {} due later ({})",
            watched.len() - due.len(),
            watched.len(),
            due.len(),
            due.join(", ")
        )
    };
    let mut lines = Vec::new();
    for (date, words, watched, published, passed, by, history) in &week {
        let history = match (*history, *watched && !*passed) {
            (true, _) => "history pass published".to_string(),
            (false, true) => format!("history pass not published yet, due by {by} {:02}:00 MT", s.deadline_hour),
            (false, false) => "history pass not published".to_string(),
        };
        lines.push(match (published, tallies.get(date)) {
            (true, Some(t)) => {
                let share = t.measured().map_or("no measurements".into(), |v| format!("{} of its measurements have a value", pct(v)));
                format!("{words}: published · {share} · {} · {history}", faults(t.engine_faults))
            }
            (true, None) => format!("{words}: published · {history}"),
            (false, _) if *watched && !*passed => format!("{words}: not published yet, due by {by} {:02}:00 MT", s.deadline_hour),
            (false, _) => format!("{words}: not published"),
        });
    }
    lines.push("Unusual-reading flags this week: not counted here; the flag threshold is calibrated by the engine's science, not by core".into());
    lines.push("Reporter dry run: not connected yet".into());
    Ok(compose(&first, &lines, open))
}

/// One minute of the watch: the daily runs' due sessions queued (C-051), the check, then the
/// digest when it is due (09:00 in the owner's zone, once a day) and the weekly check (Saturday,
/// 09:00). A failed step does not stop the next; the first failure is returned.
pub async fn tick(client: &mut Client, s: &Settings, now: &str) -> Result<Change, String> {
    let queued = match &s.daily {
        Some(d) => daily::queue_due(client, d, now).await.map(|_| ()),
        None => Ok(()),
    };
    let checked = check(client, s, now).await;
    // round w, 13: a failed check leaves the problem list stale; no post is composed from it
    let Ok(change) = checked else {
        return checked;
    };
    let row = client
        .query_one(
            "SELECT to_char(t, 'YYYY-MM-DD'), extract(hour FROM t) >= 9, extract(isodow FROM t) = 6
             FROM (SELECT $1::text::timestamptz AT TIME ZONE 'America/Edmonton' AS t) x",
            &[&now],
        )
        .await
        .map_err(db)?;
    let (date, nine, saturday): (String, bool, bool) = (row.get(0), row.get(1), row.get(2));
    if nine {
        post_once(client, &format!("digest {date}"), s, now, false).await?;
        if saturday {
            post_once(client, &format!("weekly {date}"), s, now, true).await?;
        }
    }
    queued.map(|()| change)
}

/// Composes the digest (or the weekly check) and queues it once under `key`. It is composed
/// outside any lock (it counts measurement bodies, which can take long); the problems' lock is
/// then held only to read the open problems again and queue the post if they are the ones it was
/// composed from: a post saying what is open is never queued beside a problem opening or resolving
/// (round v, 7), and a publish opening a problem never waits for a digest's counting (round w, 4).
/// When they changed, nothing is queued and the next minute composes it again.
async fn post_once(client: &mut Client, key: &str, s: &Settings, now: &str, weekly_check: bool) -> Result<(), String> {
    if notice::queued(&*client, key).await? {
        return Ok(());
    }
    let read = open(&*client, now).await?;
    let text = if weekly_check { weekly_of(&*client, s, now, &read).await? } else { digest_of(&*client, s, now, &read).await? };
    let ids = |o: &[Open]| o.iter().map(|p| p.id).collect::<Vec<_>>();
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    if ids(&open(&tx, now).await?) == ids(&read) {
        notice::alert_once(&tx, key, &text).await?;
    }
    tx.commit().await.map_err(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(i: i64, summary: &str) -> Open {
        Open { id: i, kind: "SESSION_LATE".into(), key: format!("{i}"), summary: summary.into(), command: "vyreon-core queue".into(), age: 3 * 3600 + 60 * i }
    }

    #[test]
    fn the_verdict_says_first_whether_to_look() {
        assert_eq!(verdict(&[], &[], &[]), "🟢 Running fine");
        assert_eq!(verdict(&[], &["2 provider gaps".into(), "1 retried attempt".into()], &[]), "🟡 Running; handled: 2 provider gaps, 1 retried attempt");
        let late = problem(1, "Mon Nov 16 not published by its Tue 10:00 MT deadline");
        assert_eq!(verdict(std::slice::from_ref(&late), &["2 provider gaps".into()], &[]), "🔴 Needs you: Mon Nov 16 not published by its Tue 10:00 MT deadline");
        assert_eq!(verdict(&[late.clone(), problem(2, "x"), problem(3, "y")], &[], &[]), "🔴 Needs you: Mon Nov 16 not published by its Tue 10:00 MT deadline (and 2 more problems)");
        // the live digest of 2026-11-20 10:00 MT read "(and 1 more)" after a summary naming days
        assert_eq!(verdict(&[late.clone(), problem(2, "x")], &[], &[]), "🔴 Needs you: Mon Nov 16 not published by its Tue 10:00 MT deadline (and 1 more problem)");
        // C-054 amended 2026-11-20: a check not fully judged is yellow, never red
        let unjudged = ["reference checks of 3 published days".to_string()];
        assert_eq!(verdict(&[], &[], &unjudged), "🟡 Running; not fully judged: reference checks of 3 published days");
        assert_eq!(verdict(&[], &["1 provider gap".into()], &unjudged), "🟡 Running; handled: 1 provider gap; not fully judged: reference checks of 3 published days");
        assert!(verdict(&[late], &[], &unjudged).starts_with("🔴"));
    }

    /// C-050 amended 2026-11-20: the backfill's finish from the running block's own days, on the
    /// cluster's recorded times (`core.lease`, `vyreon.day_revision.published_at`, read 2026-11-20).
    #[test]
    fn the_backfill_finish_is_measured_on_the_running_blocks_days() {
        // job 1420 (2026-10-28 to 10-30): lease 964 granted 08:08:16.87Z, 10-28 published
        // 14:19:16.67Z, two sessions left; at the 10:00 MT digest (16:00Z). The live digest said
        // "Sun 09:26" (2 sessions x 24 h / 1 published in 24 h); its first day took 6.18 h
        let block_1420 = Running { granted: 1795162096.867582, published: vec![1795184356.666791], remaining: 2 };
        let Pace::Measured { hours_a_day, first_days_only, finish, parallel } = pace(std::slice::from_ref(&block_1420), 0, 1795190400.0) else { panic!() };
        assert_eq!((format!("{hours_a_day:.2}"), first_days_only, parallel), ("6.18".into(), true, None));
        assert_eq!(finish.round(), (1795184356.666791_f64 + 2.0 * (1795184356.666791 - 1795162096.867582)).round(), "Sat 02:41Z, Fri 20:41 MT");
        // job 1383 (2026-11-02 to 11-04), lease 926 granted 2026-11-19 15:32:27.78Z: 11-02
        // published 23:55:19.60Z (8.4 h, with the download), 11-03 04:21:17.89Z (4.43 h); at
        // 06:00Z one session left: about 08:47Z, and 11-04 really published at 09:15:31Z
        let block_1383 = Running { granted: 1795102347.781976, published: vec![1795132519.59603, 1795148477.88756], remaining: 1 };
        let Pace::Measured { hours_a_day, first_days_only, finish, .. } = pace(std::slice::from_ref(&block_1383), 0, 1795154400.0) else { panic!() };
        assert_eq!((format!("{hours_a_day:.2}"), first_days_only), ("4.43".into(), false));
        assert!((finish - 1795166131.050349).abs() < 30.0 * 60.0, "within half an hour of the real publication");
        // a day running late: never before now plus the days after the one computing
        let Pace::Measured { finish, .. } = pace(std::slice::from_ref(&block_1383), 0, 1795166000.0) else { panic!() };
        assert_eq!(finish, 1795166000.0);
        // nothing published under the running lease yet; none running
        assert_eq!(pace(&[Running { published: vec![], ..block_1420.clone() }], 0, 1795190400.0), Pace::Unmeasured);
        assert_eq!(pace(&[], 3, 1795190400.0), Pace::NoneRunning);
        // blocks waiting: the whole remainder at the running blocks' pace, from now
        let Pace::Measured { finish, parallel, .. } = pace(&[block_1420.clone()], 3, 1795190400.0) else { panic!() };
        assert_eq!(parallel, Some(1));
        assert!((finish - (1795190400.0 + 5.0 * (1795184356.666791 - 1795162096.867582))).abs() < 1.0);
    }

    #[test]
    fn a_post_stays_under_discords_limit_with_many_problems() {
        let open: Vec<Open> = (0..200).map(|i| problem(i, &format!("Day 2025-01-{:02} failed for the last time (job {i}): {}", i % 28 + 1, "x".repeat(150)))).collect();
        let lines = vec!["Latest session stored: Mon Nov 16 · 98.4% of its measurements have a value (typical 98.1%) · 0 engine faults".to_string(); 6];
        let text = compose(&verdict(&open, &[], &[]), &lines, &open);
        assert!(text.chars().count() <= LIMIT && LIMIT < 2000, "{}", text.chars().count());
        assert!(text.contains("Open problems (200):"));
        let shown = text.lines().filter(|l| l.starts_with("• Day ")).count();
        assert!(text.ends_with(&format!("• … and {} more: `vyreon-core problems`", 200 - shown)), "{text}");
        // a few fit whole
        let few = compose("🔴 x", &lines, &open[..2]);
        assert_eq!(few.lines().filter(|l| l.starts_with("• ")).count(), 2);
        assert!(!few.contains("more:"));
        assert!(compose("🟢 Running fine · Tue Nov 17", &lines, &[]).ends_with("\nOpen problems: none"));
    }

    /// Audit 2026-11-22 changes-core 18: ten long lines (days set aside with 160-character causes)
    /// once filled the post before the open problems, and Discord's cut took the problems and their
    /// command. The lines are cut first now: the problems' heading and count always fit.
    #[test]
    fn long_lines_never_push_the_open_problems_out_of_a_post() {
        let cause = watch_cause();
        let lines: Vec<String> = (1..=12).map(|d| format!("Set aside: Mon Dec {d:02}, 2025 · FAILED · {cause}")).collect();
        let open: Vec<Open> = (0..3).map(|i| problem(i, &format!("Day 2025-12-{:02} failed for the last time (job {i})", i + 1))).collect();
        let text = compose(&verdict(&open, &[], &[]), &lines, &open);
        assert!(text.chars().count() <= LIMIT, "{}", text.chars().count());
        assert!(text.contains("\nOpen problems (3):"), "{text}");
        assert!(text.contains("vyreon-core problems") || text.lines().filter(|l| l.starts_with("• Day ")).count() == 3, "{text}");
        let shown = text.lines().filter(|l| l.starts_with("Set aside: ")).count();
        assert!(shown < 12 && text.contains(&format!("\n… and {} more lines, cut to fit", 12 - shown)), "{text}");
        // with no problem open, the closing line stays too
        let calm = compose("🟡 Running", &lines, &[]);
        assert!(calm.chars().count() <= LIMIT && calm.ends_with("\nOpen problems: none"), "{calm}");
        // lines that fit are all shown, nothing said about cutting
        let few = compose("🟡 Running", &lines[..2], &open);
        assert!(!few.contains("cut to fit") && few.lines().filter(|l| l.starts_with("Set aside: ")).count() == 2);
    }

    /// The first line of a real stop that is not a hold (`test_support::provider_502_stop`), cut
    /// as the digest cuts a cause (160 characters).
    fn watch_cause() -> String {
        one_line(&test_support::provider_502_stop(), 160)
    }

    #[test]
    fn ages_and_lines_read_plainly() {
        assert_eq!((age(59), age(600), age(3 * 3600 + 5), age(47 * 3600), age(5 * 86400)), ("0 min".into(), "10 min".into(), "3 h".into(), "47 h".into(), "5 days".into()));
        assert_eq!(one_line("\n  stop: 2024-07-16 not published\nmore", 100), "stop: 2024-07-16 not published");
        assert_eq!(one_line("abcdef", 4), "abc…");
        assert!(one_line("GET https://x/?api_key=abc", 100).contains("not shown"), "a credential is never posted");
    }

    /// The engine's provider transport error as the real worker log of 2024-07-16 shows it
    /// (`test_support::WORKER_LOG_REFUSED_DAY`, line 169).
    #[test]
    fn provider_statuses_are_read_from_the_engines_transport_error() {
        let real = r#"options 2024-07-16 GLD: Retryable("exit 3: == failed capture /work/h5-acq/GLD/options/2023-12-19/capture: option acquisition:Transport(\"HttpStatus(502)\")")"#;
        assert_eq!(provider_statuses(real), [502]);
        assert_eq!(provider_statuses("HttpStatus(401) and HttpStatus(4x) HttpStatus(429)"), [401, 429]);
        assert!(provider_statuses("prepare:Science(\"close quotes cover under half of the population\")").is_empty());
    }

    #[test]
    fn provider_gaps_are_counted_by_session_not_listed_by_ticker() {
        let g = |s: &str, w: &str| (s.to_owned(), w.to_owned());
        assert_eq!(gaps_words(&[g("2025-01-10", "tapes"), g("2025-01-13", "tapes")]), "2025-01-10 and 2025-01-13 tapes");
        assert_eq!(
            gaps_words(&[g("2025-01-02", "tapes"), g("2025-01-03", "tapes"), g("2025-01-06", "tapes"), g("2025-01-06", "company captures")]),
            "2025-01-06 company captures; tapes on 3 sessions, 2025-01-02 to 2025-01-06"
        );
    }
}
