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
//! Times are computed by PostgreSQL in `ZONE` (the owner's clock, UTC−6 all year: C-072), from a
//! `now` the caller passes, so a check can be run for any moment.
//!
//! Not here (C-065): spend against the ceiling needs DigitalOcean's billing, which core cannot
//! read without a cloud credential, and what core holds (leases) misses the idle nodes that cost
//! most, so the operator keeps it (`deploy/k8s/ops/health.sh`); idle worker nodes are the
//! autoscaler's (KEDA and the pool's minimum of 0).
use crate::daily::{self, Daily};
use crate::{archive, budget, evidence, notice, refcheck};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio_postgres::{Client, GenericClient};

/// The owner's clock (C-072): UTC−6 all year, the one zone for every time core shows the owner
/// ("MT") and every deadline it computes for them. The owner's clock is UTC−6 year-round
/// (corrected 2026-11-11). 'America/Edmonton' meant whatever the tz database of the PostgreSQL
/// image says: older releases fall back to UTC−7 in winter (the cluster's did: its "MT" ran an
/// hour behind the owner's Discord on 2026-11-24), newer ones keep UTC−6 from 2026-11. 'Etc/GMT+6' is the
/// tz database's fixed UTC−6 zone (POSIX sign: "GMT+6" is six hours behind Greenwich), valid both
/// ways in PostgreSQL's AT TIME ZONE (timestamptz to local, and local to timestamptz) and in `TZ=`;
/// a bare '-06:00' string is not (PostgreSQL reads it as a POSIX zone, UTC+6). Market times stay
/// in New York's zone (`daily::READY_ZONE`).
pub const ZONE: &str = "Etc/GMT+6";
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
/// C-068 (audit opsurface ops.md 2): WAL archiving is failing when its last attempt failed and
/// nothing has been archived for this long, or stuck when a finished WAL file has waited twice this
/// long to be archived. CloudNativePG closes a segment every 5 minutes (`archive_timeout`), and the
/// plugin archives two at a time, so a quarter hour is three missed segments.
pub const ARCHIVE_STALE_MINUTES: f64 = 15.0;
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
    /// C-073: hours after publishing (and its worker's end) that a day with no completed check gets
    /// a re-run (`refcheck::queue_reruns`; `VYREON_CORE_REFCHECK_RERUN_HOURS`, default
    /// `refcheck::RERUN_AFTER_HOURS`, `off`: None). Needs `refcheck_from`.
    pub refcheck_rerun_hours: Option<f64>,
    /// C-068: the newest base backup must be at most this many days old (`VYREON_CORE_BACKUP_MAX_DAYS`;
    /// the weekly schedule plus a day); None: the base backups are not watched.
    pub backup_days: Option<i32>,
    /// C-078: the spend against the owner's ceiling (`VYREON_CORE_SPEND_SINCE`, `_BASELINE_USD`,
    /// `_CEILING_USD`); None: not watched.
    pub spend: Option<crate::spend::SpendWatch>,
    /// C-079: the republish of published days whose method versions differ from a newly admitted build's,
    /// planned and scheduled by core; None: not done.
    pub recompute: Option<crate::recompute::RecomputeWatch>,
    /// C-091: the phase's first and last session (`VYREON_CORE_PROGRESS_FROM`/`_TO`), the digest's database line's
    /// total; None: every session a backfill block covers.
    pub phase: Option<(String, String)>,
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
    /// The parts of the watch that failed this minute, each `part: error` (C-069; their problems
    /// open once, the service's log says each minute's).
    pub failing: Vec<String>,
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
             FROM core.problem WHERE resolved_at IS NULL AND NOT quiet ORDER BY opened_at, problem_id",
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

/// One part of the watch (C-069): its key, what it does (said when it cannot), and the kinds of
/// problem it finds, which stay as they were while it cannot run (never resolved by a part that
/// did not look).
#[derive(Debug)]
pub struct Part {
    pub key: &'static str,
    pub what: &'static str,
    pub kinds: &'static [&'static str],
}

/// A part of the watch that failed: one problem per part, its summary the part's error (C-069).
pub const WATCH_FAILING: &str = "WATCH_FAILING";
/// Each minute's error of a failing part is in the service's log.
const WATCH_LOG: &str = "kubectl -n vyreon logs deploy/vyreon-core";

const DEADLINES: Part = Part { key: "deadlines", what: "check the sessions' publishing deadlines", kinds: &["SESSION_LATE", "REPORT_AT_RISK"] };
const QUEUE: Part = Part { key: "queue", what: "check whether the work queue is paused", kinds: &["QUEUE_PAUSED"] };
const FAILED_JOBS: Part = Part { key: "failed jobs", what: "check for work that failed for the last time", kinds: &["JOB_FAILED"] };
const REFUSED: Part = Part { key: "refused days", what: "check for days the input plan refused", kinds: &["DAY_REFUSED"] };
const PROVIDER: Part = Part { key: "provider", what: "check the provider's errors", kinds: &["PROVIDER_AUTH", "PROVIDER_ERRORS", "PROVIDER_THROTTLED"] };
const STALLS: Part = Part { key: "stalls", what: "check for stalled work", kinds: &["STALLED"] };
const CLAIMS: Part = Part { key: "claims", what: "check for workers leaving while work waits", kinds: &["CLAIMS_EMPTY"] };
const DAILY: Part = Part { key: "daily", what: "check the daily runs", kinds: &["DAILY_STOPPED"] };
const ARCHIVE: Part = Part { key: "archive", what: "check the perishable archive", kinds: &["ARCHIVE_MISSING"] };
const REFCHECK: Part = Part { key: "refcheck", what: "check the reference checks (and queue their re-runs)", kinds: &["REFCHECK_DISAGREES", "REFCHECK_NOT_RUN"] };
const PROGRESS: Part = Part { key: "progress", what: "check for workers making no progress", kinds: &["NO_PROGRESS"] };
const THRASHING: Part = Part { key: "thrashing", what: "check for jobs losing their leases", kinds: &["JOB_THRASHING"] };
const STORAGE: Part = Part { key: "storage", what: "check the database volume and its WAL", kinds: &["DATABASE_FULL", "WAL_ARCHIVING"] };
const BACKUPS: Part = Part { key: "backups", what: "check the backups", kinds: &["BACKUP_STALE", "WAL_ARCHIVING"] };
const CALENDAR: Part = Part { key: "calendar", what: "check where the trading calendar ends", kinds: &["CALENDAR_END"] };
const STOP_RULE: Part = Part { key: "stop rule", what: "count the phase-1 stop rule's days", kinds: &["STOP_RULE"] };
const DERIVED_AGAIN: Part = Part { key: "not derived", what: "check the days left to derive again", kinds: &["NOT_DERIVED"] };

/// The tick's own parts beside the check (C-069). Their WATCH_FAILING problems are the tick's to
/// open and resolve (`record`), never the check's: the check cannot see them.
const SPEND: Part = Part { key: "spend", what: "check the spend against the ceiling", kinds: &["SPEND"] };
const CHECK: Part = Part { key: "check", what: "run its checks, so no problem opens or resolves", kinds: &[] };
const DIGEST: Part = Part { key: "digest", what: "queue the daily digest", kinds: &[] };
const WEEKLY: Part = Part { key: "weekly check", what: "queue the weekly check", kinds: &[] };
const SPEND_LINE: Part = Part { key: "spend line", what: "queue the morning's spend line", kinds: &[] };
const RECOMPUTE: Part = Part { key: "recompute", what: "plan the recompute of days a new build's methods changed", kinds: &[] };
const TICK_PARTS: [&Part; 5] = [&CHECK, &DIGEST, &WEEKLY, &SPEND_LINE, &RECOMPUTE];

/// (owner, 2026-11-25) a failure's cause in plain words: the engine's reason, not its stop message's
/// framing ("day D: stop: D not published: <reason>" says "<reason>").
pub fn plain_cause(why: &str) -> String {
    let tail = why.rsplit_once("not published: ").map(|(_, r)| r).or_else(|| why.rsplit_once("stop: ").map(|(_, r)| r)).unwrap_or(why);
    tail.trim().to_owned()
}

/// The problem a part that failed is.
fn failing(p: &Part, error: &str) -> Found {
    Found { kind: WATCH_FAILING, key: p.key.into(), summary: format!("Watch failing: cannot {} ({})", p.what, one_line(error, 240)), command: WATCH_LOG.into(), detail: None }
}

