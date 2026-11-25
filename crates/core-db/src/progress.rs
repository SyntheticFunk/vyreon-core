//! The progress channel (C-065, owner 2026-11-22): what core logs and what the run did, every
//! `WINDOW_MINUTES` in one silent post for a second, optional Discord channel. The owner's alerts
//! channel (C-050) says whether to look; this one says what moved.
//!
//! C-073 (owner, 2026-11-24: readable at a glance), a post is:
//!   * a headline: a dot (🟢 fine, 🟡 something to watch: an engine fault, a retry, a check that
//!     disagrees, a failure; 🔴 an alert open), the backfill's days published of its sessions, the jobs
//!     building, the failures;
//!   * "+N this half hour:" and each day published, in one line: its date, the share of its readings
//!     with a value, its engine faults flagged ⚠, a reference check that disagrees or did not complete
//!     flagged, the time it took (provider absences are expected and not in the line);
//!   * the other news, each one short line: a retried or resumed block, a check of an earlier day that
//!     disagrees, every logged line but a block's routine end (a pause, a resume, an admission, a
//!     withdrawal shortened to what and why; a failure, a day set aside, a lost lease as logged); a
//!     hand-over's own notice is its one line;
//!   * the state in plain words: the history passes (done of all, and what the next waits for), the
//!     tapes while some are to do, the newest daily session.
//! Pods, job ids and zero counters stay behind `vyreon-core` commands. A half hour with nothing new is
//! the headline's one quiet line; with no work left and nothing new, no post. A window's post is queued
//! with the window, in one transaction, so a window is never posted twice nor lost (C-027's outbox).
use crate::notice;
use crate::watch::{self, LIMIT, ZONE, one_line};
use serde_json::Value;
use tokio_postgres::{Client, GenericClient};

/// A window's length (the owner's "every 30 minutes").
pub const WINDOW_MINUTES: f64 = 30.0;
/// At most this many posts a window; what does not fit is counted.
/// C-073: one post a window; what does not fit waits for the next window.
pub const MAX_POSTS: usize = 1;
/// A logged line is cut to this many characters.
pub const LINE_MAX: usize = 400;
/// Progress posts not delivered within this many hours are not sent (`notice::drop_stale_progress`);
/// an event committed this long after its own time is still stated by the next window (`compose`).
pub const STALE_HOURS: f64 = 6.0;
/// Windows, what they stated, and progress posts done with are kept this many days.
pub const KEEP_DAYS: f64 = 7.0;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// A window composed and queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub start: String,
    pub end: String,
    pub lines: usize,
    pub posts: usize,
}

/// "3,904".
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// A reference check's outcome in words, from its report's fields (`outcome`, `checked`, `failed`,
/// `unexplained`: `failed_unexplained`, null from a suite without rulings, C-056;
/// `reference_errors`; `why`). Reference errors (comparisons the reference suite itself could not
/// make) are unjudged, so they are stated beside the verdict, never folded into it (C-054
/// amended 2026-11-20; audit round 2 of core-watch, 10).
pub fn refcheck_words(r: &Value) -> String {
    let n = |k: &str| r[k].as_i64();
    match r["outcome"].as_str() {
        Some("COMPLETED") => {
            let (checked, failed) = (thousands(n("checked").unwrap_or(0)), n("failed").unwrap_or(0));
            let errors = n("reference_errors").filter(|e| *e > 0).map_or(String::new(), |e| format!("; {} reference errors not judged", thousands(e)));
            match n("unexplained") {
                _ if failed == 0 => format!("reference check agrees ({checked} comparisons{errors})"),
                Some(0) => format!("reference check agrees ({checked} comparisons; {} failures explained by rulings{errors})", thousands(failed)),
                Some(u) => format!("reference check DISAGREES: {} unexplained failures of {checked}{errors}", thousands(u)),
                None => format!("reference check: {} failures of {checked}, not triaged against rulings{errors}", thousands(failed)),
            }
        }
        Some(outcome) => {
            let why = r["why"].as_str().map_or(String::new(), |w| format!(": {}", one_line(w, 80)));
            format!("reference check did not complete ({}{why})", outcome.to_lowercase())
        }
        None => "reference check not in yet".into(),
    }
}

/// One published day's line (C-073): its date, the share of its slots with a value, its engine
/// faults flagged (a defect: never hidden), a reference check that disagrees or did not complete
/// flagged (one that agrees, or is not in yet, is not news), and the time its block took for it.
/// Provider absences are expected and not in the line (`vyreon-core refcheck show`, the reader API).
pub fn day_line(date: &str, t: watch::Tally, refcheck: Option<&Value>, minutes: Option<f64>) -> String {
    let share = if t.slots > 0 { 100.0 * (t.slots - t.withheld) as f64 / t.slots as f64 } else { 0.0 };
    let mut line = format!("{date} ✓ {} of readings", percent(share));
    if t.engine_faults > 0 {
        line.push_str(&format!(" · {} ⚠", counted(t.engine_faults, "engine fault", "engine faults")));
    }
    if let Some(flag) = refcheck.and_then(refcheck_flag) {
        line.push_str(&format!(" · {flag}"));
    }
    if let Some(m) = minutes {
        line.push_str(&format!(" · {}", duration(m)));
    }
    line
}

/// A reference check worth a line (C-073): one that disagrees or did not complete; None for one
/// that agrees or is not judged yet (the digest counts those).
pub fn refcheck_flag(r: &Value) -> Option<String> {
    match r["outcome"].as_str() {
        Some("COMPLETED") => r["unexplained"].as_i64().filter(|u| *u > 0).map(|u| format!("reference check DISAGREES ⚠ ({} unexplained)", thousands(u))),
        Some(outcome) => Some(format!("reference check did not complete ⚠ ({})", outcome.to_lowercase())),
        None => None,
    }
}

/// "99.1%", "100%", "0.7%".
pub fn percent(p: f64) -> String {
    let p = (p * 10.0).round() / 10.0;
    if p == p.trunc() { format!("{p:.0}%") } else { format!("{p:.1}%") }
}

/// "52 min", "5.3 h".
pub fn duration(minutes: f64) -> String {
    if minutes < 60.0 { format!("{minutes:.0} min") } else { format!("{:.1} h", minutes / 60.0) }
}

/// A hand-over's reason is cut to this many characters in its line (C-072).
pub const REASON_MAX: usize = 80;
/// The started blocks' line names at most this many blocks; the rest are counted.
pub const STARTED_MAX: usize = 24;

/// A build as a hand-over's line names it: its engine commit (before the `+` of its inventory), or
/// the whole name when the two builds share it.
fn build_words(build: &str, other: &str) -> String {
    let commit = |b: &str| b.split('+').next().unwrap_or(b).to_owned();
    if commit(build) == commit(other) { build.to_owned() } else { commit(build) }
}