/// What the parts of one check found, and the parts that failed with their errors.
struct Checked {
    found: Vec<Found>,
    failed: Vec<(&'static Part, String)>,
}

impl Checked {
    /// Runs one part in a savepoint of the check's transaction (C-069): a part that fails is rolled
    /// back alone and named, and the others still run. Err only when the transaction itself fails.
    async fn part(&mut self, c: &impl GenericClient, p: &'static Part, run: impl std::future::Future<Output = Result<Vec<Found>, String>>) -> Result<(), String> {
        c.batch_execute("SAVEPOINT watch_part").await.map_err(db)?;
        match run.await {
            Ok(found) => {
                c.batch_execute("RELEASE SAVEPOINT watch_part").await.map_err(db)?;
                self.found.extend(found);
            }
            Err(e) => {
                c.batch_execute("ROLLBACK TO SAVEPOINT watch_part; RELEASE SAVEPOINT watch_part").await.map_err(db)?;
                self.failed.push((p, e));
            }
        }
        Ok(())
    }
}

/// Every problem that holds at `now`, each part of the check on its own (C-069): a part that fails
/// is a WATCH_FAILING problem, and the problems of its kinds that are open stay open as they are.
/// `queueing`: the daily runs' queueing error of this minute, if it failed.
async fn current(c: &impl GenericClient, s: &Settings, now: &str, open: &[Open], queueing: Option<&str>) -> Result<Checked, String> {
    let mut checked = Checked { found: Vec::new(), failed: Vec::new() };
    if let Some(from) = &s.deadlines_from {
        checked
            .part(c, &DEADLINES, async {
                let mut found = Vec::new();
                deadlines(c, s, from, now, open, &mut found).await?;
                Ok(found)
            })
            .await?;
    }
    checked
        .part(c, &QUEUE, async {
            let paused = c
                .query_opt("SELECT reason, to_char(changed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM core.queue_control WHERE paused", &[])
                .await
                .map_err(db)?;
            Ok(paused
                .map(|row| {
                    let reason: String = row.get(0);
                    // C-073: the first cause, and the latest added while paused (`work::pause`)
                    let causes: Vec<&str> = reason.split(crate::work::ALSO).collect();
                    let also = match causes.len() {
                        1 => String::new(),
                        n => format!("; also{}: {}", if n > 2 { format!(" ({} more causes)", n - 1) } else { String::new() }, one_line(causes[n - 1], 160)),
                    };
                    Found {
                        kind: "QUEUE_PAUSED",
                        key: row.get(1),
                        summary: format!("Work queue paused, nothing is handed out: {}{also}", one_line(causes[0], 160)),
                        command: "vyreon-core queue".into(),
                        detail: None,
                    }
                })
                .into_iter()
                .collect())
        })
        .await?;
    checked
        .part(c, &FAILED_JOBS, async {
            let mut found = Vec::new();
            failed_jobs(c, &mut found).await?;
            Ok(found)
        })
        .await?;
    // C-084: the owner's phase-1 stop rule, the part core counts
    checked
        .part(c, &STOP_RULE, async {
            let tally = crate::stoprule::tally(c, &s.profile).await?;
            Ok(tally
                .and_then(|t| {
                    t.summary().map(|summary| Found { kind: "STOP_RULE", key: t.build.clone(), summary, command: format!("vyreon-core stoprule {}", s.profile), detail: None })
                })
                .into_iter()
                .collect())
        })
        .await?;
    checked
        .part(c, &REFUSED, async {
            let mut found = Vec::new();
            refused_days(c, &mut found).await?;
            Ok(found)
        })
        .await?;
    checked
        .part(c, &PROVIDER, async {
            let mut found = Vec::new();
            provider(c, now, open, &mut found).await?;
            Ok(found)
        })
        .await?;
    checked
        .part(c, &STALLS, async {
            let mut found = Vec::new();
            stalled(c, now, &mut found).await?;
            Ok(found)
        })
        .await?;
    checked
        .part(c, &PROGRESS, async {
            let mut found = Vec::new();
            no_progress(c, now, &mut found).await?;
            Ok(found)
        })
        .await?;
    checked
        .part(c, &THRASHING, async {
            let mut found = Vec::new();
            thrashing(c, now, &mut found).await?;
            Ok(found)
        })
        .await?;
    checked
        .part(c, &CLAIMS, async {
            let mut found = Vec::new();
            claims_empty(c, now, open, &mut found).await?;
            Ok(found)
        })
        .await?;
    // C-051: daily runs that cannot be given work are a problem until they can; and (round x, 11)
    // so is a session due and not queued: the tick queues before it checks, so one is left only
    // when queueing failed, and no digest of that minute is green. C-069: the queueing's error is
    // said in the problem, not left to the service's log
    if let Some(d) = &s.daily {
        checked
            .part(c, &DAILY, async {
                let daily = async {
                    let why = match daily::build_stopped(c, d).await? {
                        Some(why) => Some(format!("build {} {why}, so no new session is queued", d.build)),
                        None => match (daily::due(c, d, now).await?.first(), queueing) {
                            (Some(oldest), Some(e)) => Some(format!("a session due is not queued (the oldest {oldest}): queueing failed: {}", one_line(e, 240))),
                            (Some(oldest), None) => Some(format!("a session due is not queued (the oldest {oldest}): queueing failed, the service's log says why")),
                            (None, Some(e)) => Some(format!("queueing failed: {}", one_line(e, 240))),
                            (None, None) => None,
                        },
                    };
                    Ok::<_, String>(why.map(|why| Found { kind: "DAILY_STOPPED", key: d.build.clone(), summary: format!("Daily runs stopped: {why}"), command: "vyreon-core daily".into(), detail: None }))
                };
                // the check of the daily runs failing, and their queueing with it: both errors said
                match daily.await {
                    Ok(found) => Ok(found.into_iter().collect()),
                    Err(e) => Err(queueing.map_or_else(|| e.clone(), |q| format!("{e}; their queueing failed too: {q}"))),
                }
            })
            .await?;
    }
    // C-052: perishable archives missing past the configured age
    if let Some(a) = &s.archive
        && let Some(days) = a.alert_days
    {
        checked
            .part(c, &ARCHIVE, async {
                let missing = archive::missing(c, &s.profile, a.expected_from.as_deref(), now).await?;
                let old: Vec<&archive::Missing> = missing.iter().filter(|m| m.age >= i64::from(days) * 86_400).collect();
                Ok(old
                    .first()
                    .map(|first| Found {
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
                    })
                    .into_iter()
                    .collect())
            })
            .await?;
    }
    // C-054: a published day whose reference check disagrees, and one whose check did not run, are
    // two problems, each one alert however many days it holds
    checked
        .part(c, &REFCHECK, async {
            // C-073: the days due get their re-run first, so a day whose re-run was just queued is
            // not counted as not run
            // C-087: none while the backfill is stopped (`vyreon-core backfill stop`), as with the setting `off`
            let mut rerun = s.refcheck_rerun_hours.filter(|_| s.refcheck_from.is_some());
            if rerun.is_some() && crate::backfill::stopped(c).await? {
                rerun = None;
            }
            if let (Some(hours), Some(from)) = (rerun, &s.refcheck_from) {
                refcheck::queue_reruns(c, &s.profile, from, hours, now).await?;
            }
            let checks = refcheck::state_with(c, &s.profile, s.refcheck_from.as_deref(), rerun, now).await?;
            let mut found = Vec::new();
            if let Some(summary) = checks.disagree_summary() {
                found.push(Found { kind: "REFCHECK_DISAGREES", key: "refcheck".into(), summary, command: "vyreon-core refcheck list".into(), detail: None });
            }
            if let Some(summary) = checks.not_run_summary() {
                found.push(Found { kind: "REFCHECK_NOT_RUN", key: "refcheck".into(), summary, command: "vyreon-core refcheck list".into(), detail: None });
            }
            Ok(found)
        })
        .await?;
    // C-068 (audit opsurface ops.md 2): the volume holds the data and the WAL, and the WAL grows while
    // archiving fails; both, and the base backups, are watched where a volume is set (production)
    if let Some(volume) = s.volume_bytes.filter(|v| *v > 0) {
        checked
            .part(c, &STORAGE, async {
                let st = storage(c, now).await?;
                Ok(storage_found(&st, volume, is_open(open, "DATABASE_FULL", "volume")))
            })
            .await?;
    }
    // C-078: the spend at 80% and 100% of the owner's ceiling
    if let Some(w) = &s.spend {
        checked.part(c, &SPEND, crate::spend::found(c, w, now, open)).await?;
    }
    if let Some(days) = s.backup_days {
        checked
            .part(c, &BACKUPS, async {
                let mut found = Vec::new();
                backups(c, days, now, open, &mut found).await?;
                Ok(found)
            })
            .await?;
    }
    checked
        .part(c, &CALENDAR, async {
            let row = c
                .query_one(
                    "SELECT max(session)::text, (max(session) - ($1::text::timestamptz AT TIME ZONE $2)::date)::int FROM core.session",
                    &[&now, &ZONE],
                )
                .await
                .map_err(db)?;
            let mut found = Vec::new();
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
            Ok(found)
        })
        .await?;
    // what no job derives again stays a problem until each of its days is derived again: a
    // revision of its kind published after the problem opened (the fallen ones are superseded).
    // A job does not resolve it (round w, 8 and 9: a block already past the day, a repair job
    // cancelled after the problem resolved, derive nothing)
    checked
        .part(c, &DERIVED_AGAIN, async {
            let mut found = Vec::new();
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
        })
        .await?;
    // a part that could not look resolves nothing: its kinds' open problems stay as they are
    let mut held = Vec::new();
    for (p, e) in &checked.failed {
        for o in open {
            if let Some(kind) = p.kinds.iter().copied().find(|k| *k == o.kind)
                && !checked.found.iter().chain(&held).any(|f: &Found| f.kind == o.kind && f.key == o.key)
            {
                held.push(Found { kind, key: o.key.clone(), summary: o.summary.clone(), command: o.command.clone(), detail: None });
            }
        }
        held.push(failing(p, e));
    }
    checked.found.extend(held);
    Ok(checked)
}

/// What the server says of its volume, its WAL and its backups (C-068).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Storage {
    /// The core login may read the WAL directory (`pg_monitor`): without it only `data` is read.
    pub readable: bool,
    /// Every database's size (only this database's when not `readable`).
    pub data: i64,
    /// `pg_wal`'s files (segments, history and backup files).
    pub wal: Option<i64>,
    /// Minutes since the archiver's last success and last failure (`pg_stat_archiver`).
    pub archived_ago: Option<f64>,
    pub failed_ago: Option<f64>,
    /// WAL files finished and not yet archived (`.ready`), and minutes the oldest has waited.
    pub waiting: i64,
    pub oldest_waiting: Option<f64>,
}

/// Reads `Storage` at `now`. Never fails for want of a privilege: the functions that need
/// `pg_monitor` are called only when the login holds it (a failed statement would abort the check's
/// transaction).
pub async fn storage(c: &impl GenericClient, now: &str) -> Result<Storage, String> {
    let readable: bool = c
        .query_one(
            "SELECT has_function_privilege('pg_catalog.pg_ls_waldir()', 'EXECUTE')
                AND has_function_privilege('pg_catalog.pg_ls_archive_statusdir()', 'EXECUTE')
                AND pg_has_role('pg_read_all_stats', 'USAGE')",
            &[],
        )
        .await
        .map_err(db)?
        .get(0);
    let r = c
        .query_one(
            "SELECT extract(epoch FROM $1::text::timestamptz - last_archived_time)::float8 / 60,
                    extract(epoch FROM $1::text::timestamptz - last_failed_time)::float8 / 60
             FROM pg_stat_archiver",
            &[&now],
        )
        .await
        .map_err(db)?;
    let mut st = Storage { readable, archived_ago: r.get(0), failed_ago: r.get(1), ..Storage::default() };
    if !readable {
        st.data = c.query_one("SELECT pg_database_size(current_database())", &[]).await.map_err(db)?.get(0);
        return Ok(st);
    }
    let r = c
        .query_one(
            "SELECT (SELECT sum(pg_database_size(oid))::bigint FROM pg_database),
                    (SELECT COALESCE(sum(size), 0)::bigint FROM pg_ls_waldir()),
                    (SELECT count(*) FROM pg_ls_archive_statusdir() WHERE name LIKE '%.ready'),
                    (SELECT extract(epoch FROM $1::text::timestamptz - min(modification))::float8 / 60 FROM pg_ls_archive_statusdir() WHERE name LIKE '%.ready')",
            &[&now],
        )
        .await
        .map_err(db)?;
    st.data = r.get(0);
    st.wal = Some(r.get(1));
    st.waiting = r.get(2);
    st.oldest_waiting = r.get(3);
    Ok(st)
}

/// The problems `st` makes against a volume of `volume` bytes (C-068, audit opsurface ops.md 2):
/// `DATABASE_FULL` on the data and the WAL together (a failing archive grows `pg_wal`, which
/// `pg_database_size` never counted), `WAL_ARCHIVING` when archiving fails or is stuck (or the WAL
/// cannot be read). Round 2, 16: without the WAL (the login lost `pg_monitor`) an open
/// `DATABASE_FULL` stays open, never resolved on the data alone.
pub fn storage_found(st: &Storage, volume: i64, full_open: bool) -> Vec<Found> {
    let mut found = Vec::new();
    let used = st.data + st.wal.unwrap_or(0);
    let share = used as f64 / volume as f64;
    let blind_open = full_open && st.wal.is_none();
    if blind_open || if full_open { share > DATABASE_CLEAR } else { share > DATABASE_FULL } {
        let summary = match st.wal {
            Some(w) => format!("Database volume {:.0}% full: {} of {}, {} of it WAL", share * 100.0, gigabytes(used), gigabytes(volume), gigabytes(w)),
            None if blind_open => format!("Database volume: cannot be judged while the WAL is unreadable ({} of data alone); it was full", gigabytes(st.data)),
            None => format!("Database volume {:.0}% full on the data alone: {} of {} (WAL not read)", share * 100.0, gigabytes(used), gigabytes(volume)),
        };
        found.push(Found { kind: "DATABASE_FULL", key: "volume".into(), summary, command: "vyreon-core digest".into(), detail: None });
    }
    if !st.readable {
        found.push(Found {
            kind: "WAL_ARCHIVING",
            key: "unreadable".into(),
            summary: "The watch cannot read the WAL: the core login lacks pg_monitor (deploy/k8s/README.md, Backups B0)".into(),
            command: "vyreon-core digest".into(),
            detail: None,
        });
    }
    let failing = st.failed_ago.is_some_and(|f| st.archived_ago.is_none_or(|a| f < a)) && st.archived_ago.is_none_or(|a| a >= ARCHIVE_STALE_MINUTES);
    let stuck = st.oldest_waiting.is_some_and(|w| w >= 2.0 * ARCHIVE_STALE_MINUTES);
    if failing || stuck {
        let minutes = |m: Option<f64>| m.map_or_else(|| "never".to_owned(), |m| format!("{} ago", age((m * 60.0) as i64)));
        let wal = st.wal.map_or_else(String::new, |w| format!(", {} in pg_wal", gigabytes(w)));
        found.push(Found {
            kind: "WAL_ARCHIVING",
            key: "archiver".into(),
            summary: format!(
                "WAL archiving to the backup bucket is {}: last success {}, last failure {}; {} WAL files waiting{wal}, kept on the database volume until archived",
                if failing { "failing" } else { "stuck" },
                minutes(st.archived_ago),
                minutes(st.failed_ago),
                st.waiting
            ),
            command: "kubectl -n vyreon logs vyreon-pg-1 --all-containers --since=1h".into(),
            detail: None,
        });
    }
    found
}

/// What the backup plugin itself records (C-068 round 2, `round2-core.md` 2 and 3), as core read it
/// through the Kubernetes API: whether the database's Cluster archives its WAL through the Barman
/// Cloud plugin (`spec.plugins`, `isWALArchiver`), and the plugin's recovery window for the server
/// (the ObjectStore's `status.serverRecoveryWindow.<server>`, which the plugin keeps from the backups
/// it finds in the bucket). PostgreSQL's own signs are not evidence: CloudNativePG reports a WAL file
/// archived when no archiver is configured, and `pg_backup_stop` writes its history file before the
/// copy to the bucket is complete.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackupEvidence {
    pub cluster: String,
    pub archiver: bool,
    pub object_store: Option<String>,
    pub server: Option<String>,
    pub first_recoverability_point: Option<String>,
    pub last_successful_backup: Option<String>,
    pub last_failed_backup: Option<String>,
}

/// The plugin's name in a Cluster's `spec.plugins` (plugin-barman-cloud v0.16.0).
pub const BARMAN_PLUGIN: &str = "barman-cloud.cloudnative-pg.io";

impl BackupEvidence {
    /// From the Cluster object `cluster` (named `name`), and the ObjectStore its archiver names, if
    /// read (`store`). The server is the plugin's `serverName`, else the Cluster's name.
    pub fn from_objects(name: &str, cluster: &Value, store: Option<&Value>) -> BackupEvidence {
        let archiver = cluster["spec"]["plugins"]
            .as_array()
            .and_then(|p| p.iter().find(|p| p["name"] == BARMAN_PLUGIN && p["isWALArchiver"] == true && p["enabled"] != false))
            .cloned();
        let object_store = archiver.as_ref().and_then(|p| p["parameters"]["barmanObjectName"].as_str()).map(str::to_owned);
        let server = archiver.as_ref().map(|p| p["parameters"]["serverName"].as_str().unwrap_or(name).to_owned());
        let window = match (store, &server) {
            (Some(store), Some(server)) => store["status"]["serverRecoveryWindow"][server.as_str()].clone(),
            _ => Value::Null,
        };
        let at = |k: &str| window[k].as_str().map(str::to_owned);
        BackupEvidence {
            cluster: name.to_owned(),
            archiver: archiver.is_some() && object_store.is_some(),
            object_store,
            server,
            first_recoverability_point: at("firstRecoverabilityPoint"),
            last_successful_backup: at("lastSuccessfulBackupTime"),
            last_failed_backup: at("lastFailedBackupTime"),
        }
    }
}