fn counted(n: i64, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

/// A hand-over as the progress channel says it (C-072).
#[derive(Debug, Clone, Default)]
pub struct HandOver {
    pub id: i64,
    pub from: String,
    pub to: String,
    pub reason: String,
    /// Tapes, day blocks and ordered passes moved to the successor.
    pub moved: [i64; 3],
    /// C-073 (release audit, Codex 8, Gemini 9): the reference-check re-runs moved.
    pub refchecks: i64,
    /// C-074 (release-2 audit, Gemini 12): the republishes moved.
    pub republishes: i64,
    /// The old build's tape verify jobs cancelled, not moved.
    pub verify_cancelled: i64,
    /// The running jobs its withdrawal stopped (moved, to run again under the successor).
    pub restarted: i64,
    /// C-077: the running day blocks it let finish their day.
    pub draining: i64,
    /// Its successor has been admitted by the time the window is composed; when not, the line keeps
    /// the hand-over's instruction to admit it (audit c072, 2; never said of a successor admitted
    /// and then withdrawn, whose withdrawal is deliberate).
    pub admitted: bool,
}

impl HandOver {
    /// One line: its builds, what it moved, the running jobs restarted, its reason cut to
    /// `REASON_MAX`, then its window's steps.
    pub fn line(&self) -> String {
        let HandOver { id, from, to, reason, moved, refchecks, republishes, verify_cancelled, restarted, draining, admitted } = self;
        let (verify_cancelled, restarted) = (*verify_cancelled, *restarted);
            let parts: Vec<String> = [(moved[0], "tape", "tapes"), (moved[1], "block", "blocks"), (moved[2], "pass", "passes"), (*refchecks, "re-run", "re-runs"), (*republishes, "republish", "republishes")]
            .iter()
            .filter(|(n, ..)| *n > 0)
            .map(|(n, one, many)| counted(*n, one, many))
            .collect();
        let mut line = format!(
            "Hand-over {id}: {} → {}, {}",
            build_words(from, to),
            build_words(to, from),
            if parts.is_empty() { "nothing moved".to_string() } else { format!("{} moved", parts.join(" + ")) }
        );
        if restarted > 0 {
            line.push_str(&format!(", {} restarted", counted(restarted, "running job", "running jobs")));
        }
        if *draining > 0 {
            line.push_str(&format!(", {} first", counted(*draining, "running block finishing its day", "running blocks finishing their day")));
        }
        if verify_cancelled > 0 {
            line.push_str(&format!(", {} cancelled", counted(verify_cancelled, "tape verify job", "tape verify jobs")));
        }
        line.push_str(&format!(" — {}", one_line(reason, REASON_MAX)));
        if !admitted {
            line.push_str(&format!(" · {} not admitted yet", build_words(to, from)));
        }
        // C-073 (audit c072 r4): the whole line is checked as a logged line is (a build's name is put
        // in as it is; `build::build_name` refuses an odd one at registration, this holds for any)
        if crate::evidence::unredacted(&line).is_some() {
            return format!("Hand-over {id}: (its line is not shown: it may hold a credential; `vyreon-core log 100`)");
        }
        line
    }
}

/// "07-09" when the year is the line's, else the whole date.
fn day_words(date: &str, year: Option<&str>) -> String {
    match year {
        Some(y) if date.starts_with(y) && date.len() == 10 => date[5..].to_owned(),
        _ => date.to_owned(),
    }
}

/// One started block: (first session, last session, its attempt, the session it resumed from).
pub type Started = (String, String, i64, Option<String>);

/// The started blocks' line is cut into lines of at most this many characters (C-073, audit c072
/// r4: one long line of retries was cut by the post splitter, its retry dates lost).
pub const STARTED_LINE_MAX: usize = 600;

/// The day blocks started in a window (C-072), in as few lines as `STARTED_LINE_MAX` allows: "Started
/// 8 blocks (2024): 07-09–07-12, 07-15, …", then "Started, continued: …"; each line with the indexes
/// (into `blocks`) of the starts it states: every named one, and on the last line the plain starts
/// only counted. A post that cannot hold a line leaves its starts for the next window.
pub fn started_lines(blocks: &[Started]) -> Vec<(String, Vec<usize>)> {
    let text = started_line(blocks);
    let named = started_named(blocks);
    let (head, rest) = text.split_once(": ").unwrap_or((&text, ""));
    // the spans, in order, as `started_line` wrote them; the "… and N more" tail stays with the last
    let (spans, more) = match rest.rsplit_once(", … and ") {
        Some((s, m)) => (s, Some(format!("… and {m}"))),
        None => (rest, None),
    };
    let spans: Vec<&str> = split_spans(spans);
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    let mut line = format!("{head}: ");
    let mut idx: Vec<usize> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        if !idx.is_empty() && line.chars().count() + span.chars().count() + 2 > STARTED_LINE_MAX {
            out.push((line.trim_end_matches(", ").to_owned(), std::mem::take(&mut idx)));
            line = "Started, continued: ".to_owned();
        }
        line.push_str(span);
        line.push_str(", ");
        idx.push(named[i]);
    }
    if let Some(m) = more {
        line.push_str(&m);
    }
    // the plain starts counted, not named, go with the last line
    idx.extend((0..blocks.len()).filter(|i| !named.contains(i)));
    out.push((line.trim_end_matches(", ").to_owned(), idx));
    out
}

/// The spans of a started line, split at the ", " between spans (never inside a span's "(from …,
/// attempt …)").
fn split_spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 && bytes.get(i + 1) == Some(&b' ') => {
                spans.push(&text[start..i]);
                start = i + 2;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < text.len() {
        spans.push(&text[start..]);
    }
    spans
}

/// The indexes of the starts `started_line` names, in its order.
fn started_named(blocks: &[Started]) -> Vec<usize> {
    let notable = |(first, _, attempt, from): &Started| *attempt > 1 || from.as_deref().is_some_and(|f| f != first);
    let mut plain = 0;
    (0..blocks.len())
        .filter(|i| {
            if notable(&blocks[*i]) {
                return true;
            }
            plain += 1;
            plain <= STARTED_MAX
        })
        .collect()
}

/// The day blocks started in a window, in one line (C-072): "Started 8 blocks (2024): 07-09–07-12,
/// 07-15, …", a block resumed or retried saying so (a retry is worth a look); at most
/// `STARTED_MAX` plain starts named, the rest counted, and every resumed or retried one named
/// whatever its place (audit c072, 6: its lease is stated by this window only).
pub fn started_line(blocks: &[Started]) -> String {
    let years: Vec<&str> = blocks.iter().flat_map(|(f, l, ..)| [f.get(..4).unwrap_or(""), l.get(..4).unwrap_or("")]).collect();
    let year = years.first().filter(|y| years.iter().all(|x| x == *y)).copied();
    let notable = |(first, _, attempt, from): &&Started| *attempt > 1 || from.as_deref().is_some_and(|f| f != first);
    let mut plain = 0;
    let named: Vec<&Started> = blocks
        .iter()
        .filter(|b| {
            if notable(b) {
                return true;
            }
            plain += 1;
            plain <= STARTED_MAX
        })
        .collect();
    let spans: Vec<String> = named
        .iter()
        .map(|(first, last, attempt, from)| {
            let mut span = if first == last { day_words(first, year) } else { format!("{}–{}", day_words(first, year), day_words(last, year)) };
            let mut notes = Vec::new();
            if let Some(f) = from.as_deref().filter(|f| f != first) {
                notes.push(format!("from {}", day_words(f, year)));
            }
            if *attempt > 1 {
                notes.push(format!("attempt {attempt}"));
            }
            if !notes.is_empty() {
                span.push_str(&format!(" ({})", notes.join(", ")));
            }
            span
        })
        .collect();
    let more = blocks.len() - named.len();
    let more = if more > 0 { format!(", … and {more} more") } else { String::new() };
    let year = year.map_or(String::new(), |y| format!(" ({y})"));
    format!("Started {}{year}: {}{more}", counted(blocks.len() as i64, "block", "blocks"), spans.join(", "))
}

/// A logged line as a post holds it: one line (its own line breaks as " / "), cut to `LINE_MAX`,
/// and never one a credential pattern matches.
fn flat(text: &str) -> String {
    let joined = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" / ");
    one_line(&joined, LINE_MAX)
}

pub async fn lines(c: &mut impl GenericClient, profile: &str, start: &str, end: &str) -> Result<Vec<String>, String> {
    Ok(compose(c, profile, start, end).await?.lines.into_iter().map(|(l, _)| l).collect())
}

/// What a line states, as `progress_shown` records it: (source, id).
type Shown = Vec<(&'static str, i64)>;

/// The run's state at a window's end (C-073's headline).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Head {
    /// The backfill's days (sessions of its day blocks) and those published (base and day-local).
    pub total: i64,
    pub published: i64,
    /// Jobs running (live leases), the daily session's apart.
    pub building: i64,
    /// Jobs set aside (FAILED) of builds that run.
    pub failed: i64,
    /// Open problems (each one alerted on the owner's channel).
    pub alerts: i64,
    /// Work still to do (queued or running).
    pub unfinished: i64,
    /// Something to watch in the window: an engine fault, a retry, a check that disagrees.
    pub watch: bool,
}

impl Head {
    fn dot(&self) -> &'static str {
        if self.alerts > 0 {
            "🔴"
        } else if self.watch || self.failed > 0 {
            "🟡"
        } else {
            "🟢"
        }
    }

    fn failures(&self) -> String {
        if self.failed == 0 { "no failures".into() } else { format!("{} failed", thousands(self.failed)) }
    }

    /// "🟢 Phase 1: 4 of 597 days published (0.7%) · 9 building · no failures".
    pub fn line(&self, label: &str) -> String {
        let days = if self.total > 0 {
            format!("{} of {} days published ({}) · ", thousands(self.published), thousands(self.total), percent(100.0 * self.published as f64 / self.total as f64))
        } else {
            String::new()
        };
        format!("{} {label}: {days}{} building · {}", self.dot(), thousands(self.building), self.failures())
    }

    /// A window with nothing new: "🟢 Phase 1: 4/597 · 9 building · nothing new".
    pub fn quiet(&self, label: &str) -> String {
        let days = if self.total > 0 { format!("{}/{} · ", thousands(self.published), thousands(self.total)) } else { String::new() };
        let failed = if self.failed > 0 { format!(" · {} failed", thousands(self.failed)) } else { String::new() };
        format!("{} {label}: {days}{} building{failed} · nothing new", self.dot(), thousands(self.building))
    }
}

/// A window composed: its lines (each with what it states), the events it states with no line of
/// their own (routine: a plain block start, a block's end, a check that agrees), the status lines
/// (passes, the daily run, tapes), whether anything new happened, and the headline.
struct Composed {
    lines: Vec<(String, Shown)>,
    routine: Shown,
    news_days: usize,
    news: bool,
    head: Head,
}

/// A logged line as the progress channel says it (C-073): routine ones (a block's end: the days it
/// published are their own lines) none; a pause, a resume, an admission, a withdrawal shortened to
/// what happened and why; everything else as logged (a failure, a day set aside, a lost lease are
/// always named).
pub fn short_notice(message: &str) -> Option<String> {
    if message.starts_with("Day block ") && message.contains(" done") && message.contains(" blocks to go)") {
        return None;
    }
    let text = if let Some(rest) = message.strip_prefix("Queue paused: ") {
        format!("Queue paused: {}", rest.split(". No work is handed out").next().unwrap_or(rest))
    } else if message == "Queue resumed." {
        "Queue resumed".into()
    } else if let Some(rest) = message.strip_prefix("Engine build ").and_then(|r| r.strip_suffix(" admitted: its workers may claim work.")) {
        format!("Build {} admitted", rest.split('+').next().unwrap_or(rest))
    } else if let Some(rest) = message.strip_prefix("Engine build ") {
        match rest.split_once(" withdrawn: ") {
            Some((build, why)) => {
                let (why, stopped) = match why.split_once(" running jobs stopped") {
                    Some((head, _)) => match head.rsplit_once(". ") {
                        Some((reason, n)) => {
                            let n: i64 = n.trim().parse().unwrap_or(0);
                            (reason.to_owned(), format!(" ({} stopped)", counted(n, "running job", "running jobs")))
                        }
                        None => (head.to_owned(), String::new()),
                    },
                    None => (why.trim_end_matches('.').to_owned(), String::new()),
                };
                format!("Build {} withdrawn: {why}{stopped}", build.split('+').next().unwrap_or(build))
            }
            None => message.to_owned(),
        }
    } else {
        message.to_owned()
    };
    Some(flat(&text))
}