/// Keeps what a read of the plugin's record found, or why it failed (one row, replaced).
pub async fn put_backup_evidence(c: &impl GenericClient, read: Result<&BackupEvidence, &str>) -> Result<(), String> {
    let e = read.ok().cloned().unwrap_or_default();
    let error = read.err().map(|e| one_line(e, 300));
    c.execute(
        "INSERT INTO core.backup_evidence (one, read_at, error, cluster, archiver, object_store, server, first_recoverability_point, last_successful_backup, last_failed_backup)
         VALUES (true, clock_timestamp(), $1, $2, $3, $4, $5, $6::text::timestamptz, $7::text::timestamptz, $8::text::timestamptz)
         ON CONFLICT (one) DO UPDATE SET read_at = EXCLUDED.read_at, error = EXCLUDED.error, cluster = EXCLUDED.cluster, archiver = EXCLUDED.archiver,
             object_store = EXCLUDED.object_store, server = EXCLUDED.server, first_recoverability_point = EXCLUDED.first_recoverability_point,
             last_successful_backup = EXCLUDED.last_successful_backup, last_failed_backup = EXCLUDED.last_failed_backup",
        &[
            &error,
            &read.ok().map(|_| e.cluster.clone()),
            &read.ok().map(|_| e.archiver),
            &e.object_store,
            &e.server,
            &e.first_recoverability_point,
            &e.last_successful_backup,
            &e.last_failed_backup,
        ],
    )
    .await
    .map_err(db)?;
    Ok(())
}

/// The digest's words for the backups, from the plugin's record only (never PostgreSQL's own signs).
async fn backup_words(c: &impl GenericClient, now: &str) -> Result<String, String> {
    let row = c
        .query_opt(
            "SELECT error IS NULL AND read_at > $1::text::timestamptz - interval '15 minutes', archiver,
                    extract(epoch FROM $1::text::timestamptz - last_successful_backup)::float8, first_recoverability_point IS NOT NULL
             FROM core.backup_evidence",
            &[&now],
        )
        .await
        .map_err(db)?;
    Ok(match row.map(|r| (r.get::<_, bool>(0), r.get::<_, Option<bool>>(1), r.get::<_, Option<f64>>(2), r.get::<_, bool>(3))) {
        Some((true, Some(true), Some(ago), true)) => format!("WAL archived by the plugin; newest recoverable base backup {} ago (the plugin's record)", age(ago as i64)),
        Some((true, Some(true), _, _)) => "WAL archived by the plugin; no recoverable base backup recorded".into(),
        Some((true, _, _, _)) => "WAL archiving is not configured".into(),
        _ => "backups unverified (the plugin's record not read)".into(),
    })
}

/// The plugin's record older than this is no evidence (core reads it every 5 minutes).
pub const EVIDENCE_STALE_MINUTES: f64 = 15.0;

/// C-068 round 2: the backups as the plugin records them, when `backup_days` is set. A problem unless
/// there is evidence: the record read within `EVIDENCE_STALE_MINUTES`, the Cluster archiving its WAL
/// through the plugin (`WAL_ARCHIVING` key `not configured` otherwise), and a recovery window with a
/// successful base backup no older than `backup_days` (`BACKUP_STALE` otherwise).
async fn backups(c: &impl GenericClient, days: i32, now: &str, open: &[Open], found: &mut Vec<Found>) -> Result<(), String> {
    // without evidence nothing the plugin's record said is resolved (round 2, 16's principle): an open
    // "archiving is off" stays open while the record cannot be read
    let keep_off = |found: &mut Vec<Found>| {
        if let Some(o) = open.iter().find(|o| o.kind == "WAL_ARCHIVING" && o.key == "not configured") {
            found.push(Found { kind: "WAL_ARCHIVING", key: o.key.clone(), summary: o.summary.clone(), command: o.command.clone(), detail: None });
        }
    };
    let row = c
        .query_opt(
            "SELECT extract(epoch FROM $1::text::timestamptz - read_at)::float8 / 60, error, archiver, server, object_store,
                    first_recoverability_point IS NOT NULL,
                    extract(epoch FROM $1::text::timestamptz - last_successful_backup)::float8 / 86400,
                    last_failed_backup > COALESCE(last_successful_backup, '-infinity')
             FROM core.backup_evidence",
            &[&now],
        )
        .await
        .map_err(db)?;
    let stale = |why: String| Found { kind: "BACKUP_STALE", key: "base backup".into(), summary: why, command: "kubectl -n vyreon get objectstore,backup".into(), detail: None };
    let Some(r) = row else {
        found.push(stale("Backups unverified: core has never read the backup plugin's record (VYREON_CORE_PG_CLUSTER, its Role: README B0)".into()));
        keep_off(found);
        return Ok(());
    };
    let (ago, error, archiver, server, store, window, backup_age, failed_last): (f64, Option<String>, Option<bool>, Option<String>, Option<String>, bool, Option<f64>, Option<bool>) =
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6), r.get(7));
    if let Some(e) = error {
        found.push(stale(format!("Backups unverified: core cannot read the backup plugin's record: {}", one_line(&e, 160))));
        keep_off(found);
        return Ok(());
    }
    if ago >= EVIDENCE_STALE_MINUTES {
        found.push(stale(format!("Backups unverified: the backup plugin's record was last read {} ago", age((ago * 60.0) as i64))));
        keep_off(found);
        return Ok(());
    }
    if archiver != Some(true) {
        found.push(Found {
            kind: "WAL_ARCHIVING",
            key: "not configured".into(),
            summary: "WAL archiving is off: the database's Cluster has no Barman Cloud WAL archiver, so no WAL reaches the bucket (10-postgres.yaml `plugins`)".into(),
            command: "kubectl -n vyreon get cluster -o yaml".into(),
            detail: None,
        });
    }
    let failed = if failed_last == Some(true) { "; the newest attempt failed" } else { "" };
    let server = server.unwrap_or_default();
    let store = store.unwrap_or_default();
    match backup_age {
        Some(a) if window && a <= f64::from(days) => {}
        Some(a) if window => found.push(stale(format!(
            "The newest recoverable base backup of {server} finished {} ago, more than {days} days{failed}",
            age((a * 86400.0) as i64)
        ))),
        _ => found.push(stale(format!("No recoverable base backup: the plugin records none for {server} in ObjectStore {store}{failed}"))),
    }
    Ok(())
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

/// (c) A job out of attempts: its final lease's stop message, from that lease's evidence (C-049) or
/// the lease itself, as the reader states the day's cause (`core.job_failure_cause`, 0039). C-068
/// (audit opsurface ops.md 10): an earlier attempt's evidence is never the final cause.
async fn failed_jobs(c: &impl GenericClient, found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "SELECT j.job_id, j.kind, j.first_session::text, j.last_session::text,
                    (SELECT e.message FROM core.lease l JOIN core.job_evidence e ON e.lease_id = l.lease_id
                     WHERE l.lease_id = f.lease_id),
                    (SELECT COALESCE(l.failed_stage || ': ', '') || COALESCE(l.failure, 'no reason given') FROM core.lease l WHERE l.lease_id = f.lease_id),
                    (SELECT count(*) FROM core.lease l WHERE l.job_id = j.job_id)
             FROM core.job j
             LEFT JOIN LATERAL (SELECT l.lease_id FROM core.lease l WHERE l.job_id = j.job_id AND l.outcome IN ('FAILED', 'EXPIRED')
                                ORDER BY l.lease_id DESC LIMIT 1) f ON true
             WHERE j.state = 'FAILED' AND j.kind <> 'REFCHECK'
               -- C-075: a day held for the input plan's refusal is DAY_REFUSED's, one alert for one cause
               AND NOT EXISTS (SELECT FROM core.day_refusal r WHERE r.job_id = j.job_id AND r.lease_id = f.lease_id AND r.held)
             ORDER BY j.job_id",
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
            // C-074 (release-2 audit, Codex 16, Gemini 9): the day keeps the revisions it had
            "REPUBLISH" => format!("Republish of {first} (its revisions stay as they were)"),
            _ => format!("Ordered pass for {first}"),
        };
        let why = evidence.or(lease).unwrap_or_else(|| "no reason recorded".into());
        found.push(Found {
            kind: "JOB_FAILED",
            key: format!("job {job} after {leases} attempts"),
            summary: format!("{what} failed for the last time: {}", one_line(&plain_cause(&why), 160)),
            command: format!("vyreon-core evidence show {job}"),
            detail: None,
        });
    }
    Ok(())
}

/// C-075 (an observability gap, 2026-11-24): a day the engine's input plan refused and core held
/// (`work::fail`), until the day is published (a day-local revision of it published after the refusal):
/// one problem per day, naming the refused inputs, the job and its build.
async fn refused_days(c: &impl GenericClient, found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "SELECT DISTINCT ON (r.profile_id, r.session) r.session::text, r.job_id, r.engine_build, r.inputs, r.profile_id
             FROM core.day_refusal r
             WHERE r.held
               AND NOT EXISTS (SELECT FROM vyreon.day_revision d
                               WHERE d.represented_date = r.session AND d.profile_id = r.profile_id AND d.day_local
                                 AND d.state = 'PUBLISHED' AND d.published_at > r.refused_at)
             ORDER BY r.profile_id, r.session, r.refused_at DESC",
            &[],
        )
        .await
        .map_err(db)?;
    for r in &rows {
        let (day, job, build, inputs): (String, i64, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3));
        found.push(Found {
            kind: "DAY_REFUSED",
            key: format!("day {day}"),
            summary: format!("Day {day} refused by the engine's input plan (job {job}, build {}): {}", short_build(&build), one_line(&inputs, 240)),
            command: format!("vyreon-core evidence show {job}"),
            detail: None,
        });
    }
    Ok(())
}

fn short_build(build: &str) -> &str {
    build.get(..8).unwrap_or(build)
}

/// (d) The provider failing, from the failure evidence's decisive text (its stop message and the
/// input plan's refusals, never the log excerpt, where retried requests of good attempts also
/// show): an authentication refusal (HTTP 401 or 403) not followed by a success that called the
/// provider, or provider errors on more than `PROVIDER_ERROR_ATTEMPTS` attempts in
/// `PROVIDER_WINDOW_MINUTES`. C-068 (audit opsurface ops.md 9): the success is a tape or day-block
/// lease granted after the refusal (so its worker read the key after it: a worker reads its key file
/// when it starts) that stored a tape cache before it finished (round 2, 15: a tape whose caches were
/// all held already finishes without a request), or published a day's base; an ordered pass calls no provider, and
/// a revision or a job of a lease granted before the refusal proves nothing of the key now.
async fn provider(c: &impl GenericClient, now: &str, open: &[Open], found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "SELECT e.job_id, e.lease_id, extract(epoch FROM $1::text::timestamptz - e.stored_at)::float8, e.message, COALESCE(e.refusals::text, ''),
                    to_char(e.stored_at AT TIME ZONE $2, 'Dy HH24:MI'),
                    EXISTS (SELECT FROM core.lease l JOIN core.job j USING (job_id)
                            WHERE j.kind IN ('TAPE', 'DAY_BLOCK') AND l.granted_at > e.stored_at AND l.granted_at <= $1::text::timestamptz
                              AND ((j.kind = 'TAPE' AND l.outcome = 'DONE' AND l.ended_at <= $1::text::timestamptz
                                    AND EXISTS (SELECT FROM core.tape_cache t WHERE t.session BETWEEN j.first_session AND j.last_session
                                                  AND t.stored_at >= l.granted_at AND t.stored_at <= l.ended_at))
                                   OR EXISTS (SELECT FROM core.lease_revision r JOIN vyreon.day_revision d USING (revision_id)
                                              WHERE r.lease_id = l.lease_id AND d.base_revision IS NULL AND d.published_at <= $1::text::timestamptz)))
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
            summary: format!("The provider (Intrinio) refuses our API key: HTTP {code} on job {job} at {at} MT, and no tape or day block started since has succeeded"),
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
    // C-095: the provider refusing requests for their rate (HTTP 429), as the workers reported each one
    let t = c
        .query_one(
            "SELECT count(*), count(DISTINCT worker), min(committed), max(committed),
                    (array_agg(message ORDER BY at DESC) FILTER (WHERE message IS NOT NULL))[1]
             FROM core.provider_refusal
             WHERE status = 429 AND at > $1::text::timestamptz - make_interval(secs => $2::float8 * 60) AND at <= $1::text::timestamptz",
            &[&now, &PROVIDER_WINDOW_MINUTES],
        )
        .await
        .map_err(db)?;
    let (n, workers, least, most, words): (i64, i64, Option<i32>, Option<i32>, Option<String>) = (t.get(0), t.get(1), t.get(2), t.get(3), t.get(4));
    if n > 0 {
        let words = words.map_or_else(String::new, |w| format!("; its words: \"{}\"", w.chars().take(120).collect::<String>()));
        found.push(Found {
            kind: "PROVIDER_THROTTLED",
            key: "intrinio".into(),
            summary: format!(
                "The provider (Intrinio) refused {n} requests for their rate (HTTP 429) on {workers} workers in the last 30 minutes, while core counted {} to {} requests a minute{words}",
                least.unwrap_or(0),
                most.unwrap_or(0)
            ),
            command: "vyreon-core budget".into(),
            detail: None,
        });
    }
    Ok(())
}