/// `lines`, and what they state. Each event (a day published, a report, a lease granted, a line
/// logged) is stamped inside the transaction that writes it, which may commit after a window that
/// covers its time was composed; the next window would then leave it out by its time (audit round
/// 2 of core-watch, 9). So a window states each event once (`core.progress_shown`), and takes
/// every one not stated yet from `STALE_HOURS` before its start (never before the first window's
/// start, nor after its own) to its end.
///
/// C-073 (audit c072 r4): each line carries what it states, so a line the window's post cannot hold
/// leaves its events to the next window (`tick` records only what its post carried).
async fn compose(c: &mut impl GenericClient, profile: &str, start: &str, end: &str) -> Result<Composed, String> {
    let mut lines: Vec<(String, Shown)> = Vec::new();
    let mut routine: Shown = Vec::new();
    let mut watch_flag = false;
    let from: String = c
        .query_one(
            "SELECT LEAST($1::text::timestamptz,
                          GREATEST($1::text::timestamptz - make_interval(secs => $2::float8 * 3600),
                                   COALESCE((SELECT min(window_start) FROM core.progress_window), $1::text::timestamptz)))::text",
            &[&start, &STALE_HOURS],
        )
        .await
        .map_err(db)?
        .get(0);
    let not_shown = |source: &str, id: &str| format!("NOT EXISTS (SELECT FROM core.progress_shown s WHERE s.source = '{source}' AND s.id = {id})");

    // each day published: its day-local revision (a block publishes the base, then the day-local
    // revision on it, C-031), with the lease that published it (`core.lease_revision`; none when
    // it was published by hand: no minutes), the minutes counted from that lease's previous
    // day-local revision or its grant
    let days = c
        .query(
            &format!(
                "WITH d AS (SELECT b.represented_date AS day, b.revision_id AS base, l.revision_id AS local, l.published_at,
                                   l.republish_scope AS local_scope, b.republish_scope AS base_scope
                            FROM vyreon.day_revision l JOIN vyreon.day_revision b ON b.revision_id = l.base_revision AND b.represented_date = l.represented_date
                            WHERE l.day_local AND l.profile_id = $3 AND l.published_at > $1::text::timestamptz AND l.published_at <= $2::text::timestamptz
                              AND {fresh})
                 SELECT to_char(d.day, 'Mon FMDD, YYYY'), d.base, d.local,
                        extract(epoch FROM d.published_at - GREATEST(x.granted_at,
                            (SELECT max(p.published_at) FROM core.lease_revision pr JOIN vyreon.day_revision p USING (revision_id)
                             WHERE pr.lease_id = x.lease_id AND p.day_local AND p.published_at < d.published_at)))::float8 / 60,
                        (SELECT jsonb_build_object('id', r.refcheck_report_id, 'outcome', r.outcome, 'unexplained', r.failed_unexplained)
                         FROM core.refcheck_report r
                         WHERE r.base_revision = d.base AND (r.day_local_revision IS NULL OR r.day_local_revision = d.local)
                           AND r.stored_at <= $2::text::timestamptz
                         ORDER BY r.stored_at DESC, r.refcheck_report_id DESC LIMIT 1)::text,
                        -- C-074: a republish's scopes (the day-local revision's, and its base's when the job republished it)
                        d.local_scope::text, CASE WHEN d.base_scope->>'job' = d.local_scope->>'job' THEN d.base_scope::text END
                 FROM d LEFT JOIN LATERAL (
                     SELECT l.lease_id, l.granted_at FROM core.lease_revision lr JOIN core.lease l USING (lease_id) JOIN core.job j USING (job_id)
                     WHERE lr.revision_id = d.local AND j.kind = 'DAY_BLOCK') x ON true
                 ORDER BY d.published_at, d.day",
                fresh = not_shown("revision", "l.revision_id")
            ),
            &[&from, &end, &profile],
        )
        .await
        .map_err(db)?;
    // the reports a day line already states (C-054: a report is of the base and the day-local
    // revision it checked, both: audit round 2 of core-watch, 2)
    let mut stated: Vec<i64> = Vec::new();
    if !days.is_empty() {
        let revisions: Vec<i64> = days.iter().flat_map(|r| [r.get::<_, i64>(1), r.get::<_, i64>(2)]).collect();
        watch::tally_batches(c, &revisions, None).await?;
        for r in &days {
            let (date, base, local): (String, i64, i64) = (r.get(0), r.get(1), r.get(2));
            let t = c
                .query_one(
                    "SELECT COALESCE(sum(slots), 0)::bigint, COALESCE(sum(withheld), 0)::bigint, COALESCE(sum(provider_withheld), 0)::bigint, COALESCE(sum(engine_faults), 0)::bigint
                     FROM core.revision_tally WHERE revision_id = ANY($1)",
                    &[&vec![base, local]],
                )
                .await
                .map_err(db)?;
            let tally = watch::Tally { slots: t.get(0), withheld: t.get(1), provider_withheld: t.get(2), engine_faults: t.get(3) };
            let minutes: Option<f64> = r.get(3);
            let check: Option<Value> = r.get::<_, Option<String>>(4).and_then(|s| serde_json::from_str(&s).ok());
            let check_id = check.as_ref().and_then(|c| c["id"].as_i64());
            stated.extend(check_id);
            watch_flag |= tally.engine_faults > 0 || check.as_ref().and_then(refcheck_flag).is_some();
            let mut said: Shown = vec![("revision", local)];
            said.extend(check_id.map(|id| ("refcheck", id)));
            let line = day_line(&date, tally, check.as_ref(), minutes);
            let scopes: Vec<Value> = [r.get::<_, Option<String>>(5), r.get::<_, Option<String>>(6)].into_iter().flatten().filter_map(|t| serde_json::from_str(&t).ok()).collect();
            lines.push((if scopes.is_empty() { line } else { republished_line(&line, &scopes) }, said));
        }
    }
    let news_days = lines.len();

    // reference checks that came in for days published before: only one that disagrees or did not
    // complete is a line; one that agrees is stated without a line (the digest counts them)
    let checks = c
        .query(
            &format!(
                "SELECT to_char(r.session, 'Mon FMDD, YYYY'), jsonb_build_object('outcome', r.outcome, 'unexplained', r.failed_unexplained)::text,
                        b.state = 'PUBLISHED' AND (r.day_local_revision IS NULL
                            OR EXISTS (SELECT FROM vyreon.day_revision x WHERE x.revision_id = r.day_local_revision AND x.state = 'PUBLISHED')),
                        r.refcheck_report_id
                 FROM core.refcheck_report r JOIN vyreon.day_revision b ON b.revision_id = r.base_revision
                 WHERE b.profile_id = $3 AND r.stored_at > $1::text::timestamptz AND r.stored_at <= $2::text::timestamptz AND NOT r.refcheck_report_id = ANY($4)
                   AND {fresh}
                 ORDER BY r.stored_at, r.refcheck_report_id",
                fresh = not_shown("refcheck", "r.refcheck_report_id")
            ),
            &[&from, &end, &profile, &stated],
        )
        .await
        .map_err(db)?;
    for r in &checks {
        let (date, check, current, id): (String, String, bool, i64) = (r.get(0), r.get(1), r.get(2), r.get(3));
        let check: Value = serde_json::from_str(&check).map_err(|e| e.to_string())?;
        match refcheck_flag(&check).filter(|_| current) {
            Some(flag) => {
                watch_flag = true;
                lines.push((format!("{date}: {flag}"), vec![("refcheck", id)]));
            }
            None => routine.push(("refcheck", id)),
        }
    }

    // day blocks started: a plain start is the headline's "building"; a retried or resumed one is
    // named (a retry is worth a look)
    let started = c
        .query(
            &format!(
                "SELECT COALESCE(l.job_first, j.first_session)::text, COALESCE(l.job_last, j.last_session)::text, l.start_session::text,
                        (SELECT count(*) FROM core.lease p WHERE p.job_id = l.job_id AND p.lease_id <= l.lease_id), l.lease_id
                 FROM core.lease l JOIN core.job j USING (job_id)
                 WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $3 AND l.granted_at > $1::text::timestamptz AND l.granted_at <= $2::text::timestamptz
                   AND {fresh}
                 ORDER BY l.granted_at, l.lease_id",
                fresh = not_shown("lease", "l.lease_id")
            ),
            &[&from, &end, &profile],
        )
        .await
        .map_err(db)?;
    let mut notable: Vec<(Started, i64)> = Vec::new();
    for r in &started {
        let (first, last, from_day, attempt, lease): (String, String, Option<String>, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        if attempt > 1 || from_day.as_deref().is_some_and(|f| f != first) {
            notable.push(((first, last, attempt, from_day), lease));
        } else {
            routine.push(("lease", lease));
        }
    }
    if !notable.is_empty() {
        watch_flag = true;
        let blocks: Vec<Started> = notable.iter().map(|(b, _)| b.clone()).collect();
        for (line, of) in started_lines(&blocks) {
            let line = line.replacen("Started ", "Restarted ", 1);
            lines.push((line, of.iter().map(|i| ("lease", notable[*i].1)).collect()));
        }
    }

    // everything core logged in the window, shortened (C-073); a hand-over's own notice is its one line
    let logged = c
        .query(
            &format!(
                "SELECT to_char(n.created_at AT TIME ZONE $3, 'HH24:MI'), n.message, n.notice_id FROM core.notice n
                 WHERE NOT n.post AND n.created_at > $1::text::timestamptz AND n.created_at <= $2::text::timestamptz AND {fresh} ORDER BY n.notice_id",
                fresh = not_shown("notice", "n.notice_id")
            ),
            &[&from, &end, &ZONE],
        )
        .await
        .map_err(db)?;
    // the hand-overs a logged notice is (`Hand-over <id>: `), whatever their `handed_at`: a hand-over's
    // notice is stamped at its transaction's start, its row later, so the two can fall either side of a
    // window's end (C-073, audit c072 r4)
    let named: Vec<i64> = logged
        .iter()
        .filter_map(|r| r.get::<_, &str>(1).strip_prefix("Hand-over ")?.split_once(": ")?.0.parse::<i64>().ok())
        .collect();

    // the hand-overs of the window (C-072), each in one line: what it moved, the running jobs its own
    // withdrawal stopped (carried to the successor, to run again). Stated once: a hand-over whose own
    // notice a window stated before (a core before 0054 recorded only the notice) is stated (C-073,
    // audit c072 r4: posted again on the upgrade)
    let hand_overs = c
        .query(
            &format!(
                "SELECT h.hand_over_id, h.from_build, h.to_build, h.reason,
                        count(*) FILTER (WHERE j.kind = 'TAPE' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE j.kind = 'DAY_BLOCK' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE j.kind = 'ORDERED_PASS' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE x.old_job IS NOT NULL AND x.new_job IS NULL),
                        count(*) FILTER (WHERE j.kind = 'REFCHECK' AND x.new_job IS NOT NULL),
                        CASE WHEN bool_and(x.restarted IS NOT NULL OR x.old_job IS NULL) THEN count(*) FILTER (WHERE x.restarted AND x.new_job IS NOT NULL)
                             ELSE (SELECT count(DISTINCT l.job_id) FROM core.hand_over_job y JOIN core.lease l ON l.job_id = y.old_job
                                   WHERE y.hand_over_id = h.hand_over_id AND y.new_job IS NOT NULL
                                     AND l.outcome = 'CANCELLED' AND l.failure = 'its engine build was withdrawn') END,
                        (SELECT b.admitted_at IS NOT NULL AND (b.withdrawn_at IS NULL OR b.withdrawn_at > h.handed_at)
                         FROM core.engine_build b WHERE b.engine_build = h.to_build),
                        count(*) FILTER (WHERE j.kind = 'REPUBLISH' AND x.new_job IS NOT NULL),
                        (SELECT count(*) FROM core.drain dr WHERE dr.hand_over_id = h.hand_over_id)
                 FROM core.hand_over h LEFT JOIN core.hand_over_job x USING (hand_over_id) LEFT JOIN core.job j ON j.job_id = x.old_job
                 WHERE ((h.handed_at > $1::text::timestamptz AND h.handed_at <= $2::text::timestamptz) OR h.hand_over_id = ANY($3))
                   AND {fresh}
                   AND NOT EXISTS (SELECT FROM core.progress_shown s JOIN core.notice n ON s.source = 'notice' AND n.notice_id = s.id
                                   WHERE n.message LIKE 'Hand-over ' || h.hand_over_id || ': %')
                 GROUP BY h.hand_over_id ORDER BY h.handed_at, h.hand_over_id",
                fresh = not_shown("hand_over", "h.hand_over_id")
            ),
            &[&from, &end, &named],
        )
        .await
        .map_err(db)?;
    let hand_overs: Vec<HandOver> = hand_overs
        .iter()
        .map(|r| HandOver {
            id: r.get(0),
            from: r.get(1),
            to: r.get(2),
            reason: r.get(3),
            moved: [r.get(4), r.get(5), r.get(6)],
            verify_cancelled: r.get(7),
            refchecks: r.get(8),
            republishes: r.get(11),
            draining: r.get(12),
            restarted: r.get(9),
            admitted: r.get(10),
        })
        .collect();

    let mut said = vec![false; hand_overs.len()];
    for r in &logged {
        let (at, message, id): (String, String, i64) = (r.get(0), r.get(1), r.get(2));
        match hand_overs.iter().position(|h| message.starts_with(&format!("Hand-over {}: ", h.id))) {
            Some(i) => {
                said[i] = true;
                lines.push((format!("{at} {}", hand_overs[i].line()), vec![("notice", id), ("hand_over", hand_overs[i].id)]));
            }
            None => match short_notice(&message) {
                Some(text) => lines.push((format!("{at} {text}"), vec![("notice", id)])),
                None => routine.push(("notice", id)),
            },
        }
    }
    // a hand-over whose own notice was stated before (or is not in yet) still has its line
    for (h, _) in hand_overs.iter().zip(&said).filter(|(_, said)| !**said) {
        lines.push((h.line(), vec![("hand_over", h.id)]));
    }
    let mut news = !lines.is_empty();

    // the state: the headline's numbers, and the passes, the daily run and the tapes in plain words
    let h = c
        .query_one(
            "WITH s AS (SELECT DISTINCT x.session FROM core.job j JOIN core.session x ON x.session BETWEEN j.first_session AND j.last_session
                        WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $1 AND j.priority = 0 AND j.state <> 'CANCELLED')
             SELECT (SELECT count(*) FROM s),
                    (SELECT count(*) FROM s WHERE EXISTS (
                        SELECT FROM vyreon.day_revision b WHERE b.represented_date = s.session AND b.profile_id = $1 AND b.state = 'PUBLISHED' AND b.base_revision IS NULL
                          AND EXISTS (SELECT FROM vyreon.day_revision l WHERE l.base_revision = b.revision_id AND l.day_local AND l.state = 'PUBLISHED'))),
                    (SELECT count(*) FROM core.lease l JOIN core.job j USING (job_id)
                     WHERE l.ended_at IS NULL AND l.expires_at >= $2::text::timestamptz AND j.profile_id = $1 AND j.priority = 0),
                    (SELECT count(*) FROM core.job j WHERE j.state = 'FAILED' AND j.profile_id = $1 AND j.kind <> 'REFCHECK' AND core.build_runs(j.engine_build)),
                    (SELECT count(*) FROM core.problem WHERE resolved_at IS NULL),
                    (SELECT count(*) FROM core.job j WHERE j.state IN ('QUEUED', 'LEASED') AND j.profile_id = $1 AND core.build_runs(j.engine_build))",
            &[&profile, &end],
        )
        .await
        .map_err(db)?;
    let head = Head { total: h.get(0), published: h.get(1), building: h.get(2), failed: h.get(3), alerts: h.get(4), unfinished: h.get(5), watch: watch_flag };

    // the ordered passes: done of all, and why the next one waits (they run in date order, C-046)
    let p = c
        .query_one(
            "WITH p AS (SELECT * FROM core.job j WHERE j.kind = 'ORDERED_PASS' AND j.profile_id = $1 AND j.state <> 'CANCELLED' AND core.build_runs(j.engine_build))
             SELECT (SELECT count(*) FROM p), (SELECT count(*) FROM p WHERE state = 'DONE'),
                    (SELECT to_char(min(first_session), 'Mon FMDD') FROM p WHERE state = 'LEASED'),
                    (SELECT to_char(min(first_session), 'Mon FMDD') FROM p WHERE state IN ('QUEUED', 'FAILED')),
                    (SELECT min(first_session) FROM p WHERE state IN ('QUEUED', 'FAILED')) IN (SELECT first_session FROM core.claimable_job WHERE kind = 'ORDERED_PASS'),
                    (SELECT count(*) FROM core.lease l JOIN p USING (job_id) WHERE l.outcome = 'DONE' AND l.ended_at > $2::text::timestamptz AND l.ended_at <= $3::text::timestamptz)",
            &[&profile, &start, &end],
        )
        .await
        .map_err(db)?;
    let (all, done, running, next, claimable, done_now): (i64, i64, Option<String>, Option<String>, Option<bool>, i64) = (p.get(0), p.get(1), p.get(2), p.get(3), p.get(4), p.get(5));
    news |= done_now > 0;
    let mut status = Vec::new();
    if running.is_some() || next.is_some() {
        let now = if done_now > 0 { format!(" (+{})", thousands(done_now)) } else { String::new() };
        let what = match (running, next, claimable) {
            (Some(day), ..) => format!("computing {day}"),
            (None, Some(day), Some(true)) => format!("next {day}"),
            (None, Some(day), _) => format!("waiting for {day} (they run in date order)"),
            (None, None, _) => String::new(),
        };
        status.push(format!("History passes: {} of {} done{now} · {what}", thousands(done), thousands(all)));
    }
    // C-077: the old build's blocks finishing their day after a release: their slots are why the new
    // build's blocks have not started (not a stall: the watch's STALLED waits for them)
    let d = c
        .query_one(
            "SELECT count(*), string_agg(DISTINCT split_part(j.engine_build, '+', 1), ', '),
                    to_char(min(d.until) FILTER (WHERE d.until >= $3::text::timestamptz) AT TIME ZONE $2, 'HH24:MI'),
                    count(*) FILTER (WHERE j.next_session IS NULL), count(*) FILTER (WHERE d.until < $3::text::timestamptz)
             FROM core.drain d JOIN core.lease l USING (lease_id) JOIN core.job j ON j.job_id = d.job_id
             WHERE l.ended_at IS NULL AND j.profile_id = $1",
            &[&profile, &ZONE, &end],
        )
        .await
        .map_err(db)?;
    if d.get::<_, i64>(0) > 0 {
        let (n, builds, until, checking, overdue): (i64, Option<String>, Option<String>, i64, i64) = (d.get(0), d.get(1), d.get(2), d.get(3), d.get(4));
        let check = if checking > 0 { format!(" ({checking} with the day done, its check to come)") } else { String::new() };
        // (release-4 audit, Codex 6) the earliest bound to come, and any drain already past its own
        let bound = match (until, overdue) {
            (Some(u), 0) => format!(", the first bound {u} MT"),
            (Some(u), o) => format!(", {o} PAST their bound (core stops them), the next bound {u} MT"),
            (None, o) => format!(", {o} PAST their bound (core stops them)"),
        };
        status.push(format!(
            "Draining: {} of build {} finishing their day{check}{bound}; the new build's blocks start as they end",
            counted(n, "block", "blocks"),
            builds.unwrap_or_default()
        ));
        // (release-4 audit, Codex 5) said every window while they drain: a quiet half hour keeps it
        news = true;
    }
    // the tapes, while some are still to do
    let t = c
        .query_one(
            "SELECT count(*), count(*) FILTER (WHERE state = 'DONE'), count(*) FILTER (WHERE state IN ('QUEUED', 'LEASED')) FROM core.job j
             WHERE j.kind = 'TAPE' AND j.profile_id = $1 AND j.state <> 'CANCELLED' AND core.build_runs(j.engine_build)
               AND NOT COALESCE(j.scope @> '{\"verify\": true}', false) AND j.priority = 0",
            &[&profile],
        )
        .await
        .map_err(db)?;
    if t.get::<_, i64>(2) > 0 {
        status.push(format!("Tapes: {} of {} done", thousands(t.get(1)), thousands(t.get(0))));
    }
    // the newest daily session: computing, waiting, failed, or published today
    if let Some(d) = c
        .query_opt(
            "SELECT j.first_session::text, j.state,
                    (SELECT to_char(max(l.granted_at) AT TIME ZONE $3, 'HH24:MI') FROM core.lease l WHERE l.job_id = j.job_id AND l.ended_at IS NULL),
                    EXISTS (SELECT FROM core.job t WHERE t.kind = 'TAPE' AND t.priority = 1 AND t.first_session = j.first_session AND t.state IN ('QUEUED', 'LEASED')),
                    (SELECT to_char(max(l.published_at) AT TIME ZONE $3, 'HH24:MI') FROM vyreon.day_revision l
                     WHERE l.represented_date = j.first_session AND l.profile_id = $1 AND l.day_local AND l.state = 'PUBLISHED'),
                    j.finished_at > $2::text::timestamptz - interval '24 hours'
             FROM core.job j WHERE j.kind = 'DAY_BLOCK' AND j.priority = 1 AND j.profile_id = $1 AND j.state <> 'CANCELLED'
             ORDER BY j.first_session DESC LIMIT 1",
            &[&profile, &end, &ZONE],
        )
        .await
        .map_err(db)?
    {
        let (day, state, since, tape, published, recent): (String, String, Option<String>, bool, Option<String>, Option<bool>) = (d.get(0), d.get(1), d.get(2), d.get(3), d.get(4), d.get(5));
        let line = match state.as_str() {
            "LEASED" => Some(format!("Daily {day}: computing since {}", since.unwrap_or_else(|| "?".into()))),
            "QUEUED" if tape => Some(format!("Daily {day}: queued, waiting for its tapes")),
            "QUEUED" => Some(format!("Daily {day}: queued")),
            "FAILED" => Some(format!("Daily {day}: failed ⚠ (the alert says why)")),
            "DONE" if recent == Some(true) => Some(format!("Daily {day}: published{} ✓", published.map_or(String::new(), |p| format!(" {p}")))),
            _ => None,
        };
        status.extend(line);
    }
    lines.extend(status.into_iter().map(|l| (l, Vec::new())));
    Ok(Composed { lines, routine, news_days, news, head })
}