/// The kinds of work, in the order the watch states them, with the ScaledJob that starts their pods.
/// C-068 round 2 (`round2-core.md` 13): the daily session's blocks (DAY_BLOCK of priority 1) are a
/// group of their own, as they have a ScaledJob of their own (`day-block-daily`): a backfill block's
/// lease says nothing of the daily pod.
const KINDS: [(&str, &str); 6] = [
    ("TAPE", "tape"),
    ("DAY_BLOCK", "day-block"),
    ("DAILY_BLOCK", "day-block-daily"),
    ("ORDERED_PASS", "ordered-pass"),
    ("REFCHECK", "refcheck"),
    // C-074
    ("REPUBLISH", "republish"),
];

/// C-073: the kinds whose pods the scaler holds back while the request budget is pressed
/// (`core.scaler_count`): a kind waiting for that reason is not stalled.
const BUDGETED: [&str; 4] = ["TAPE", "DAY_BLOCK", "REFCHECK", "REPUBLISH"];

/// A job's stall group: its kind, or `DAILY_BLOCK` for a daily session's day block (`KINDS`).
const GROUP: &str = "CASE WHEN j.kind = 'DAY_BLOCK' AND j.priority = 1 THEN 'DAILY_BLOCK' ELSE j.kind END";

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
    // (release-3 audit, Gemini 14) a kind is leased by a build that runs: a withdrawn build's draining block
    // (C-077) is no sign that the new build's pods of the kind start; it still counts for the chain below
    let any_leased: bool = c
        .query_one("SELECT EXISTS (SELECT FROM core.lease l WHERE l.ended_at IS NULL AND l.expires_at >= $1::text::timestamptz)", &[&now])
        .await
        .map_err(db)?
        .get(0);
    let leased: Vec<String> = c
        .query(
            &format!(
                "SELECT DISTINCT {GROUP} FROM core.lease l JOIN core.job j USING (job_id)
                 WHERE l.ended_at IS NULL AND l.expires_at >= $1::text::timestamptz AND core.build_runs(j.engine_build)"
            ),
            &[&now],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // C-077 (live, 2026-11-25 22:17 MT: "Day blocks stalled … 45 claimable and none leased" while the 8
    // day-block slots were the old build's draining blocks): a kind with a draining lease within its bound
    // is waiting for those slots, not stalled; the progress channel says so (`progress`'s "Draining" line).
    // Once the drains end (or one passes its bound) the clock starts again from the last lease's end
    // (release-4 audit, Codex 6 and 7) the slot a drain holds is its pod's ScaledJob (KEDA names a Job's pod
    // after it: `day-block-daily-…`, `day-block-…`; VYREON_WORKER_NAME is the pod's name), not its job's
    // priority (a backfill pod may claim a daily block); and a kind with a drain past its bound is not excused
    let slot = format!("CASE WHEN l.worker LIKE 'day-block-daily-%' THEN 'DAILY_BLOCK' WHEN l.worker LIKE 'day-block-%' THEN 'DAY_BLOCK' ELSE {GROUP} END");
    let draining: Vec<String> = c
        .query(
            &format!(
                "SELECT {slot} FROM core.drain d JOIN core.lease l USING (lease_id) JOIN core.job j ON j.job_id = d.job_id
                 WHERE l.ended_at IS NULL AND l.expires_at >= $1::text::timestamptz
                 GROUP BY 1 HAVING bool_and(d.until >= $1::text::timestamptz)"
            ),
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
    // (kind, claimable, not daily, oldest queued, earliest admission of a build with such work, not daily
    // and held back by the scaler for the backfill's stop or its day blocks' ramp: C-086, C-087)
    let rows = c
        .query(
            &format!(
                "WITH k AS (SELECT {GROUP} AS kind, j.kind AS job_kind, j.priority, j.created_at, j.engine_build FROM core.claimable_job j),
                      held AS (SELECT h.job_kind, h.engine_build FROM (SELECT DISTINCT job_kind, engine_build FROM k WHERE priority <> 1) h
                               WHERE core.backfill_held(h.job_kind, h.engine_build, $1::text::timestamptz))
                 SELECT kind, count(*), count(*) FILTER (WHERE priority <> 1), min(created_at)::text,
                        (SELECT min(b.admitted_at) FROM core.engine_build b WHERE b.engine_build IN (SELECT engine_build FROM k x WHERE x.kind = k.kind))::text,
                        count(*) FILTER (WHERE priority <> 1 AND (job_kind, engine_build) IN (SELECT job_kind, engine_build FROM held))
                 FROM k GROUP BY kind"
            ),
            &[&now],
        )
        .await
        .map_err(db)?;
    let mut any_claimable = false;
    let pressed = budget::pressed(c, now).await?.is_some();
    for (kind, pod) in KINDS {
        let row = rows.iter().find(|r| r.get::<_, String>(0) == kind);
        let claimable: i64 = row.map_or(0, |r| r.get(1));
        any_claimable |= claimable > 0;
        // C-073: the backfill's work held back by the scaler while the budget is pressed is the
        // budget's pace, not a stall (its clock starts again after); the daily session's work is
        // never held back (`core.scaler_count`), so it alone still counts (release audit, Codex 4,
        // Gemini 1: a daily tape's stall was hidden by backfill pressure)
        let held_back = pressed && BUDGETED.contains(&kind);
        // C-086, C-087: the backfill's work the scaler holds back for the stop or the ramp is not a stall either
        let all_backfill = row.map_or(0, |r| r.get::<_, i64>(2));
        let backfill = if held_back { 0 } else { all_backfill - row.map_or(0, |r| r.get::<_, i64>(5)) };
        let claimable = claimable - all_backfill + backfill;
        if claimable == 0 || leased.iter().any(|l| l == kind) || draining.iter().any(|l| l == kind) {
            c.execute("DELETE FROM core.stall_seen WHERE kind = $1", &[&kind]).await.map_err(db)?;
            continue;
        }
        let row = row.expect("a kind with claimable work has its row");
        let (queued, admitted): (Option<String>, Option<String>) = (row.get(3), row.get(4));
        c.execute("INSERT INTO core.stall_seen (kind, since) VALUES ($1, $2::text::timestamptz) ON CONFLICT (kind) DO NOTHING", &[&kind, &now]).await.map_err(db)?;
        let limit = if backfill == 0 { DAILY_STALL_MINUTES } else { STALL_MINUTES };
        // audit round 2 of core-watch, 7: the clock never starts before the work could be claimed,
        // whatever the watch did not see (a restart over an overnight pause)
        let (minutes, since) = stall_clock(
            c,
            now,
            kind,
            &format!(
                "(SELECT max(GREATEST(l.granted_at, COALESCE(l.ended_at, l.granted_at))) FROM core.lease l JOIN core.job j USING (job_id)
                  WHERE {GROUP} = $5 AND l.granted_at <= $1::text::timestamptz)"
            ),
            &queued,
            &admitted,
        )
        .await?;
        if minutes >= limit {
            let words = if kind == "DAILY_BLOCK" { "daily day blocks" } else { kind_words(kind) };
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
    // the chain: nothing claimable, nothing leased, queued work of a running build left that waits on
    // something (C-068 round 2, `round2-core.md` 14). Not a pass whose day is neither published for it
    // nor being computed (a missing day, its block cancelled: it holds nothing, C-062; or a day not
    // scheduled yet); and while a pass of a running build stands FAILED, no pass at all (it holds the
    // later ones and is `JOB_FAILED`), but the tapes and blocks still count
    let r = c
        .query_one(
            "WITH q AS (SELECT j.kind FROM core.job j
                        WHERE j.state = 'QUEUED' AND j.next_session IS NOT NULL AND core.build_runs(j.engine_build)
                          AND (j.kind <> 'ORDERED_PASS'
                               OR (NOT EXISTS (SELECT FROM core.job f WHERE f.kind = 'ORDERED_PASS' AND f.state = 'FAILED' AND core.build_runs(f.engine_build))
                                   AND (core.day_published(j.first_session, j.profile_id, j.engine_build)
                                        OR EXISTS (SELECT FROM core.job d WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id
                                                     AND d.state IN ('QUEUED', 'LEASED') AND j.first_session BETWEEN d.next_session AND d.last_session)))))
             SELECT (SELECT count(*) FROM q),
                    (SELECT string_agg(n || ' ' || CASE kind WHEN 'DAY_BLOCK' THEN 'day blocks' WHEN 'TAPE' THEN 'tape jobs' WHEN 'REFCHECK' THEN 'reference-check re-runs' WHEN 'REPUBLISH' THEN 'republishes' ELSE 'ordered passes' END, ', ' ORDER BY kind)
                     FROM (SELECT kind, count(*) AS n FROM q GROUP BY kind) g),
                    (SELECT paused FROM core.queue_control)",
            &[],
        )
        .await
        .map_err(db)?;
    let (queued, kinds, paused): (i64, Option<String>, bool) = (r.get(0), r.get(1), r.get(2));
    if any_claimable || any_leased || paused || queued == 0 {
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
                "Work stalled since {since} MT: {queued} jobs queued ({}), none claimable and none leased: they wait for work that is not running (a withdrawn build's? `build hand-over`)",
                kinds.unwrap_or_default()
            ),
            command: "vyreon-core queue".into(),
            detail: None,
        });
    }
    Ok(())
}

/// C-073: how long a worker may show no progress before NO_PROGRESS.
pub const NO_PROGRESS_MINUTES: f64 = 20.0;
/// C-073: how long past its declared end a wait may run before NO_PROGRESS.
pub const WAIT_GRACE_MINUTES: f64 = 15.0;
/// C-073 hotfix (live, 2026-11-24): a step whose name begins with "download" or "fill" (a day's input
/// download: the worker's `download <date>`, its mirrored `download <date>: fill <date>: …`, a fill's
/// own `fill <date>: …`) is an implicit wait of this many minutes from when it began: Intrinio's bulk
/// option files are no request in core's ledger, and a block's first download runs about three hours
/// (measured 2026-11-24; later days about 37 minutes). 240 = that worst case and an hour's margin.
pub const DOWNLOAD_STEP_MAX_MINUTES: f64 = 240.0;

/// Whether a reported step is a download (`DOWNLOAD_STEP_MAX_MINUTES`).
pub fn is_download_step(step: &str) -> bool {
    let s = step.trim_start().to_ascii_lowercase();
    s.starts_with("download") || s.starts_with("fill")
}

/// C-073 (live investigation, 2026-11-24: the daily run's worker heartbeated through an hour's
/// deliberate EXQ01 cutoff wait, indistinguishable from a stall): a live lease whose worker reports
/// its steps (`work::report_step`; a worker that never reported one, an image before it, is not
/// judged: it could not declare a long computation or a wait) and shows no progress for
/// `NO_PROGRESS_MINUTES`: no request drawn or reported (`core.budget_lease`, `core.request_usage`), no
/// new step or wait, nothing published, no tape cache or reference check stored, since the later of
/// those and its grant. A declared wait covers its time: the deadline is then the later of the above
/// and the wait's end plus `WAIT_GRACE_MINUTES`, so a wait that overstays alerts too. One problem per
/// job (key: the job), alerted once, resolved when progress resumes or the lease ends.
async fn no_progress(c: &impl GenericClient, now: &str, found: &mut Vec<Found>) -> Result<(), String> {
    let rows = c
        .query(
            "WITH live AS (
                 SELECT l.lease_id, l.job_id, l.worker, l.granted_at, j.kind, j.priority, j.next_session, j.first_session, j.last_session,
                        s.step, s.step_since, s.reason, s.progress,
                        -- a download step is a wait of its own bound (`DOWNLOAD_STEP_MAX_MINUTES`), the later of it and a declared one
                        -- (engine D-462: a download that reports its progress is judged by it, 20 minutes without a new
                        -- report, so a hung one alerts within them; the implicit bound is only for one that reports none)
                        GREATEST(s.waiting_until, CASE WHEN (lower(ltrim(s.step)) LIKE 'download%' OR lower(ltrim(s.step)) LIKE 'fill%') AND s.progress IS NULL
                                                       THEN s.step_since + make_interval(secs => $5::float8 * 60) END) AS waiting_until,
                        s.progress IS NULL AND (s.waiting_until IS NULL OR s.waiting_until < s.step_since + make_interval(secs => $5::float8 * 60)) AS implicit
                 FROM core.lease l JOIN core.job j USING (job_id) JOIN core.lease_step s USING (lease_id)
                 WHERE l.ended_at IS NULL AND l.expires_at >= $1::text::timestamptz AND l.granted_at <= $1::text::timestamptz),
             p AS (
                 -- (C-074, release-2 audit, Codex 8) a download that reports its progress is judged by its progress
                 -- alone: request traffic never hides a counter that stopped
                 SELECT live.*, CASE WHEN live.progress IS NOT NULL AND (lower(ltrim(live.step)) LIKE 'download%' OR lower(ltrim(live.step)) LIKE 'fill%')
                                     THEN GREATEST(live.granted_at, live.step_since) ELSE GREATEST(live.granted_at, live.step_since,
                     (SELECT max(b.granted_at) FROM core.budget_lease b
                      WHERE b.worker = live.worker AND b.granted_at >= live.granted_at AND b.granted_at <= $1::text::timestamptz
                        AND (b.used IS NULL OR b.used > 0)),
                     (SELECT max(u.minute) + interval '1 minute' FROM core.request_usage u
                      WHERE u.worker = live.worker AND u.requests > 0 AND u.minute >= date_trunc('minute', live.granted_at) AND u.minute <= $1::text::timestamptz),
                     (SELECT max(d.published_at) FROM core.lease_revision lr JOIN vyreon.day_revision d USING (revision_id)
                      WHERE lr.lease_id = live.lease_id AND d.published_at <= $1::text::timestamptz),
                     (SELECT max(r.stored_at) FROM core.refcheck_report r WHERE r.lease_id = live.lease_id AND r.stored_at <= $1::text::timestamptz),
                     (SELECT max(k.checked_at) FROM core.tape_check k WHERE k.lease_id = live.lease_id AND k.checked_at <= $1::text::timestamptz),
                     (SELECT max(GREATEST(a.requested_at, a.kept_at)) FROM core.perishable_archive a
                      WHERE a.job_id = live.job_id AND a.worker = live.worker AND a.requested_at >= live.granted_at AND a.requested_at <= $1::text::timestamptz),
                     -- release audit (Codex 18, Gemini 10): a tape job's own caches and verdicts only; the
                     -- day block's captures of its own sessions
                     (SELECT max(t.stored_at) FROM core.tape_cache t
                      WHERE live.kind = 'TAPE' AND t.session BETWEEN live.first_session AND live.last_session
                        AND t.stored_at >= live.granted_at AND t.stored_at <= $1::text::timestamptz),
                     (SELECT max(v.stored_at) FROM core.tape_verdict v
                      WHERE live.kind = 'TAPE' AND v.session BETWEEN live.first_session AND live.last_session
                        AND v.stored_at >= live.granted_at AND v.stored_at <= $1::text::timestamptz),
                     (SELECT max(i.stored_at) FROM core.input_capture i
                      WHERE live.kind = 'DAY_BLOCK' AND i.session BETWEEN live.first_session AND live.last_session
                        AND i.stored_at >= live.granted_at AND i.stored_at <= $1::text::timestamptz)
                 ) END AS last FROM live)
             SELECT job_id, CASE WHEN kind = 'DAY_BLOCK' AND priority = 1 THEN 'DAILY_BLOCK' ELSE kind END, COALESCE(next_session, last_session)::text, worker, step,
                    to_char(last AT TIME ZONE $2, 'Dy HH24:MI'), to_char(waiting_until AT TIME ZONE $2, 'Dy HH24:MI'),
                    CASE WHEN implicit AND (lower(ltrim(step)) LIKE 'download%' OR lower(ltrim(step)) LIKE 'fill%')
                         THEN 'a download step is allowed ' || $5::float8::int || ' min' ELSE reason END,
                    waiting_until IS NOT NULL AND waiting_until + make_interval(secs => $4::float8 * 60) < $1::text::timestamptz
             FROM p
             WHERE $1::text::timestamptz > GREATEST(last + make_interval(secs => $3::float8 * 60), waiting_until + make_interval(secs => $4::float8 * 60))
             ORDER BY job_id",
            &[&now, &ZONE, &NO_PROGRESS_MINUTES, &WAIT_GRACE_MINUTES, &DOWNLOAD_STEP_MAX_MINUTES],
        )
        .await
        .map_err(db)?;
    for r in &rows {
        let (job, kind, session, worker, step, since, until, reason, overstayed): (i64, String, String, String, String, String, Option<String>, Option<String>, bool) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6), r.get(7), r.get(8));
        let what = match kind.as_str() {
            "DAILY_BLOCK" => "Daily day block",
            "DAY_BLOCK" => "Day block",
            "ORDERED_PASS" => "Ordered pass",
            "REFCHECK" => "Reference-check re-run",
            "REPUBLISH" => "Republish",
            _ => "Tape job",
        };
        let wait = match (overstayed, until, reason) {
            (true, Some(u), Some(why)) => format!("; its declared wait ({}) was to end {u} MT", one_line(&why, 120)),
            _ => String::new(),
        };
        found.push(Found {
            kind: "NO_PROGRESS",
            key: format!("job {job}"),
            summary: format!("{what} job {job} ({session}) on {worker}: no progress since {since} MT, at step \"{}\"{wait}", one_line(&step, 120)),
            command: format!("kubectl -n vyreon logs {worker}"),
            detail: None,
        });
    }
    Ok(())
}

/// C-073: how many leases a job may lose without moving on before it is JOB_THRASHING.
pub const THRASH_LEASES: i64 = 3;
/// C-073: only leases granted this many hours before `now` count (a job queued again by
/// `retry-failed` after its cause was fixed starts clean a day later at the latest).
pub const THRASH_WINDOW_HOURS: f64 = 24.0;