/// A window's post (C-073): its headline, then "+N this half hour:" and the days, the other news and
/// the state, each indented; with nothing new, the headline's one quiet line; nothing at all when no
/// work is left and nothing happened. Returns the post and how many lines it carried.
fn render(composed: &Composed, label: &str) -> (Option<String>, usize) {
    if !composed.news {
        if composed.head.unfinished == 0 && composed.head.alerts == 0 {
            return (None, 0);
        }
        return (Some(composed.head.quiet(label)), 0);
    }
    let mut lines: Vec<String> = Vec::new();
    if composed.news_days > 0 {
        lines.push(format!("+{} this half hour:", thousands(composed.news_days as i64)));
    }
    lines.extend(composed.lines.iter().map(|(l, _)| l.clone()));
    let indented: Vec<String> = lines.iter().map(|l| format!("   {l}")).collect();
    let (posts, carried) = split_counted(&composed.head.line(label), &indented);
    let carried = if composed.news_days > 0 { carried.saturating_sub(1) } else { carried };
    (posts.into_iter().next(), carried)
}

/// `text` cut to at most `max` characters, the characters left out counted at its end ("… (N
/// characters cut)"), never cut silently; as it is when it fits.
pub fn fit(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_owned();
    }
    let suffix = |cut: usize| format!("… ({} characters cut)", thousands(cut as i64));
    // the count's own length depends on how much is cut: settle it in two steps (the second
    // cuts no more than the first, so its count is no longer)
    let first = max.saturating_sub(suffix(n).chars().count());
    let keep = max.saturating_sub(suffix(n - first).chars().count());
    text.chars().take(keep).collect::<String>() + &suffix(n - keep)
}