/// C-073 (MILESTONES' automation debt of 2026-11-23: 11 tapes OOM-killed 12 times in an hour, each
/// kill a lost lease re-queued quietly, and nothing alerted): a job still to do (QUEUED or LEASED)
/// whose attempts on the session it is still on (leases that began there: `start_session`) ended
/// lost or failed `THRASH_LEASES` times or more within `THRASH_WINDOW_HOURS`. One problem per kind
/// of work (key: the kind, a daily session's blocks apart), one alert however many jobs: the job
/// that lost most (then the oldest) is named with its session and its last error (the lost lease's
/// evidence, or its stop as core recorded it); the summary counts the others. A job set aside
/// (FAILED) is JOB_FAILED's, a job that moves on or finishes resolves it, and an attempt stopped by
/// the provider's errors is PROVIDER_ERRORS's (not counted).
async fn thrashing(c: &impl GenericClient, now: &str, found: &mut Vec<Found>) -> Result<(), String> {
    // newest first within each job
    let rows = c
        .query(
            &format!(
                "SELECT {GROUP}, j.job_id, j.next_session::text, l.outcome,
                        COALESCE(e.message, COALESCE(l.failed_stage || ': ', '') || COALESCE(l.failure, 'no reason given'))
                 FROM core.job j JOIN core.lease l ON l.job_id = j.job_id
                 LEFT JOIN core.job_evidence e ON e.lease_id = l.lease_id
                 WHERE j.state IN ('QUEUED', 'LEASED') AND j.next_session IS NOT NULL AND core.build_runs(j.engine_build)
                   AND l.outcome IN ('EXPIRED', 'FAILED') AND l.start_session = j.next_session
                   AND l.ended_at <= $1::text::timestamptz
                   AND l.granted_at > $1::text::timestamptz - make_interval(secs => $2::float8 * 3600)
                   AND (j.retried_at IS NULL OR l.granted_at > j.retried_at)
                 ORDER BY 1, j.job_id, l.ended_at DESC, l.lease_id DESC"
            ),
            &[&now, &THRASH_WINDOW_HOURS],
        )
        .await
        .map_err(db)?;
    // per kind: (job, session, lost, last outcome, last error), each job's losses counted; a loss to
    // the provider's errors (an HTTP status the provider check counts) is PROVIDER_ERRORS's, one alert
    // for one cause, and not counted here
    let mut jobs: BTreeMap<String, Vec<(i64, String, i64, String, String)>> = BTreeMap::new();
    for r in &rows {
        let (kind, job, session, outcome, why): (String, i64, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        if provider_statuses(&why).iter().any(|c| is_provider_error(*c)) {
            continue;
        }
        let list = jobs.entry(kind).or_default();
        match list.last_mut() {
            Some(last) if last.0 == job => last.2 += 1,
            _ => list.push((job, session, 1, outcome, why)),
        }
    }
    for (kind, list) in jobs {
        let mut thrashing: Vec<&(i64, String, i64, String, String)> = list.iter().filter(|j| j.2 >= THRASH_LEASES).collect();
        // the job that lost most, then the oldest
        thrashing.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        let Some((job, session, n, outcome, why)) = thrashing.first().copied() else { continue };
        let why = if outcome == "EXPIRED" && why == "the lease was not renewed" {
            "its lease expired with no stop recorded (the pod killed: out of memory, its deadline, or its node lost)".to_owned()
        } else {
            one_line(why, 160)
        };
        let words = if kind == "DAILY_BLOCK" { "daily day blocks" } else { kind_words(&kind) };
        let mut title = words.to_owned();
        title[..1].make_ascii_uppercase();
        let others = match thrashing.len() {
            1 => String::new(),
            2 => "; 1 more job of the kind too".into(),
            k => format!("; {} more jobs of the kind too", k - 1),
        };
        found.push(Found {
            kind: "JOB_THRASHING",
            key: kind.clone(),
            summary: format!("{title} thrashing: job {job} ({session}) lost {n} leases without moving on, the last: {why}{others}"),
            command: format!("vyreon-core evidence list {job}"),
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

/// C-091: the problems a stall that may heal itself is; opened quietly, alerted only once they have held
/// `QUIET_MINUTES` (2026-11-26: a NO_PROGRESS alerted, then resolved itself 8 minutes later).
pub const QUIET_KINDS: [&str; 1] = ["NO_PROGRESS"];
pub const QUIET_MINUTES: f64 = 30.0;

/// C-091: the problems a deliberate stop of the backfill causes itself (`vyreon-core backfill stop`: its cancelled
/// passes leave days not derived, its days are left unchecked); neither opened nor resolved while it is stopped.
pub const STOP_SUPPRESSED: [&str; 2] = ["NOT_DERIVED", "REFCHECK_NOT_RUN"];

/// Whether the backfill is stopped (C-087), read in a savepoint: a database before 0064 (or a part of it renamed
/// away, a test's) reads as not stopped.
async fn backfill_stopped(c: &impl GenericClient) -> Result<bool, String> {
    c.batch_execute("SAVEPOINT backfill_flag").await.map_err(db)?;
    match c.query_one("SELECT backfill_stopped FROM core.queue_control", &[]).await {
        Ok(r) => {
            c.batch_execute("RELEASE SAVEPOINT backfill_flag").await.map_err(db)?;
            Ok(r.get(0))
        }
        Err(_) => {
            c.batch_execute("ROLLBACK TO SAVEPOINT backfill_flag; RELEASE SAVEPOINT backfill_flag").await.map_err(db)?;
            Ok(false)
        }
    }
}

/// Opens a problem in the caller's transaction (with its alert, but for a quiet one), unless it is open already;
/// not one the backfill's deliberate stop causes (C-091).
async fn open_one(c: &impl GenericClient, f: &Found, now: Option<&str>) -> Result<bool, String> {
    // a publish opens NOT_DERIVED in its own transaction: it waits for a digest being composed
    c.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    if STOP_SUPPRESSED.contains(&f.kind) && backfill_stopped(c).await? {
        return Ok(false);
    }
    let quiet = QUIET_KINDS.contains(&f.kind);
    let n = c
        .execute(
            "INSERT INTO core.problem (kind, key, opened_at, summary, command, detail, quiet)
             VALUES ($1, $2, COALESCE($3::text::timestamptz, now()), $4, $5, $6::text::jsonb, $7)
             ON CONFLICT (kind, key) WHERE resolved_at IS NULL DO NOTHING",
            &[&f.kind, &f.key, &now, &f.summary, &f.command, &f.detail.as_ref().map(Value::to_string), &quiet],
        )
        .await
        .map_err(db)?;
    if n == 1 && !quiet {
        notice::alert(c, &alert_for(f.kind, &f.summary, &f.command)).await?;
    }
    Ok(n == 1)
}

/// What a problem of `kind` means for the published numbers, in plain words (owner, 2026-11-24 and
/// 2026-11-25): whether it affects them (yes or no) and how; and, for the problems only the owner can act on
/// (`needs_you`), what to do, in plain words with no command (C-091).
pub fn consequence(kind: &str) -> (bool, &'static str, Option<&'static str>) {
    match kind {
        "SESSION_LATE" => (true, "that day shows as not published until its work finishes", None),
        "REPORT_AT_RISK" => (true, "Sunday's report may go out with this week incomplete", None),
        "QUEUE_PAUSED" => (false, "nothing new is computed while paused", None),
        "JOB_FAILED" => (true, "that day stays unpublished (readers see it missing, with its cause)", None),
        "PROVIDER_AUTH" => (false, "no new data can be fetched", Some("check the Intrinio key and the account")),
        "PROVIDER_ERRORS" => (false, "the work is retried by itself", None),
        "PROVIDER_THROTTLED" => (false, "requests slow down and are tried again by themselves; a capture that runs out of time is withheld with its cause", None),
        "DATABASE_FULL" => (false, "publishing stops when the volume is full", Some("approve a larger database volume (it costs money)")),
        "CALENDAR_END" => (false, "the nightly runs stop at the calendar's end", None),
        "NOT_DERIVED" => (true, "those days' history readings (unusualness, windows) are missing until derived again", None),
        "DAILY_STOPPED" => (false, "new sessions are not computed", None),
        "ARCHIVE_MISSING" => (false, "inputs kept for later re-checks may be lost", None),
        "REFCHECK_DISAGREES" => (true, "numbers may be wrong where the independent check disagrees; they stay published until judged", None),
        "REFCHECK_NOT_RUN" => (false, "those days are published but not independently checked", None),
        "STALLED" => (false, "nothing new is computed meanwhile", None),
        "CLAIMS_EMPTY" => (false, "work waits while workers start and leave", None),
        "WAL_ARCHIVING" | "BACKUP_STALE" => (false, "the database could not be restored to now", None),
        "WATCH_FAILING" => (false, "other problems may go unreported meanwhile", None),
        "JOB_THRASHING" => (false, "that work keeps restarting; its day stays unpublished until it finishes", None),
        "NO_PROGRESS" => (false, "that work is stuck; its day stays unpublished until it moves", None),
        "DAY_REFUSED" => (true, "that day stays unpublished (readers see it missing, with its cause); nothing more is tried on it", None),
        "STOP_RULE" => (true, "the owner's rule stops phase 1 for a fix and a restart, once the coordinator confirms it is the engine", None),
        "SPEND" => (false, "the spend nears or passes the owner's ceiling", Some("decide whether to go on: raise the ceiling, or stop the workers")),
        _ => (false, "", None),
    }
}

/// C-091 (owner, 2026-11-26): the problems only the owner can act on (money or the ceiling, a decision, the
/// provider's account): 🔴 "Needs you". Every other problem is ⚠️ "Problem", the coordinator's to handle.
pub fn needs_you(kind: &str) -> bool {
    consequence(kind).2.is_some()
}

/// An alert in plain words (C-091, owner 2026-11-26): 🔴 "Needs you: what: what to do" for the owner's own
/// actions; else ⚠️ "Problem: what", whether it affects the published numbers and how, and that the coordinator
/// is on it. No command (`command` stays the problem's, for `vyreon-core problems`).
pub fn alert_for(kind: &str, summary: &str, _command: &str) -> String {
    let (affects, effect, action) = consequence(kind);
    let summary = summary.trim_end_matches('.');
    if let Some(action) = action {
        return format!("🔴 Needs you: {summary}: {action}.");
    }
    let mut text = format!("⚠️ Problem: {summary}.");
    if !effect.is_empty() {
        text.push_str(&format!("\nPublished numbers {}: {effect}.", if affects { "affected" } else { "not affected" }));
    }
    text.push_str(" The coordinator is on it.");
    text
}

/// `alert_for` as the alerts channel carries it (`notice::alert`: `plain::for_alert`).
pub fn posted_alert(kind: &str, summary: &str, command: &str) -> String {
    crate::plain::for_alert(&alert_for(kind, summary, command))
}

/// `resolved_text` as the alerts channel carries it.
pub fn posted_resolved(summary: &str, age_seconds: i64) -> String {
    crate::plain::for_alert(&resolved_text(summary, age_seconds))
}

pub fn resolved_text(summary: &str, age_seconds: i64) -> String {
    format!("✅ Resolved: {summary} (it was open {})", age(age_seconds))
}

/// Finds the problems that hold at `now`, opens the new ones and resolves the ones that cleared,
/// with their posts, in one transaction.
pub async fn check(client: &mut Client, s: &Settings, now: &str) -> Result<Change, String> {
    check_queued(client, s, now, None).await
}

/// Resolves an open problem, with its post.
async fn resolve(c: &impl GenericClient, o: &Open, now: &str) -> Result<(), String> {
    c.execute("UPDATE core.problem SET resolved_at = GREATEST($2::text::timestamptz, opened_at) WHERE problem_id = $1", &[&o.id, &now]).await.map_err(db)?;
    notice::alert(c, &resolved_text(&o.summary, o.age)).await
}

/// `check`, told the error of this minute's queueing of the daily runs, if it failed (C-069).
async fn check_queued(client: &mut Client, s: &Settings, now: &str, queueing: Option<&str>) -> Result<Change, String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    let open = open(&tx, now).await?;
    let checked = current(&tx, s, now, &open, queueing).await?;
    // C-091: while the backfill is stopped by the owner, what the stop itself causes is neither opened nor resolved
    let stopped = backfill_stopped(&tx).await?;
    let found: Vec<Found> = checked.found.into_iter().filter(|f| !(stopped && STOP_SUPPRESSED.contains(&f.kind))).collect();
    let mut change = Change { failing: checked.failed.iter().map(|(p, e)| format!("{}: {e}", p.key)).collect(), ..Change::default() };
    // C-091: the quiet problems (a stall that may heal itself): said once they have held QUIET_MINUTES, resolved
    // without a word before
    let quiet = tx
        .query(
            "SELECT problem_id, kind, key, summary, command, extract(epoch FROM $1::text::timestamptz - opened_at)::float8 / 60
             FROM core.problem WHERE resolved_at IS NULL AND quiet",
            &[&now],
        )
        .await
        .map_err(db)?;
    for q in &quiet {
        let (id, kind, key, summary, command, minutes): (i64, String, String, String, String, f64) = (q.get(0), q.get(1), q.get(2), q.get(3), q.get(4), q.get(5));
        match found.iter().find(|f| f.kind == kind && f.key == key) {
            None => {
                tx.execute("UPDATE core.problem SET resolved_at = GREATEST($2::text::timestamptz, opened_at) WHERE problem_id = $1", &[&id, &now]).await.map_err(db)?;
            }
            Some(f) if minutes >= QUIET_MINUTES => {
                tx.execute("UPDATE core.problem SET quiet = false, summary = $2, command = $3 WHERE problem_id = $1", &[&id, &f.summary, &f.command]).await.map_err(db)?;
                notice::alert(&tx, &alert_for(f.kind, &f.summary, &f.command)).await?;
                change.opened.push(f.summary.clone());
            }
            Some(_) => {}
        }
        let _ = (summary, command);
    }
    let quiet: Vec<(String, String)> = quiet.iter().map(|q| (q.get(1), q.get(2))).collect();
    for f in &found {
        if quiet.iter().any(|(k, key)| k == f.kind && *key == f.key) {
            continue;
        }
        if !is_open(&open, f.kind, &f.key) && open_one(&tx, f, Some(now)).await? {
            if !QUIET_KINDS.contains(&f.kind) {
                change.opened.push(f.summary.clone());
            }
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
        // the tick's own parts are the tick's to resolve (`record`)
        if o.kind == WATCH_FAILING && TICK_PARTS.iter().any(|p| p.key == o.key) {
            continue;
        }
        if stopped && STOP_SUPPRESSED.contains(&o.kind.as_str()) {
            continue;
        }
        if !found.iter().any(|f| f.kind == o.kind && f.key == o.key) {
            resolve(&tx, o, now).await?;
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
/// C-090 as amended by C-094: and the measurements an engine fault withheld on every object that has a value to
/// lose (`whole_faults`: two objects or more faulted, and every other object withheld whole, as IBIT's empty chain
/// beside 62 faulted assets), the systematic breakage the stop rule stops for.
const COUNTED: &str = "SELECT d.revision_id, count(m.measurement_id)::int AS slots, (count(*) FILTER (WHERE m.whole))::int AS withheld,
                (count(*) FILTER (WHERE m.whole AND NOT m.fault))::int AS provider_withheld, (count(*) FILTER (WHERE m.fault))::int AS engine_faults,
                (SELECT COALESCE(array_agg(w.measurement_id ORDER BY w.measurement_id), '{}'::text[])
                 FROM (SELECT m.measurement_id FROM vyreon.measurement m WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date
                       GROUP BY m.measurement_id
                       HAVING count(DISTINCT m.object_id) FILTER (WHERE $FAULT) >= 2
                          AND bool_and($FAULT OR strpos(m.body_text, m.measurement_id || '/' || m.object_id || '/' || m.represented_date::text || '/engine-fault\"') > 0)) w) AS whole_faults
         FROM vyreon.day_revision d
         LEFT JOIN LATERAL (
             SELECT m.measurement_id,
                    strpos(m.body_text, m.measurement_id || '/' || m.object_id || '/' || d.represented_date::text || '/engine-fault\"') > 0 AS whole,
                    $FAULT AS fault
             FROM vyreon.measurement m WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date) m ON true
         WHERE d.published_at IS NOT NULL AND $WHERE
         GROUP BY d.revision_id";

/// (engine investigation, 2026-11-25) a measurement row `m` is engine-caused when its stated reason is:
/// the body's terminal reason, or a role's, one of `ENGINE_REASONS`. Not the words anywhere in the body (a
/// FIELD body lists other assets' reasons), and never the withheld slot's result id (every withheld slot's
/// ends `/engine-fault`, a provider's absence too: IBIT/ETHA's EMPTY_DELIVERED_POPULATION before their
/// options listed, PROVIDER_COVERAGE_START, CARRY_NOT_IDENTIFIED). Every other stated reason is the
/// provider's; core refuses no bundle for its reasons (they are the engine's to state).
/// Audit 2026-11-26 (C-094): never NULL (a healthy body with no terminal reason gave NULL, which `bool_and` ignores:
/// one faulted asset read as every asset's); and a direct owner's body states its reason on `failure_reason` and each
/// of its `coordinates` (M0-PRINT-SIZE: no terminal reason, no roles), read as well.
pub const FAULT: &str = "COALESCE(m.body->'measurement'->'value'->>'terminal_reason' IN ('ENGINE_FAULT', 'PARENT_ENGINE_FAULT', 'TAPE_LISTING_RESTATED')
     OR m.body->'measurement'->'value'->>'failure_reason' IN ('ENGINE_FAULT', 'PARENT_ENGINE_FAULT', 'TAPE_LISTING_RESTATED')
     OR EXISTS (SELECT FROM jsonb_array_elements(CASE WHEN jsonb_typeof(m.body->'measurement'->'value'->'roles') = 'array'
                                                     THEN m.body->'measurement'->'value'->'roles' ELSE '[]'::jsonb END
                                                 || CASE WHEN jsonb_typeof(m.body->'measurement'->'value'->'coordinates') = 'array'
                                                     THEN m.body->'measurement'->'value'->'coordinates' ELSE '[]'::jsonb END) r
                WHERE r->>'reason' IN ('ENGINE_FAULT', 'PARENT_ENGINE_FAULT', 'TAPE_LISTING_RESTATED')), false)";

/// The stated withholding reasons whose cause class is the engine (`FAULT` lists them): an engine fault,
/// a parent's, and (engine D-468) a tape session not derivable again under a moved provider listing record.
pub const ENGINE_REASONS: [&str; 3] = ["ENGINE_FAULT", "PARENT_ENGINE_FAULT", "TAPE_LISTING_RESTATED"];

/// C-071: the revisions one statement counts. A statement counts its revisions' bodies whole, so
/// its time grows with them; this bounds it (a day's history revision holds every slot of the 64).
pub const TALLY_BATCH: usize = 25;

/// C-071: how long one minute of the watch counts for the digest before it leaves the rest to the
/// next minute, well inside `/live`'s 15 minutes (`core_api::LIVE_WITHIN`).
pub const TALLY_BUDGET: Duration = Duration::from_secs(300);

/// C-071 (round 3, 1): every counting statement runs in its own transaction under these, well
/// inside `/live`'s 15 minutes: a batch that runs long or waits on a lock fails (and rolls back)
/// as the watch's failing part (C-069), instead of holding the tick until core is restarted.
pub const TALLY_LIMITS: &str = "SET LOCAL statement_timeout = '60s'; SET LOCAL lock_timeout = '10s'";

/// A transaction (on a transaction: a savepoint) under `TALLY_LIMITS`.
async fn bounded<C: GenericClient>(c: &mut C) -> Result<tokio_postgres::Transaction<'_>, String> {
    let tx = c.transaction().await.map_err(db)?;
    tx.batch_execute(TALLY_LIMITS).await.map_err(db)?;
    Ok(tx)
}

/// Counts what each published revision holds, once (a published revision never changes): one
/// statement, so at most `TALLY_BATCH` revisions (`tally_batches`).
async fn tally(c: &impl GenericClient, revisions: &[i64]) -> Result<(), String> {
    if revisions.is_empty() {
        return Ok(());
    }
    c.execute(
        &format!(
            "INSERT INTO core.revision_tally (revision_id, slots, withheld, provider_withheld, engine_faults, whole_faults) {}
             ON CONFLICT (revision_id) DO NOTHING",
            COUNTED.replace("$FAULT", FAULT).replace("$WHERE", "d.revision_id = ANY($1)")
        ),
        &[&revisions],
    )
    .await
    .map_err(db)?;
    Ok(())
}

/// C-071: counts `revisions` `TALLY_BATCH` at a time, each batch one statement under
/// `TALLY_LIMITS` committed on its own: what is counted stays counted when the process dies or is
/// restarted, a revision is counted once (`ON CONFLICT DO NOTHING`, a batch whole or not at all),
/// and the next call goes on from there. With `until`, it stops after the first batch that ends
/// past it. True when every revision is counted.
pub async fn tally_batches(c: &mut impl GenericClient, revisions: &[i64], until: Option<Instant>) -> Result<bool, String> {
    let batches = revisions.chunks(TALLY_BATCH).count();
    for (i, batch) in revisions.chunks(TALLY_BATCH).enumerate() {
        let tx = bounded(c).await?;
        tally(&tx, batch).await?;
        tx.commit().await.map_err(db)?;
        if i + 1 < batches && until.is_some_and(|t| Instant::now() >= t) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// C-071: counts every revision of the profile published by `now` and not counted yet (every
/// layer: base, day-local and the ordered passes' history revisions alike), oldest first,
/// `TALLY_BATCH` at a time: each batch chosen and counted in one transaction under
/// `TALLY_LIMITS`, committed on its own. Round 3, 3: what is left is what has no count, whenever
/// it was published, so a later minute never loses one to a moving 24-hour cutoff. With `until`,
/// it stops after the first batch that ends past it. True when none is left.
pub async fn tally_untallied(c: &mut impl GenericClient, profile: &str, now: &str, until: Option<Instant>) -> Result<bool, String> {
    loop {
        let tx = bounded(c).await?;
        let next: Vec<i64> = tx
            .query(
                "SELECT d.revision_id FROM vyreon.day_revision d WHERE d.profile_id = $2 AND d.published_at <= $1::text::timestamptz
                   AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)
                 ORDER BY d.revision_id LIMIT $3",
                &[&now, &profile, &(TALLY_BATCH as i64 + 1)],
            )
            .await
            .map_err(db)?
            .iter()
            .map(|r| r.get(0))
            .collect();
        let batch = &next[..next.len().min(TALLY_BATCH)];
        tally(&tx, batch).await?;
        tx.commit().await.map_err(db)?;
        if next.len() <= TALLY_BATCH {
            return Ok(true);
        }
        if until.is_some_and(|t| Instant::now() >= t) {
            return Ok(false);
        }
    }
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
///
/// C-071: each statement under `TALLY_LIMITS`; with `until` (the watch's minute), None when the
/// revisions left to count outlast it (round 3, 2: composing respects the minute's budget too).
pub async fn session_tallies(c: &mut impl GenericClient, profile: &str, sessions: &[String], until: Option<Instant>) -> Result<Option<BTreeMap<String, Tally>>, String> {
    // C-071: the revisions not counted yet, counted first in batches; the statement below then
    // counts only one published since (round x, 7 holds: one snapshot)
    let tx = bounded(c).await?;
    let uncounted: Vec<i64> = tx
        .query(
            "SELECT d.revision_id FROM vyreon.day_revision d
             WHERE d.profile_id = $1 AND d.state = 'PUBLISHED' AND d.represented_date = ANY($2::text[]::date[])
               AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)
             ORDER BY d.revision_id",
            &[&profile, &sessions],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    tx.commit().await.map_err(db)?;
    if !tally_batches(c, &uncounted, until).await? {
        return Ok(None);
    }
    // a session counts once its base revision is published; its derived revisions add their slots
    let tx = bounded(c).await?;
    let rows = tx
        .query(
            &format!(
                "WITH shown AS (SELECT d.revision_id, d.represented_date FROM vyreon.day_revision d
                            WHERE d.profile_id = $1 AND d.state = 'PUBLISHED' AND d.represented_date = ANY($2::text[]::date[])
                              AND EXISTS (SELECT FROM vyreon.day_revision b WHERE b.represented_date = d.represented_date AND b.profile_id = d.profile_id
                                            AND b.state = 'PUBLISHED' AND b.base_revision IS NULL)),
                 counted AS ({}),
                 kept AS (INSERT INTO core.revision_tally (revision_id, slots, withheld, provider_withheld, engine_faults, whole_faults)
                          SELECT * FROM counted ON CONFLICT (revision_id) DO NOTHING),
                 t AS (SELECT t.revision_id, t.slots, t.withheld, t.provider_withheld, t.engine_faults FROM core.revision_tally t
                       WHERE t.revision_id IN (SELECT revision_id FROM shown)
                       UNION ALL SELECT revision_id, slots, withheld, provider_withheld, engine_faults FROM counted)
                 SELECT s.represented_date::text, sum(t.slots)::bigint, sum(t.withheld)::bigint, sum(t.provider_withheld)::bigint, sum(t.engine_faults)::bigint
                 FROM shown s JOIN t USING (revision_id)
                 GROUP BY 1",
                COUNTED.replace("$FAULT", FAULT).replace(
                    "$WHERE",
                    "d.revision_id IN (SELECT revision_id FROM shown) AND NOT EXISTS (SELECT FROM core.revision_tally t WHERE t.revision_id = d.revision_id)"
                )
            ),
            &[&profile, &sessions],
        )
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Some(rows.iter().map(|r| (r.get(0), Tally { slots: r.get(1), withheld: r.get(2), provider_withheld: r.get(3), engine_faults: r.get(4) })).collect()))
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
        "REFCHECK" => "reference-check re-runs",
        "REPUBLISH" => "republishes",
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
const DAY: &str = "CASE WHEN extract(year FROM $D) = extract(year FROM ($1::text::timestamptz AT TIME ZONE '$Z'))
                        THEN to_char($D, 'Dy Mon FMDD') ELSE to_char($D, 'Dy Mon FMDD, YYYY') END";

fn day_of(expr: &str) -> String {
    DAY.replace("$D", expr).replace("$Z", ZONE)
}

/// The daily digest at `now`, from the problems as last checked.
pub async fn digest(c: &mut impl GenericClient, s: &Settings, now: &str) -> Result<String, String> {
    let open = open(&*c, now).await?;
    // with no budget, every revision is counted and the digest composed
    Ok(digest_of(c, s, now, &open, None).await?.unwrap_or_default())
}

/// C-091 (owner, 2026-11-26: "keep it concise"): the 09:00 digest as posted, three lines: the verdict (🔴 when
/// something needs the owner, ⚠️ when problems are open for the coordinator, else 🟢), the database's progress
/// (`progress::database_line`), and what needs the owner or "Nothing needs you". Everything else is behind
/// `vyreon-core digest` and `vyreon-core problems`.
pub async fn digest_post(c: &mut impl GenericClient, s: &Settings, now: &str, open: &[Open]) -> Result<String, String> {
    let today: String = c.query_one("SELECT to_char($1::text::timestamptz AT TIME ZONE $2, 'Dy Mon FMDD')", &[&now, &ZONE]).await.map_err(db)?.get(0);
    let mut needs: Vec<String> = open.iter().filter(|o| needs_you(&o.kind)).map(|o| one_line(&o.summary, 160)).collect();
    if let Some(w) = &s.spend
        && let crate::spend::Spend::Unread(why) = crate::spend::spend(&*c, w, now).await?
    {
        needs.push(format!("the spend is not known ({}), so the ceiling is not watched", one_line(&why, 120)));
    }
    let problems = open.iter().filter(|o| !needs_you(&o.kind)).count();
    let database = crate::progress::database_line(c, &s.profile, s.phase.as_ref(), now).await?;
    // C-097 (owner, 2026-11-26: "you shouldn't have to go looking"): the run's own lines after the three (what was
    // published, a usual day's time, the provider's refusals, the largest withholdings); none when they do not read
    let mut post = digest_lines(&today, &needs, problems, &database);
    for line in crate::runview::run_block(c, &s.profile, now).await {
        post.push('\n');
        post.push_str(&line);
    }
    Ok(post)
}

/// The three lines of `digest_post`.
pub fn digest_lines(today: &str, needs: &[String], problems: usize, database: &str) -> String {
    let verdict = match (needs.is_empty(), problems) {
        (false, _) => format!("🔴 Needs you · {today}"),
        (true, 0) => format!("🟢 Running fine · {today}"),
        (true, 1) => format!("⚠️ 1 problem open, the coordinator is on it · {today}"),
        (true, n) => format!("⚠️ {n} problems open, the coordinator is on them · {today}"),
    };
    let needs = if needs.is_empty() { "Nothing needs you".to_owned() } else { format!("Needs you: {}", needs.join("; ")) };
    format!("{verdict}\n{database}\n{needs}")
}

/// The digest at `now` for the problems `open`. C-071: None when its counting outlasts `until`
/// (the watch's minute), composed the next minute.
async fn digest_of(c: &mut impl GenericClient, s: &Settings, now: &str, open: &[Open], until: Option<Instant>) -> Result<Option<String>, String> {
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
    let Some(tallies) = session_tallies(c, &s.profile, &dates, until).await? else {
        return Ok(None);
    };
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

    // what was handled: provider gaps the engine or the workers stated, attempts retried. C-071:
    // the day's revisions counted in batches (the watch's digest has counted them already, a
    // minute at a time; only one published since is left)
    if !tally_untallied(c, &s.profile, now, until).await? {
        return Ok(None);
    }
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
             FROM core.job GROUP BY kind ORDER BY CASE kind WHEN 'DAY_BLOCK' THEN 0 WHEN 'TAPE' THEN 1 ELSE 2 END, kind",
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
                         FROM (SELECT to_timestamp($2::float8) AT TIME ZONE $3 AS t) x",
                        &[&now, &finish, &ZONE],
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

    lines.push(match s.volume_bytes.filter(|v| *v > 0) {
        Some(v) => {
            // C-068: the volume's use with the WAL; round 2: the backups only as the plugin records them
            let st = storage(c, now).await?;
            let used = st.data + st.wal.unwrap_or(0);
            let wal = st.wal.map_or_else(|| "WAL not read".to_owned(), |w| format!("WAL {}", gigabytes(w)));
            let backup = match s.backup_days {
                None => String::new(),
                Some(_) => format!(" · {}", backup_words(c, now).await?),
            };
            format!("Database size: {} of {} ({:.0}%) · {wal}{backup}", gigabytes(used), gigabytes(v), 100.0 * used as f64 / v as f64)
        }
        None => {
            let size: i64 = c.query_one("SELECT pg_database_size(current_database())", &[]).await.map_err(db)?.get(0);
            format!("Database size: {} (its volume's size is not set, so it is not checked)", gigabytes(size))
        }
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
    // C-097: the run's own lines, last (none when they do not read)
    lines.extend(crate::runview::run_block(c, &s.profile, now).await);
    Ok(Some(compose(&format!("{} · {today}", verdict(open, &handled, &unjudged)), &lines, open)))
}

/// The Saturday check at `now`: this week's sessions (Monday to the day before), each published
/// or not with its coverage and its history pass, for Sunday's report. Green only with every
/// watched session's base and history pass published (or due later) and no problem open.
pub async fn weekly(c: &mut impl GenericClient, s: &Settings, now: &str) -> Result<String, String> {
    let open = open(&*c, now).await?;
    // with no budget, every revision is counted and the check composed
    Ok(weekly_of(c, s, now, &open, None).await?.unwrap_or_default())
}

/// The Saturday check at `now` for the problems `open`; None as `digest_of`.
async fn weekly_of(c: &mut impl GenericClient, s: &Settings, now: &str, open: &[Open], until: Option<Instant>) -> Result<Option<String>, String> {
    let rows = c
        .query(
            &format!(
                "SELECT s.session::text, {}, $3::text IS NOT NULL AND s.session >= $3::text::date,
                        EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.profile_id = $2
                                AND d.state = 'PUBLISHED' AND d.base_revision IS NULL),
                        ((s.session + 1) + make_time($4, 0, 0)) AT TIME ZONE '{ZONE}' <= $1::text::timestamptz,
                        to_char(s.session + 1, 'Dy'),
                        EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.profile_id = $2
                                AND d.state = 'PUBLISHED' AND d.base_revision IS NOT NULL AND NOT d.day_local)
                 FROM core.session s,
                      (SELECT ($1::text::timestamptz AT TIME ZONE '{ZONE}')::date AS today) t
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
    let Some(tallies) = session_tallies(c, &s.profile, &dates, until).await? else {
        return Ok(None);
    };
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
    Ok(Some(compose(&first, &lines, open)))
}

/// One minute of the watch: the daily runs' due sessions queued (C-051), the check, then the
/// digest when it is due (09:00 in the owner's zone, once a day) and the weekly check (Saturday,
/// 09:00).
///
/// C-069: each part runs whatever another's outcome (the checks each in a savepoint of the check's
/// transaction), and a part that failed is a WATCH_FAILING problem with its error, on the channel
/// like any other; a failed queueing is said in DAILY_STOPPED. Ok once the check ran and every
/// failure is recorded; Err (the check could not run, or nothing could be recorded) leaves `/live`
/// to restart a core that stays so (`core_api::LIVE_WITHIN`).
pub async fn tick(client: &mut Client, s: &Settings, now: &str) -> Result<Change, String> {
    tick_within(client, s, now, TALLY_BUDGET).await
}

/// `tick`, its posts counting revisions for at most `budget` (C-071: `TALLY_BUDGET`; a test spends
/// it at once).
pub async fn tick_within(client: &mut Client, s: &Settings, now: &str, budget: Duration) -> Result<Change, String> {
    let queueing = match &s.daily {
        Some(d) => daily::queue_due(client, d, now).await.err(),
        None => None,
    };
    let (mut change, checked) = match check_queued(client, s, now, queueing.as_deref()).await {
        Ok(change) => (change, Ok(())),
        Err(e) => (Change::default(), Err(queueing.as_ref().map_or_else(|| e.clone(), |q| format!("{e}; the daily runs' queueing failed too: {q}")))),
    };
    // the check's own outcome first, so a digest of this minute says what holds after it
    record(client, now, &[(&CHECK, checked.clone())], &mut change).await?;
    // round w, 13: a check that could not run leaves the problem list stale; no post is composed from it
    if checked.is_ok() {
        let mut outcomes: Vec<(&Part, Result<(), String>)> = Vec::new();
        match due_posts(&*client, now).await {
            Err(e) => outcomes.push((&DIGEST, Err(e))),
            Ok((date, nine, saturday)) => {
                // C-071: the posts share one budget; a post left for the next minute is a part not
                // run this minute (round 3, 4: it neither opens nor resolves its problem)
                let until = Instant::now() + budget;
                // C-094 (audit 2026-11-26): the published revisions are counted every minute, not only with the
                // 09:00 digest: the one-real-day gate, the stop rule and the readings share read these counts
                // (the digest's own minute counts inside `post_once`, under the same budget; a count that fails
                // here is tried again next minute, and one that keeps failing fails the next digest, which says so)
                if !nine || notice::queued(&*client, &format!("digest {date}")).await.unwrap_or(false) {
                    if let Err(e) = tally_untallied(client, &s.profile, now, Some(until)).await {
                        eprintln!("vyreon-core watch: counting the published revisions failed (tried again next minute): {e}");
                    }
                }
                if nine {
                    outcomes.extend(ran(post_once(client, &format!("digest {date}"), s, now, false, until).await).map(|r| (&DIGEST, r)));
                    if saturday {
                        outcomes.extend(ran(post_once(client, &format!("weekly {date}"), s, now, true, until).await).map(|r| (&WEEKLY, r)));
                    }
                    // C-091: no spend line on the progress channel any more (the spend nearing the ceiling is
                    // SPEND, and a spend not known is in the digest's "Needs you")
                }
                // C-097: what the published revisions withhold is counted the same way (the newest first), after
                // the posts and only in what is left of the minute's budget, so it never delays a digest; a
                // failure is tried again next minute and stops nothing
                if Instant::now() < until
                    && let Err(e) = crate::runview::count_uncounted(client, &s.profile, now, Some(until)).await
                {
                    eprintln!("vyreon-core watch: counting what the published revisions withhold failed (tried again next minute): {e}");
                }
                // C-079: a new build's methods compared with the published days', once; C-091: not while the
                // backfill is stopped by the owner (a decision would queue blocks)
                let stopped = crate::backfill::stopped(&*client).await.unwrap_or(false);
                if let Some(r) = s.recompute.as_ref().filter(|_| !stopped) {
                    outcomes.push((&RECOMPUTE, crate::recompute::auto(client, &s.profile, r, s.spend.as_ref(), now).await.map(|_| ())));
                }
            }
        }
        record(client, now, &outcomes, &mut change).await?;
    }
    checked.map(|()| change)
}


/// A post's outcome as `record` takes it: None when it was left for the next minute.
fn ran(posted: Result<bool, String>) -> Option<Result<(), String>> {
    posted.map(|done| done.then_some(())).transpose()
}

/// The day in the owner's zone, whether the digest is due (09:00 or later) and whether it is Saturday.
async fn due_posts(c: &impl GenericClient, now: &str) -> Result<(String, bool, bool), String> {
    let row = c
        .query_one(
            "SELECT to_char(t, 'YYYY-MM-DD'), extract(hour FROM t) >= 9, extract(isodow FROM t) = 6
             FROM (SELECT $1::text::timestamptz AT TIME ZONE $2 AS t) x",
            &[&now, &ZONE],
        )
        .await
        .map_err(db)?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

/// The tick's own parts (C-069): one that failed is a WATCH_FAILING problem, opened with its alert
/// or restated with this minute's error; one that worked resolves its problem; one not run this
/// minute (the digest before 09:00, or left for the next minute: C-071) leaves its problem as it is.
async fn record(client: &mut Client, now: &str, outcomes: &[(&Part, Result<(), String>)], change: &mut Change) -> Result<(), String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    let open = open(&tx, now).await?;
    for (p, outcome) in outcomes {
        let was = open.iter().find(|o| o.kind == WATCH_FAILING && o.key == p.key);
        match (outcome, was) {
            (Err(e), None) => {
                let f = failing(p, e);
                if open_one(&tx, &f, Some(now)).await? {
                    change.opened.push(f.summary);
                }
            }
            (Err(e), Some(o)) => {
                let f = failing(p, e);
                if o.summary != f.summary {
                    tx.execute("UPDATE core.problem SET summary = $2 WHERE problem_id = $1", &[&o.id, &f.summary]).await.map_err(db)?;
                    change.restated.push(f.summary);
                }
            }
            (Ok(()), Some(o)) => {
                resolve(&tx, o, now).await?;
                change.resolved.push(o.summary.clone());
            }
            (Ok(()), None) => {}
        }
        if let Err(e) = outcome {
            change.failing.push(format!("{}: {e}", p.key));
        }
    }
    tx.commit().await.map_err(db)
}

/// Composes the digest (or the weekly check) and queues it once under `key`. It is composed
/// outside any lock (it counts measurement bodies, which can take long); the problems' lock is
/// then held only to read the open problems again and queue the post if they are the ones it was
/// composed from: a post saying what is open is never queued beside a problem opening or resolving
/// (round v, 7), and a publish opening a problem never waits for a digest's counting (round w, 4).
/// When they changed, nothing is queued and the next minute composes it again.
///
/// C-071: it first counts every revision not counted yet, in committed batches, until `until`;
/// composing then counts only what was published since, within the same budget. False when the
/// budget ran out first: nothing composed, the rest counted (and the post composed) next minute.
async fn post_once(client: &mut Client, key: &str, s: &Settings, now: &str, weekly_check: bool, until: Instant) -> Result<bool, String> {
    if notice::queued(&*client, key).await? {
        return Ok(true);
    }
    let read = open(&*client, now).await?;
    if !tally_untallied(client, &s.profile, now, Some(until)).await? {
        return Ok(false);
    }
    // C-091: the posted digest is three lines (`digest_post`); the full one is `vyreon-core digest`'s
    let composed = if weekly_check { weekly_of(client, s, now, &read, Some(until)).await? } else { Some(digest_post(client, s, now, &read).await?) };
    let Some(text) = composed else {
        return Ok(false);
    };
    let ids = |o: &[Open]| o.iter().map(|p| p.id).collect::<Vec<_>>();
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PROBLEM_LOCK, &[]).await.map_err(db)?;
    if ids(&open(&tx, now).await?) == ids(&read) {
        notice::alert_once(&tx, key, &text).await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(true)
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

    /// C-068 (audit opsurface ops.md 2): the volume of C-067 (250 GiB). A failing archive grows
    /// `pg_wal` while the data stays put: the volume fills with the data alone at 150 GiB (60%, no
    /// alert before) and 60 GiB of WAL (84%); archiving failing for 15 minutes or a WAL file waiting
    /// 30 is a problem, a single failure followed by a success is not; a login without `pg_monitor` is
    /// told, never read as healthy, and (round 2, 16) an open full volume stays open while the WAL
    /// cannot be read.
    #[test]
    fn a_failing_archive_is_a_problem_before_the_volume_fills() {
        const GIB: i64 = 1 << 30;
        let volume = 250 * GIB;
        let healthy = Storage { readable: true, data: 150 * GIB, wal: Some(GIB), archived_ago: Some(2.0), failed_ago: None, waiting: 0, oldest_waiting: None };
        assert_eq!(storage_found(&healthy, volume, false), []);
        let kinds = |st: &Storage| storage_found(st, volume, false).iter().map(|f| (f.kind, f.key.clone())).collect::<Vec<_>>();
        let k = |a: &'static str, b: &str| (a, b.to_owned());
        let failing = Storage { archived_ago: Some(20.0), failed_ago: Some(1.0), waiting: 4, oldest_waiting: Some(19.0), wal: Some(60 * GIB), ..healthy.clone() };
        assert_eq!(kinds(&failing), [k("DATABASE_FULL", "volume"), k("WAL_ARCHIVING", "archiver")]);
        assert_eq!(storage_found(&failing, volume, false)[0].summary, "Database volume 84% full: 210.0 GiB of 250.0 GiB, 60.0 GiB of it WAL");
        assert_eq!(kinds(&Storage { archived_ago: Some(1.0), failed_ago: Some(3.0), ..healthy.clone() }), []);
        assert_eq!(kinds(&Storage { archived_ago: Some(10.0), failed_ago: Some(1.0), ..healthy.clone() }), []);
        assert_eq!(kinds(&Storage { archived_ago: None, failed_ago: Some(1.0), ..healthy.clone() }), [k("WAL_ARCHIVING", "archiver")]);
        let stuck = Storage { waiting: 7, oldest_waiting: Some(30.0), ..healthy.clone() };
        assert_eq!(kinds(&stuck), [k("WAL_ARCHIVING", "archiver")]);
        assert!(storage_found(&stuck, volume, false)[0].summary.starts_with("WAL archiving to the backup bucket is stuck: last success 2 min ago, last failure never; 7 WAL files waiting"));
        // without pg_monitor: told; a full volume open with 100 GiB of data and 110 of WAL is not
        // resolved on the 100 alone
        let blind = Storage { readable: false, data: 100 * GIB, wal: None, ..healthy.clone() };
        assert_eq!(kinds(&blind), [k("WAL_ARCHIVING", "unreadable")]);
        let still = storage_found(&blind, volume, true);
        assert_eq!((still[0].kind, still[0].summary.as_str()), ("DATABASE_FULL", "Database volume: cannot be judged while the WAL is unreadable (100.0 GiB of data alone); it was full"));
        let nearly = Storage { data: 196 * GIB, wal: Some(0), ..healthy.clone() };
        assert_eq!(storage_found(&nearly, volume, false), [], "78.4%: not yet full");
        assert_eq!(storage_found(&nearly, volume, true).len(), 1, "78.4%: still full once open");
    }

    /// C-068 round 2 (`round2-core.md` 2, 3): the evidence is the plugin's own record, in the shapes
    /// of CloudNativePG 1.30's Cluster (`spec.plugins`) and plugin-barman-cloud v0.16.0's ObjectStore
    /// CRD (`status.serverRecoveryWindow`, read from its release manifest), named as this cluster's
    /// manifests name them (`10-postgres.yaml`, `15-backup.yaml`).
    #[test]
    fn the_backup_evidence_is_the_plugins_record() {
        let cluster = json!({"spec": {"plugins": [{"name": BARMAN_PLUGIN, "isWALArchiver": true, "parameters": {"barmanObjectName": "vyreon-backup"}}]}});
        let store = json!({"status": {"serverRecoveryWindow": {"vyreon-pg": {
            "firstRecoverabilityPoint": "2026-11-23T09:04:11Z", "lastSuccessfulBackupTime": "2026-11-23T09:04:11Z"}}}});
        let e = BackupEvidence::from_objects("vyreon-pg", &cluster, Some(&store));
        assert!(e.archiver);
        assert_eq!((e.object_store.as_deref(), e.server.as_deref(), e.last_successful_backup.as_deref()), (Some("vyreon-backup"), Some("vyreon-pg"), Some("2026-11-23T09:04:11Z")));
        // the plugin removed (CloudNativePG then reports every WAL file archived): no archiver
        let bare = BackupEvidence::from_objects("vyreon-pg", &json!({"spec": {}}), None);
        assert!(!bare.archiver && bare.last_successful_backup.is_none());
        // a recovered cluster archiving under its own server name reads its own window, not the old
        let pg2 = json!({"spec": {"plugins": [{"name": BARMAN_PLUGIN, "isWALArchiver": true, "parameters": {"barmanObjectName": "vyreon-backup", "serverName": "vyreon-pg2"}}]}});
        let e = BackupEvidence::from_objects("vyreon-pg2", &pg2, Some(&store));
        assert!(e.archiver && e.server.as_deref() == Some("vyreon-pg2") && e.first_recoverability_point.is_none());
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