/// The window's posts: the header, then the lines, each post at most `LIMIT` characters, at most
/// `MAX_POSTS`; the lines that do not fit are counted on the last. A post always carries a line
/// (never the header alone), and a line too long for a post of its own is cut to fit, the cut
/// counted (audit round 2 of core-watch, 14).
pub fn split(header: &str, lines: &[String]) -> Vec<String> {
    split_counted(header, lines).0
}

/// `split`, and how many of `lines` (from the first) its posts carry; the rest are counted on the
/// last post, and their events wait for the next window (`tick`).
pub fn split_counted(header: &str, lines: &[String]) -> (Vec<String>, usize) {
    let len = |s: &str| s.chars().count();
    let tail = |n: usize| format!("\n   … and {n} more (in the next post; `vyreon-core log 100`)");
    let mut posts: Vec<String> = Vec::new();
    let mut post = header.to_string();
    let mut has_lines = false;
    for (i, line) in lines.iter().enumerate() {
        let rest = lines.len() - i - 1;
        let room = |posts: &Vec<String>| if posts.len() + 1 == MAX_POSTS && rest > 0 { len(&tail(rest)) } else { 0 };
        if has_lines && len(&post) + 1 + len(line) + room(&posts) > LIMIT {
            if posts.len() + 1 == MAX_POSTS {
                post.push_str(&tail(rest + 1));
                posts.push(post);
                return (posts, i);
            }
            posts.push(post);
            post = format!("{header} (continued)");
        }
        let line = fit(line, LIMIT.saturating_sub(len(&post) + 1 + room(&posts)));
        post.push('\n');
        post.push_str(&line);
        has_lines = true;
    }
    posts.push(post);
    (posts, lines.len())
}

/// The headline of the progress channel when no label is set (`VYREON_CORE_PROGRESS_LABEL`).
pub const DEFAULT_LABEL: &str = "Backfill";

/// The post of the last `WINDOW_MINUTES` before `now`, as the progress channel would carry it,
/// queued nowhere (`vyreon-core progress`): to read what the channel will say before it is on.
pub async fn preview(c: &mut impl GenericClient, profile: &str, now: &str) -> Result<Vec<String>, String> {
    preview_as(c, profile, DEFAULT_LABEL, now).await
}

/// `preview`, its headline labelled `label`.
pub async fn preview_as(c: &mut impl GenericClient, profile: &str, label: &str, now: &str) -> Result<Vec<String>, String> {
    let start: String = c
        .query_one("SELECT ($1::text::timestamptz - make_interval(secs => $2::float8 * 60))::text", &[&now, &WINDOW_MINUTES])
        .await
        .map_err(db)?
        .get(0);
    let composed = compose(c, profile, &start, now).await?;
    Ok(render(&composed, label).0.into_iter().collect())
}

/// When the window since the last one (or, the first time, the last `WINDOW_MINUTES`) is
/// `WINDOW_MINUTES` long at `now`, composes it and queues its post with it, in one transaction.
/// None when no window is due.
pub async fn tick(client: &mut Client, profile: &str, now: &str) -> Result<Option<Window>, String> {
    tick_as(client, profile, DEFAULT_LABEL, now).await
}

/// `tick`, its headline labelled `label` (C-073: "Phase 1").
pub async fn tick_as(client: &mut Client, profile: &str, label: &str, now: &str) -> Result<Option<Window>, String> {
    let r = client
        .query_one(
            "WITH w AS (SELECT COALESCE((SELECT max(window_end) FROM core.progress_window WHERE window_end <= $1::text::timestamptz),
                                        $1::text::timestamptz - make_interval(secs => $2::float8 * 60)) AS start)
             SELECT w.start::text, $1::text::timestamptz - w.start >= make_interval(secs => $2::float8 * 60) FROM w",
            &[&now, &WINDOW_MINUTES],
        )
        .await
        .map_err(db)?;
    let (start, due): (String, bool) = (r.get(0), r.get(1));
    if !due {
        return Ok(None);
    }
    let composed = compose(client, profile, &start, now).await?;
    let (post, carried) = render(&composed, label);
    // C-073 (audit c072 r4): only what the post carried is stated (and the routine events, which have
    // no line); a line it could not hold leaves its events to the next window
    let mut shown: Shown = composed.routine.clone();
    if composed.news {
        shown.extend(composed.lines.iter().take(carried).flat_map(|(_, s)| s.clone()));
    }
    let n_lines = if composed.news { composed.lines.len() } else { 0 };
    let posts: Vec<String> = post.into_iter().collect();
    let tx = client.transaction().await.map_err(db)?;
    let n = tx
        .execute(
            "INSERT INTO core.progress_window (window_start, window_end, lines, posts) VALUES ($1::text::timestamptz, $2::text::timestamptz, $3, $4)
             ON CONFLICT (window_end) DO NOTHING",
            &[&start, &now, &(n_lines as i32), &(posts.len() as i32)],
        )
        .await
        .map_err(db)?;
    if n == 0 {
        return Ok(None);
    }
    let (sources, ids): (Vec<&str>, Vec<i64>) = shown.into_iter().unzip();
    tx.execute(
        "INSERT INTO core.progress_shown (window_end, source, id) SELECT $1::text::timestamptz, s, i FROM unnest($2::text[], $3::bigint[]) AS u(s, i)
         ON CONFLICT DO NOTHING",
        &[&now, &sources, &ids],
    )
    .await
    .map_err(db)?;
    for (i, p) in posts.iter().enumerate() {
        notice::progress(&tx, &format!("progress {now} {}", i + 1), p).await?;
    }
    // retention: windows (and what they stated) and the progress posts done with (delivered or
    // given up) are kept `KEEP_DAYS`; the newest window stays, whatever its age (the next starts
    // at its end)
    tx.execute(
        "DELETE FROM core.progress_window WHERE window_end < $1::text::timestamptz - make_interval(secs => $2::float8 * 86400)
           AND window_end < (SELECT max(window_end) FROM core.progress_window)",
        &[&now, &KEEP_DAYS],
    )
    .await
    .map_err(db)?;
    tx.execute(
        "DELETE FROM core.notice WHERE channel = 'progress' AND sent_at IS NOT NULL AND sent_at < $1::text::timestamptz - make_interval(secs => $2::float8 * 86400)",
        &[&now, &KEEP_DAYS],
    )
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Some(Window { start, end: now.to_owned(), lines: n_lines, posts: posts.len() }))
}

/// C-074: a republished day's line: the day's, then that it was republished (its job, scope and
/// reason), how many sensors it recomputed and which values changed.
fn republished_line(line: &str, scopes: &[Value]) -> String {
    let names = |k: &str| -> Vec<String> {
        let mut v: Vec<String> = scopes.iter().flat_map(|s| s[k].as_array().cloned().unwrap_or_default()).filter_map(|x| x.as_str().map(str::to_owned)).collect();
        v.sort();
        v.dedup();
        v
    };
    let first = &scopes[0];
    let asked: Vec<String> = names("families").into_iter().chain(names("measurements")).collect();
    let changed = names("value_changed");
    format!(
        "{line} · republished (job {}, [{}]: {}): {} sensors recomputed, the rest carried; {}",
        first["job"],
        asked.join(","),
        one_line(first["reason"].as_str().unwrap_or_default(), 120),
        names("recomputed").len(),
        if changed.is_empty() { "no value changed".to_owned() } else { format!("values changed in [{}]", changed.join(",")) }
    )
}

#[cfg(test)]
mod tests {
    /// C-074: a republished day says so, with its job, scope and reason, how many sensors it recomputed
    /// and which values changed.
    #[test]
    fn a_republished_days_line_names_its_scope_and_its_changes() {
        let base = serde_json::json!({"job": 41, "families": ["DLM-CHAIN"], "measurements": [], "recomputed": [], "value_changed": [], "reason": "D-999 max pain"});
        let local = serde_json::json!({"job": 41, "families": ["DLM-CHAIN"], "measurements": [], "recomputed": ["DLM-MAX-PAIN", "DLM-ZERO-DTE"], "value_changed": ["DLM-MAX-PAIN"], "reason": "D-999 max pain"});
        assert_eq!(
            republished_line("Dec 10, 2025 ✓ 99.1% of readings", &[local.clone(), base]),
            "Dec 10, 2025 ✓ 99.1% of readings · republished (job 41, [DLM-CHAIN]: D-999 max pain): 2 sensors recomputed, the rest carried; values changed in [DLM-MAX-PAIN]"
        );
        let mut same = local;
        same["value_changed"] = serde_json::json!([]);
        assert!(republished_line("Dec 10, 2025 ✓ 100% of readings", &[same]).ends_with("the rest carried; no value changed"));
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_read_with_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(3904), "3,904");
        assert_eq!(thousands(91066), "91,066");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    /// C-073 (owner, 2026-11-24): a day reads as its date, the share of its readings with a value,
    /// its engine faults flagged, a check that disagrees or did not complete flagged, and its hours;
    /// provider absences are not in the line. The counts are the real slice's AAPL day; the minutes
    /// are made up for the test.
    #[test]
    fn a_published_day_reads_in_one_line() {
        let t = watch::Tally { slots: 3904, withheld: 294, provider_withheld: 290, engine_faults: 6 };
        let untriaged = json!({"outcome": "COMPLETED", "unexplained": null});
        assert_eq!(day_line("Dec 16, 2025", t, Some(&untriaged), Some(318.0)), "Dec 16, 2025 ✓ 92.5% of readings · 6 engine faults ⚠ · 5.3 h");
        let clean = watch::Tally { slots: 3904, withheld: 35, provider_withheld: 35, engine_faults: 0 };
        assert_eq!(day_line("Aug 13, 2024", clean, None, Some(52.0)), "Aug 13, 2024 ✓ 99.1% of readings · 52 min");
        let disagrees = json!({"outcome": "COMPLETED", "unexplained": 254});
        assert_eq!(day_line("Oct 28, 2026", clean, Some(&disagrees), None), "Oct 28, 2026 ✓ 99.1% of readings · reference check DISAGREES ⚠ (254 unexplained)");
        let timeout = json!({"outcome": "TIMEOUT"});
        assert_eq!(day_line("Oct 28, 2026", clean, Some(&timeout), None), "Oct 28, 2026 ✓ 99.1% of readings · reference check did not complete ⚠ (timeout)");
        assert_eq!((percent(0.67), percent(100.0), percent(99.14)), ("0.7%".to_string(), "100%".to_string(), "99.1%".to_string()));
        assert_eq!(refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 0})), "reference check agrees (91,066 comparisons)");
        assert_eq!(
            refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 3958, "unexplained": 12})),
            "reference check DISAGREES: 12 unexplained failures of 91,066"
        );
        assert_eq!(refcheck_words(&json!({"outcome": "TIMEOUT", "why": "the suite ran past 3600 s"})), "reference check did not complete (timeout: the suite ran past 3600 s)");
        // audit round 2 of core-watch, 10: reference errors are stated beside the verdict, never
        // read as agreement (9 a day: build 48d87154's suite on 2026-11-02 to 11-04, C-054; here
        // beside the B0 report's counts)
        assert_eq!(
            refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 0, "reference_errors": 9})),
            "reference check agrees (91,066 comparisons; 9 reference errors not judged)"
        );
        assert_eq!(
            refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 3958, "unexplained": null, "reference_errors": 9})),
            "reference check: 3,958 failures of 91,066, not triaged against rulings; 9 reference errors not judged"
        );
        assert_eq!(
            refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 3958, "unexplained": 12, "reference_errors": 9})),
            "reference check DISAGREES: 12 unexplained failures of 91,066; 9 reference errors not judged"
        );
        assert_eq!(refcheck_words(&json!({"outcome": "COMPLETED", "checked": 91066, "failed": 0, "reference_errors": 0})), "reference check agrees (91,066 comparisons)");
    }

    /// C-072: hand-over 26 of 2026-11-24 as one line (its builds, what moved, the running jobs its
    /// withdrawal stopped, its reason cut); the counts are hand-over 26's
    /// shape (blocks and passes), its reason the operator's words that night, cut.
    #[test]
    fn a_hand_over_reads_in_one_line() {
        let reason = "D-460 r3 fixes: UPATH bin-0 false zero removed on every new day; strictly better than fc36eefb on wrong numbers";
        let h = |id: i64, from: &str, to: &str, moved: [i64; 3], verify_cancelled: i64, restarted: i64, reason: &str| {
            HandOver { id, from: from.into(), to: to.into(), reason: reason.into(), moved, refchecks: 0, republishes: 0, verify_cancelled, restarted, draining: 0, admitted: true }.line()
        };
        let line = h(26, "fc36eefb+b1c8d00b5d79", "2dde1132+5f8d87f49bfc", [0, 42, 593], 0, 8, reason);
        assert_eq!(
            line,
            "Hand-over 26: fc36eefb → 2dde1132, 42 blocks + 593 passes moved, 8 running jobs restarted — D-460 r3 fixes: UPATH bin-0 false zero removed on every new day; strictly bette…"
        );
        // two images of one engine commit are named whole; one of each, nothing restarted
        assert_eq!(
            h(3, "2dde1132+aaa", "2dde1132+bbb", [1, 1, 1], 2, 0, "image rebuilt"),
            "Hand-over 3: 2dde1132+aaa → 2dde1132+bbb, 1 tape + 1 block + 1 pass moved, 2 tape verify jobs cancelled — image rebuilt"
        );
        assert_eq!(h(4, "a", "b", [0, 0, 0], 0, 1, "x"), "Hand-over 4: a → b, nothing moved, 1 running job restarted — x");
        let only = HandOver { id: 6, from: "a".into(), to: "b".into(), reason: "x".into(), refchecks: 2, admitted: true, ..HandOver::default() };
        assert_eq!(only.line(), "Hand-over 6: a → b, 2 re-runs moved — x", "a hand-over of re-runs alone moved something");
        // C-074 (release-2 audit, Gemini 12): republishes are moved work too
        let republishes = HandOver { id: 7, from: "a".into(), to: "b".into(), reason: "x".into(), republishes: 3, admitted: true, ..HandOver::default() };
        assert_eq!(republishes.line(), "Hand-over 7: a → b, 3 republishes moved — x");
        // C-077: the running blocks a release lets finish their day
        let draining = HandOver { id: 8, from: "a".into(), to: "b".into(), reason: "x".into(), moved: [0, 8, 0], draining: 8, admitted: true, ..HandOver::default() };
        assert_eq!(draining.line(), "Hand-over 8: a → b, 8 blocks moved, 8 running blocks finishing their day first — x");
        // audit c072, 2: a successor not admitted keeps the hand-over's instruction
        let waiting = HandOver { id: 5, from: "a+1".into(), to: "b+2".into(), reason: "x".into(), moved: [0, 3, 0], admitted: false, ..HandOver::default() };
        assert_eq!(waiting.line(), "Hand-over 5: a → b, 3 blocks moved — x · b not admitted yet");
    }

    /// C-072: the blocks started in a window, in one line; a resumed or retried block says so.
    #[test]
    fn the_blocks_started_read_in_one_line() {
        let b = |f: &str, l: &str, a: i64, from: Option<&str>| (f.to_string(), l.to_string(), a, from.map(str::to_string));
        let july = [
            b("2024-07-09", "2024-07-12", 1, None),
            b("2024-07-15", "2024-07-15", 1, None),
            b("2024-07-16", "2024-07-19", 2, Some("2024-07-17")),
            b("2024-07-22", "2024-07-22", 1, None),
        ];
        assert_eq!(started_line(&july), "Started 4 blocks (2024): 07-09–07-12, 07-15, 07-16–07-19 (from 07-17, attempt 2), 07-22");
        assert_eq!(started_line(&[b("2024-12-30", "2025-01-03", 1, None)]), "Started 1 block: 2024-12-30–2025-01-03");
        let mut many: Vec<Started> = (1..=30).map(|d| b(&format!("2024-08-{d:02}"), &format!("2024-08-{d:02}"), 1, None)).collect();
        let line = started_line(&many);
        assert!(line.starts_with("Started 30 blocks (2024): 08-01, 08-02,") && line.ends_with("08-24, … and 6 more"), "{line}");
        // audit c072, 6: a retry or a resumed block is named whatever its place
        many[26] = b("2024-08-27", "2024-08-28", 3, Some("2024-08-28"));
        let line = started_line(&many);
        assert!(line.ends_with("08-24, 08-27–08-28 (from 08-28, attempt 3), … and 5 more"), "{line}");
    }

    /// C-073 (audit c072 r4): many retried or resumed starts are cut into lines a post can hold,
    /// every start named once and each line knowing which starts it states; a hand-over's line with a
    /// credential pattern is not shown.
    #[test]
    fn many_retries_read_in_lines_a_post_can_hold() {
        let b = |f: &str, l: &str, a: i64, from: Option<&str>| (f.to_string(), l.to_string(), a, from.map(str::to_string));
        let july = [b("2024-07-09", "2024-07-12", 1, None), b("2024-07-16", "2024-07-19", 2, Some("2024-07-17"))];
        assert_eq!(started_lines(&july), vec![(started_line(&july), vec![0, 1])], "a short line is one line");
        let mut blocks: Vec<Started> = (0..200).map(|i| b(&format!("2024-{:02}-{:02}", 1 + i / 28, 1 + i % 28), &format!("2024-{:02}-{:02}", 1 + i / 28, 1 + i % 28), 2 + (i % 3) as i64, Some(&format!("2024-{:02}-{:02}", 1 + i / 28, 1 + i % 28)))).collect();
        blocks.extend((0..40).map(|i| b(&format!("2024-09-{:02}", 1 + i % 28), &format!("2024-09-{:02}", 1 + i % 28), 1, None)));
        let lines = started_lines(&blocks);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|(l, _)| l.chars().count() <= STARTED_LINE_MAX + 40), "{:?}", lines.iter().map(|(l, _)| l.chars().count()).collect::<Vec<_>>());
        assert!(lines[0].0.starts_with("Started 240 blocks (2024): ") && lines[1].0.starts_with("Started, continued: "));
        let mut every: Vec<usize> = lines.iter().flat_map(|(_, i)| i.clone()).collect();
        every.sort_unstable();
        assert_eq!(every, (0..240).collect::<Vec<_>>(), "each start stated by exactly one line");
        assert!(lines.last().unwrap().0.ends_with("… and 16 more"));
        let retried: usize = lines.iter().map(|(l, _)| l.matches("attempt ").count()).sum();
        assert_eq!(retried, 200, "every retry named");
        let leaky = HandOver { id: 9, from: "a+1".into(), to: "b+2 password=hunter2pass".into(), reason: "x".into(), ..HandOver::default() };
        assert!(!leaky.line().contains("hunter2"), "{}", leaky.line());
        let leaky = HandOver { id: 9, from: "a+1 password=hunter2pass".into(), to: "b+2".into(), reason: "x".into(), admitted: true, ..HandOver::default() };
        assert!(!leaky.line().contains("hunter2"), "{}", leaky.line());
    }

    /// C-073 (owner, 2026-11-24): one post a window, under Discord's limit, no "(continued)"; the lines
    /// it cannot hold are counted and wait for the next window.
    #[test]
    fn a_long_window_is_one_post_and_the_rest_waits() {
        let line = |i: usize| format!("   {i:03} 15:02 Day 2025-12-15 failed for the last time and is set aside (job 9): stop {}", "·".repeat(60));
        let short: Vec<String> = (0..3).map(line).collect();
        let one = split("🟢 Phase 1: 4 of 597 days published (0.7%) · 9 building · no failures", &short);
        assert_eq!((one.len(), one[0].lines().count()), (1, 4));
        let long: Vec<String> = (0..200).map(line).collect();
        let (posts, carried) = split_counted("🟢 Phase 1", &long);
        assert_eq!(posts.len(), 1);
        assert!(posts[0].chars().count() <= LIMIT && !posts[0].contains("(continued)"));
        let tail = posts[0].lines().last().unwrap();
        assert_eq!(tail, format!("   … and {} more (in the next post; `vyreon-core log 100`)", 200 - carried));
        assert_eq!(posts[0].lines().count(), carried + 2, "the headline, the lines carried, the count");
    }

    /// Audit round 2 of core-watch, 14: a line longer than a post is cut to fit, the cut counted,
    /// with room left for what follows.
    #[test]
    fn a_line_longer_than_a_post_is_cut_and_counted() {
        let header = "🟢 Phase 1";
        let long = format!("   Restarted 1 block: {} (attempt 2)", "x".repeat(2500));
        let lines = vec![long.clone(), "   14:20 Queue resumed".to_string()];
        let (posts, carried) = split_counted(header, &lines);
        assert_eq!((posts.len(), carried), (1, 2));
        assert!(posts[0].ends_with("\n   14:20 Queue resumed"));
        assert!(posts[0].chars().count() <= LIMIT);
        let cut = posts[0].lines().nth(1).unwrap();
        let n: usize = cut.rsplit("… (").next().unwrap().trim_end_matches(" characters cut)").replace(',', "").parse().unwrap();
        assert_eq!(cut.chars().count() - format!("… ({} characters cut)", thousands(n as i64)).chars().count() + n, long.chars().count(), "what is kept and what is counted make the line");
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit(&"y".repeat(30), 25), format!("{}… (26 characters cut)", "y".repeat(4)));
    }
}
