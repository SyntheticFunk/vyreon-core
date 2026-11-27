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

/// One published day's line, in plain words (owner, 2026-11-25): "New: Jul 18 done in 3.6 h — 99.6% of
/// readings; 2 withheld by engine bugs (ARKK); 9 withheld: provider had no data (IBIT, ETHA)". The share of
/// its slots with a value; its engine faults (a defect, never hidden) and its provider absences, each by
/// its stated reason and with the assets behind it, up to 3; a reference check that disagrees or did not
/// complete (one that agrees, or is not in yet, is not news).
pub fn day_line(date: &str, t: watch::Tally, faulted: &[String], absent: &[String], refcheck: Option<&Value>, minutes: Option<f64>) -> String {
    let share = if t.slots > 0 { 100.0 * (t.slots - t.withheld) as f64 / t.slots as f64 } else { 0.0 };
    let took = minutes.map_or(String::new(), |m| format!(" in {}", duration(m)));
    let mut line = format!("New: {date} done{took} — {} of readings", percent(share));
    let who = |a: &[String]| if a.is_empty() { String::new() } else { format!(" ({})", names_max(a, 3)) };
    if t.engine_faults > 0 {
        line.push_str(&format!("; {} withheld by engine bugs{}", thousands(t.engine_faults), who(faulted)));
    }
    // (engine investigation, 2026-11-25) a provider's stated absence is no engine bug: said apart
    if t.provider_withheld > 0 {
        line.push_str(&format!("; {} withheld: provider had no data{}", thousands(t.provider_withheld), who(absent)));
    }
    if let Some(flag) = refcheck.and_then(refcheck_flag) {
        line.push_str(&format!("; {flag}"));
    }
    line
}

/// "a, b, c and 4 more".
pub fn names_max(names: &[String], max: usize) -> String {
    let shown = names.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > max { format!("{shown} and {} more", names.len() - max) } else { shown }
}

/// A reference check worth a line (C-073): one that disagrees or did not complete; None for one
/// that agrees or is not judged yet (the digest counts those).
pub fn refcheck_flag(r: &Value) -> Option<String> {
    match r["outcome"].as_str() {
        Some("COMPLETED") => r["unexplained"].as_i64().filter(|u| *u > 0).map(|u| format!("its reference check disagrees on {}", thousands(u))),
        Some(_) => Some("its reference check did not finish".to_owned()),
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
    /// The release in plain words (owner, 2026-11-25): what the owner sees happen, no builds or ids.
    pub fn plain_line(&self) -> String {
        let mut line = if self.draining > 0 {
            let (n, verb) = if self.draining == 1 { ("1 block".to_owned(), "finishes its") } else { (format!("{} blocks", thousands(self.draining)), "finish their") };
            format!("A release started: {n} of the old version {verb} current day, then the new version takes over")
        } else if self.restarted > 0 {
            format!("A release started: the new version takes over ({} restarted)", counted(self.restarted, "running block", "running blocks"))
        } else {
            "A release started: the new version takes over the queued work".to_owned()
        };
        if !self.admitted {
            line.push_str(" (the new version is not switched on yet)");
        }
        line
    }

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

pub async fn lines(c: &mut impl GenericClient, profile: &str, start: &str, end: &str) -> Result<Vec<String>, String> {
    Ok(compose(c, profile, None, start, end).await?.lines.into_iter().map(|(l, _)| l).collect())
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
    /// (owner, 2026-11-25) 🔴 only when something needs the owner (an open alert); no mark when fine.
    fn dot(&self) -> &'static str {
        if self.alerts > 0 { "🔴 " } else { "" }
    }

    fn failures(&self) -> String {
        if self.failed == 0 { "nothing broken".into() } else { format!("{} failed", thousands(self.failed)) }
    }

    /// "Phase 1: 18 of 597 days done (3%) · 9 computing · nothing broken".
    pub fn line(&self, label: &str) -> String {
        let days = if self.total > 0 {
            format!("{} of {} days done ({}) · ", thousands(self.published), thousands(self.total), percent(100.0 * self.published as f64 / self.total as f64))
        } else {
            String::new()
        };
        format!("{}{label}: {days}{} computing · {}", self.dot(), thousands(self.building), self.failures())
    }
}

/// A window composed: its lines (each with what it states), the events it states with no line of
/// their own (routine: a plain block start, a block's end, a check that agrees), the status lines
/// (passes, the daily run, tapes), whether anything new happened, and the headline.
struct Composed {
    lines: Vec<(String, Shown)>,
    routine: Shown,
    news: bool,
    /// C-091: the database's progress, the morning's first line.
    database: String,
    /// C-093: what is running at the window's end, the last line of every post.
    running: String,
    /// C-100: the days published in the window, one line before `running` ("Published: 3 days (…) · 5 of 597
    /// kept"); `.1` is the same without the count, for the morning's post, whose first line states it.
    published: Option<(String, String)>,
}

/// C-100 (owner, 2026-11-27: the progress channel did not say when a day published): the days published in a
/// window as one line, the first `PUBLISHED_NAMED` by name: "Published: 3 days (Sep 18, 2026; Jul 9, 2024; Sep 18,
/// 2024) · 5 of 597 kept". `kept` is (kept, total) of the database's line; without it the count is left out.
pub fn published_line(days: &[String], kept: Option<(i64, i64)>) -> Option<String> {
    if days.is_empty() {
        return None;
    }
    let mut names: Vec<String> = days.iter().take(PUBLISHED_NAMED).map(|d| span(d, d)).collect();
    if days.len() > PUBLISHED_NAMED {
        let last = names.len() - 1;
        names[last] = format!("{} and {} more", names[last], days.len() - PUBLISHED_NAMED);
    }
    let mut line = format!("Published: {} ({})", counted(days.len() as i64, "day", "days"), names.join("; "));
    match kept {
        Some((kept, total)) if total > 0 => line.push_str(&format!(" · {} of {} kept", thousands(kept), thousands(total))),
        Some((kept, _)) => line.push_str(&format!(" · {} kept", thousands(kept))),
        None => {}
    }
    Some(line)
}

/// C-100: how many published days a window's line names before "and N more".
pub const PUBLISHED_NAMED: usize = 6;

/// A logged line as the progress channel says it, in plain words (owner, 2026-11-25: "currently its
/// exposing a lot of alphanumeric codes"): the events the owner follows, each one short sentence; None for
/// internal housekeeping (a block's routine end, an admission or withdrawal a release line says, a lost
/// lease retried by itself, re-runs cancelled …), which `vyreon-core log` keeps.
pub fn short_notice(message: &str) -> Option<String> {
    let day_of = |m: &str, after: &str| m.split_once(after).and_then(|(_, r)| crate::plain::day(r.get(..10)?));
    if let Some(rest) = message.strip_prefix("Queue paused: ") {
        let why = rest.split(". No work is handed out").next().unwrap_or(rest);
        let why = if why.starts_with("release ") {
            "a release is switching versions".to_owned()
        } else if why.contains("bundle refused") {
            "a day's results were refused — needs a look".to_owned()
        } else if why.contains("days stand failed") {
            "several days failed — needs a look".to_owned()
        } else if why.contains("failed for the last time") {
            "a job failed for good — needs a look".to_owned()
        } else {
            one_line(why, 120)
        };
        return Some(format!("Queue paused: {why}"));
    }
    // C-087: the backfill's stop and resume, with the owner's reason
    if let Some((_, why)) = message.strip_prefix("Backfill stopped (").and_then(|r| r.split_once("): ")) {
        return Some(format!("Backfill stopped: {}", one_line(why, 120)));
    }
    if message.starts_with("Backfill resumed (") {
        return Some("Backfill resumed".into());
    }
    if message == "Queue resumed." {
        return Some("Queue resumed".into());
    }
    if message.starts_with("Queue already paused; also: ") {
        return Some("Queue still paused, and another cause came in — needs a look".into());
    }
    if message.starts_with("Draining job ") && message.contains("after its drained day") {
        return Some("The old version tried to send a day it no longer owns — handled automatically".into());
    }
    if message.starts_with("Draining day block job ") && message.contains("stopped at its bound") {
        return Some("An old block did not finish its day in time; the new version computes it".into());
    }
    if message.starts_with("Draining day block job ") && message.contains(" ended without ") {
        let day = day_of(message, " ended without ").unwrap_or_else(|| "its day".into());
        return Some(format!("An old block stopped before finishing {day}; the new version computes it"));
    }
    if message.starts_with("Day ") && message.contains("failed for the last time and is set aside") {
        let day = day_of(message, "Day ").unwrap_or_else(|| "A day".into());
        return Some(format!("⚠ {day} could not be computed after its tries: set aside, the rest goes on"));
    }
    if message.starts_with("Day ") && message.contains("refused by the engine's input plan") {
        let day = day_of(message, "Day ").unwrap_or_else(|| "A day".into());
        return Some(format!("⚠ {day} was refused by the engine for missing inputs: held"));
    }
    if message.starts_with("Ordered pass failed for the last time") {
        return Some("⚠ A history pass failed for good; the later ones wait".into());
    }
    if message.starts_with("Full recompute of ") || message.starts_with("Republish of [") && message.contains(" queued under build ") {
        return Some("A recompute was queued (days whose methods changed)".into());
    }
    if let Some(rest) = message.strip_prefix("All scheduled ").and_then(|r| r.split_once(" are done")) {
        return Some(format!("All {} are done", rest.0));
    }
    None
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
async fn compose(c: &mut impl GenericClient, profile: &str, phase: Option<&(String, String)>, start: &str, end: &str) -> Result<Composed, String> {
    // C-091 (owner, 2026-11-26: "block queued, block finished, database progress; any errors go in alerts"): a
    // block started, a block finished, and (the morning's) the database's progress; nothing else
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
    let mut events: Vec<(String, String, Shown)> = Vec::new();
    // each block started: from where its lease began to its last session
    let started = c
        .query(
            &format!(
                "SELECT l.granted_at::text, COALESCE(l.start_session, l.job_first, j.first_session)::text, COALESCE(l.job_last, j.last_session)::text,
                        j.priority = 1, l.start_session IS DISTINCT FROM COALESCE(l.job_first, j.first_session)
                            OR (SELECT count(*) FROM core.lease p WHERE p.job_id = l.job_id AND p.lease_id < l.lease_id) > 0,
                        (SELECT count(*) FROM core.session x WHERE x.session BETWEEN COALESCE(l.start_session, l.job_first, j.first_session) AND COALESCE(l.job_last, j.last_session)),
                        l.lease_id
                 FROM core.lease l JOIN core.job j USING (job_id)
                 WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $3 AND NOT COALESCE(j.scope @> '{{\"compare_only\": true}}', false)
                   AND l.granted_at > $1::text::timestamptz AND l.granted_at <= $2::text::timestamptz AND {fresh}
                 ORDER BY l.granted_at, l.lease_id",
                fresh = not_shown("lease", "l.lease_id")
            ),
            &[&from, &end, &profile],
        )
        .await
        .map_err(db)?;
    for r in &started {
        let (at, first, last, daily, again, days, lease): (String, String, String, bool, bool, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
        let line = match (daily, again) {
            (true, false) => format!("Nightly started: {}", span(&first, &first)),
            (true, true) => format!("Nightly started again: {}", span(&first, &first)),
            (false, false) => format!("Started: {} ({})", span(&first, &last), counted(days, "day", "days")),
            (false, true) => format!("Started again: {} ({} left)", span(&first, &last), counted(days, "day", "days")),
        };
        events.push((at, line, vec![("lease", lease)]));
    }
    // each block finished (its last lease done): the days it kept, published by its build
    let finished = c
        .query(
            &format!(
                "SELECT j.finished_at::text, j.first_session::text, j.last_session::text, j.priority = 1,
                        (SELECT count(*) FROM core.session x WHERE x.session BETWEEN j.first_session AND j.last_session),
                        (SELECT count(*) FROM core.session x WHERE x.session BETWEEN j.first_session AND j.last_session
                           AND EXISTS (SELECT FROM vyreon.day_revision b WHERE b.represented_date = x.session AND b.profile_id = j.profile_id
                                         AND b.state = 'PUBLISHED' AND b.base_revision IS NULL AND b.engine_build = j.engine_build
                                         AND EXISTS (SELECT FROM vyreon.day_revision l WHERE l.base_revision = b.revision_id AND l.day_local AND l.state = 'PUBLISHED'))),
                        j.job_id
                 FROM core.job j
                 WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $3 AND j.state = 'DONE' AND NOT COALESCE(j.scope @> '{{\"compare_only\": true}}', false)
                   AND j.finished_at > $1::text::timestamptz AND j.finished_at <= $2::text::timestamptz
                   AND EXISTS (SELECT FROM core.lease l WHERE l.job_id = j.job_id AND l.outcome = 'DONE') AND {fresh}
                 ORDER BY j.finished_at, j.job_id",
                fresh = not_shown("job", "j.job_id")
            ),
            &[&from, &end, &profile],
        )
        .await
        .map_err(db)?;
    for r in &finished {
        let (at, first, last, daily, days, kept, job): (String, String, String, bool, i64, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
        let line = if daily {
            format!("Nightly finished: {} · {}", span(&first, &first), if kept == days { "kept" } else { "not kept" })
        } else if kept == days {
            format!("Finished: {} · {} kept", span(&first, &last), counted(kept, "day", "days"))
        } else {
            format!("Finished: {} · {} of {} kept", span(&first, &last), thousands(kept), counted(days, "day", "days"))
        };
        events.push((at, line, vec![("job", job)]));
    }
    // C-097 (owner, 2026-11-26: "you shouldn't have to go looking"): a published day with something to watch (slow
    // against its build's usual day, idle for the captures' cutoffs, refused by the provider, a withholding on
    // many more assets than usual) is said once; a day with nothing is not. A day whose withholdings are not
    // counted yet waits `runview::FLAG_WAIT_MINUTES` for them. A failure here is logged, never the window's.
    // C-098: under a savepoint of its own, as each day's flags below: on a transaction (the preview's) a failed
    // statement would otherwise fail every statement after it, the window's own among them
    let listed = match c.transaction().await {
        Ok(tx) => {
            let r = tx
                .query(
                    &format!(
                "SELECT d.published_at::text, d.represented_date::text, d.revision_id,
                        d.published_at <= $2::text::timestamptz - make_interval(secs => $4::float8 * 60)
                 FROM vyreon.day_revision d
                 WHERE d.profile_id = $3 AND d.base_revision IS NULL AND d.state = 'PUBLISHED'
                   AND d.published_at > $1::text::timestamptz AND d.published_at <= $2::text::timestamptz AND {fresh}
                 ORDER BY d.published_at, d.revision_id",
                        fresh = not_shown("day", "d.revision_id")
                    ),
                    &[&from, &end, &profile, &crate::runview::FLAG_WAIT_MINUTES],
                )
                .await
                .map_err(db);
            let _ = tx.rollback().await;
            r
        }
        Err(e) => Err(db(e)),
    };
    let days: Vec<(String, String, i64, bool)> = match listed {
        Ok(rows) => rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect(),
        Err(e) => {
            eprintln!("vyreon-core: the published days' flags were not read (the progress post goes on): {e}");
            Vec::new()
        }
    };
    for (at, date, revision, waited) in days {
        let read = match c.transaction().await {
            Ok(tx) => {
                let r = crate::runview::day_flags(&tx, revision).await;
                let _ = tx.rollback().await;
                r
            }
            Err(e) => Err(db(e)),
        };
        match read {
            Ok((flags, counted)) if counted || waited => {
                if let Some(line) = crate::runview::flagged_line(&date, &flags) {
                    events.push((at, line, vec![("day", revision)]));
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("vyreon-core: the flags of {date} were not read (the progress post goes on): {e}"),
        }
    }
    events.sort_by(|a, b| a.0.cmp(&b.0));
    let lines: Vec<(String, Shown)> = events.into_iter().map(|(_, l, s)| (l, s)).collect();
    let database = database_line(c, profile, phase, end).await?;
    let running = running_line(c, profile, end).await?;
    // C-100: the days published in this window (its base, published whole: with its day-local layer), in the
    // order they came; read under a transaction of its own, and a failure leaves the line out, never the post
    let published = match published_in(c, profile, phase, start, end).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vyreon-core: the window's published days were not read (the progress post goes on): {e}");
            None
        }
    };
    let news = !lines.is_empty() || published.is_some();
    Ok(Composed { lines, routine: Vec::new(), news, database, running, published })
}

/// C-100: `published_line` for the window (start, end]: with the database's count, and without.
async fn published_in(c: &mut impl GenericClient, profile: &str, phase: Option<&(String, String)>, start: &str, end: &str) -> Result<Option<(String, String)>, String> {
    let tx = c.transaction().await.map_err(db)?;
    let read = async {
        let rows = tx
            .query(
                "SELECT d.represented_date::text FROM vyreon.day_revision d
                 WHERE d.profile_id = $3 AND d.base_revision IS NULL AND d.state = 'PUBLISHED'
                   AND d.published_at > $1::text::timestamptz AND d.published_at <= $2::text::timestamptz
                   AND EXISTS (SELECT FROM vyreon.day_revision l WHERE l.base_revision = d.revision_id AND l.day_local AND l.state = 'PUBLISHED')
                 ORDER BY d.published_at, d.revision_id LIMIT 500",
                &[&start, &end, &profile],
            )
            .await
            .map_err(db)?;
        let days: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
        if days.is_empty() {
            return Ok(None);
        }
        let n = database_counts(&tx, profile, phase, end).await?;
        Ok(published_line(&days, Some((n.kept, n.total))).zip(published_line(&days, None)))
    }
    .await;
    let _ = tx.rollback().await;
    read
}

/// C-093 (owner, 2026-11-26: "seeing live jobs in the progress discord channel would be useful"): the day blocks
/// holding a live lease at `now` (granted, not ended, not expired), the nightly named apart: "Running now: 8 blocks (Jul 16 – 18, 2024; …) · the
/// nightly (Nov 25, 2026)", or "Running now: nothing". Each block by its whole span, oldest first.
pub async fn running_line(c: &impl GenericClient, profile: &str, now: &str) -> Result<String, String> {
    let rows = c
        .query(
            "SELECT j.first_session::text, j.last_session::text, j.priority = 1
             FROM core.job j JOIN core.lease l ON l.job_id = j.job_id
             WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $1 AND j.state = 'LEASED'
               AND NOT COALESCE(j.scope @> '{\"compare_only\": true}', false)
               AND l.granted_at <= $2::text::timestamptz AND l.expires_at > $2::text::timestamptz
               AND (l.ended_at IS NULL OR l.ended_at > $2::text::timestamptz)
             ORDER BY j.first_session, j.job_id",
            &[&profile, &now],
        )
        .await
        .map_err(db)?;
    let blocks: Vec<(String, String, bool)> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect();
    // C-094 (audit 2026-11-26): the other kinds holding a live lease are counted, so "nothing" is nothing
    let others = c
        .query(
            "SELECT j.kind, count(*)::bigint FROM core.job j JOIN core.lease l ON l.job_id = j.job_id
             WHERE j.kind <> 'DAY_BLOCK' AND j.profile_id = $1 AND j.state = 'LEASED'
               AND l.granted_at <= $2::text::timestamptz AND l.expires_at > $2::text::timestamptz
               AND (l.ended_at IS NULL OR l.ended_at > $2::text::timestamptz)
             GROUP BY j.kind ORDER BY j.kind",
            &[&profile, &now],
        )
        .await
        .map_err(db)?;
    let others: Vec<(String, i64)> = others.iter().map(|r| (r.get(0), r.get(1))).collect();
    Ok(running_with(&blocks, &others))
}

/// `running_words` with the live work of the other kinds, in plain words: "· also 3 tape captures, 1 history pass".
pub fn running_with(blocks: &[(String, String, bool)], others: &[(String, i64)]) -> String {
    let said: Vec<String> = others
        .iter()
        .filter(|o| o.1 > 0)
        .map(|(kind, n)| match kind.as_str() {
            "TAPE" => counted(*n, "tape capture", "tape captures"),
            "ORDERED_PASS" => counted(*n, "history pass", "history passes"),
            "REFCHECK" => counted(*n, "reference check", "reference checks"),
            "REPUBLISH" => counted(*n, "recompute", "recomputes"),
            _ => counted(*n, "other job", "other jobs"),
        })
        .collect();
    let line = running_words(blocks);
    if said.is_empty() {
        line
    } else if blocks.is_empty() {
        format!("Running now: {}", said.join(", "))
    } else {
        format!("{line} · also {}", said.join(", "))
    }
}

/// `running_line`'s words from (first, last, nightly) per running block.
pub fn running_words(blocks: &[(String, String, bool)]) -> String {
    let backfill: Vec<String> = blocks.iter().filter(|b| !b.2).map(|b| span(&b.0, &b.1)).collect();
    let nightly: Vec<String> = blocks.iter().filter(|b| b.2).map(|b| span(&b.0, &b.0)).collect();
    let mut parts: Vec<String> = Vec::new();
    if !backfill.is_empty() {
        parts.push(format!("{} ({})", counted(backfill.len() as i64, "block", "blocks"), backfill.join("; ")));
    }
    if !nightly.is_empty() {
        parts.push(format!("the nightly ({})", nightly.join("; ")));
    }
    if parts.is_empty() {
        "Running now: nothing".to_owned()
    } else {
        format!("Running now: {}", parts.join(" · "))
    }
}

/// "Oct 1 – Nov 20, 2024", "Dec 16, 2024 – Jan 31, 2025", "Nov 25, 2026".
pub fn span(first: &str, last: &str) -> String {
    let day = |d: &str| crate::plain::day(d).unwrap_or_else(|| d.to_owned());
    let year = |d: &str| d.get(..4).unwrap_or("").to_owned();
    if first == last {
        format!("{}, {}", day(first), year(first))
    } else if year(first) == year(last) {
        format!("{} – {}, {}", day(first), day(last), year(last))
    } else {
        format!("{}, {} – {}, {}", day(first), year(first), day(last), year(last))
    }
}

/// C-091: the database's progress at `now`, one line: "Database: 212 of 597 days kept (current build) · 99.4% of
/// readings measured · finish ~Dec 14".
/// The days kept are the phase's sessions (VYREON_CORE_PROGRESS_FROM/TO; unset: every session a backfill block
/// covers) published whole (base and day-local) by the newest running build. The finish is at the build's own
/// measured rate: its days kept in the last 24 hours (or since its first, if later), said once it has kept
/// `ETA_MIN_DAYS` days and run `ETA_MIN_HOURS`; before, no date.
pub async fn database_line(c: &impl GenericClient, profile: &str, phase: Option<&(String, String)>, now: &str) -> Result<String, String> {
    let n = database_counts(c, profile, phase, now).await?;
    let (total, kept, recent, hours, slots, withheld) = (n.total, n.kept, n.recent, n.hours, n.slots, n.withheld);
    let mut line = if total > 0 { format!("Database: {} of {} days kept (current build)", thousands(kept), thousands(total)) } else { format!("Database: {} days kept (current build)", thousands(kept)) };
    // (owner, 2026-11-26: "worth keeping the % of readings number") the share of the kept days' slots with a value,
    // over the day's two layers as the watch has counted them (`core.revision_tally`)
    if let (Some(slots), Some(withheld)) = (slots.filter(|n| *n > 0), withheld) {
        line.push_str(&format!(" · {} of readings measured", percent(100.0 * (slots - withheld) as f64 / slots as f64)));
    }
    // C-100: that share counts a reading as withheld only when it carries the engine's fault marker; beside it,
    // the whole readings withheld for a stated reason, a day (the canary day read "100%" with SAI-EXQ01 and
    // M0-PRINT-SIZE withheld whole on all 64: 128), over the kept days whose withholdings are counted
    if let Some(a_day) = stated_a_day(n.stated, n.stated_days) {
        line.push_str(&format!(" · {} whole readings withheld a day for stated reasons", thousands(a_day)));
    }
    if total > 0 && kept >= total {
        line.push_str(" · done");
    } else if let Some(h) = hours.filter(|h| *h >= ETA_MIN_HOURS && recent >= ETA_MIN_DAYS && total > kept) {
        let left_hours = (total - kept) as f64 / (recent as f64 / h);
        let at: String = c
            .query_one("SELECT to_char(($1::text::timestamptz + make_interval(secs => $2::float8 * 3600)) AT TIME ZONE $3, 'Mon FMDD')", &[&now, &left_hours, &ZONE])
            .await
            .map_err(db)?
            .get(0);
        line.push_str(&format!(" · finish ~{at} at the measured rate"));
    }
    Ok(line)
}

/// C-100: the whole readings withheld a day for a stated reason: their sum over the days counted, a day (rounded);
/// None when no kept day's withholdings are counted yet, or none is withheld.
pub fn stated_a_day(stated: Option<i64>, days: i64) -> Option<i64> {
    match (stated, days) {
        (Some(s), d) if d > 0 && s > 0 => Some(((s as f64) / (d as f64)).round() as i64),
        _ => None,
    }
}

/// What `database_line` says, counted: the phase's sessions, those kept by the newest running build, the kept of
/// the last 24 hours and the hours they span, the kept days' slots and the slots the watch counted withheld, and
/// (C-100) the whole readings withheld for a stated reason over the kept days whose withholdings are counted
/// (`core.withheld_counted`, both layers), and how many days those are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatabaseCounts {
    pub total: i64,
    pub kept: i64,
    pub recent: i64,
    pub hours: Option<f64>,
    pub slots: Option<i64>,
    pub withheld: Option<i64>,
    pub stated: Option<i64>,
    pub stated_days: i64,
}

/// The phase's sessions (`s`), the newest running build (`cur`) and the days it has kept whole (`kept`: base and
/// day-local published), as `database_counts` reads them; $1 the profile, $2 now, $3/$4 the phase's bounds.
const KEPT: &str = "WITH s AS (SELECT DISTINCT x.session FROM core.job j JOIN core.session x ON x.session BETWEEN j.first_session AND j.last_session
                        WHERE $3::text IS NULL AND j.kind = 'DAY_BLOCK' AND j.profile_id = $1 AND j.priority = 0
                          AND NOT COALESCE(j.scope @> '{\"compare_only\": true}', false)
                          AND NOT EXISTS (SELECT FROM core.omitted_session o WHERE o.session = x.session AND o.profile_id = $1)
                        UNION
                        SELECT x.session FROM core.session x WHERE x.session BETWEEN $3::text::date AND $4::text::date
                          AND NOT EXISTS (SELECT FROM core.omitted_session o WHERE o.session = x.session AND o.profile_id = $1)),
                  cur AS (SELECT engine_build FROM core.engine_build WHERE core.build_runs(engine_build) ORDER BY admitted_at DESC, engine_build LIMIT 1),
                  kept AS (SELECT s.session, l.published_at, b.revision_id AS base, l.revision_id AS local FROM s
                           JOIN vyreon.day_revision b ON b.represented_date = s.session AND b.profile_id = $1 AND b.state = 'PUBLISHED' AND b.base_revision IS NULL
                                AND b.engine_build = (SELECT engine_build FROM cur)
                           JOIN LATERAL (SELECT l.published_at, l.revision_id FROM vyreon.day_revision l WHERE l.base_revision = b.revision_id AND l.day_local AND l.state = 'PUBLISHED'
                                         ORDER BY l.revision_id DESC LIMIT 1) l ON true
                           WHERE l.published_at <= $2::text::timestamptz)";

pub async fn database_counts(c: &impl GenericClient, profile: &str, phase: Option<&(String, String)>, now: &str) -> Result<DatabaseCounts, String> {
    let (from, to) = (phase.map(|p| p.0.clone()), phase.map(|p| p.1.clone()));
    let r = c
        .query_one(
            &format!(
                "{KEPT}
             SELECT (SELECT count(*) FROM s), (SELECT count(*) FROM kept),
                    (SELECT count(*) FROM kept WHERE published_at > $2::text::timestamptz - interval '24 hours'),
                    extract(epoch FROM $2::text::timestamptz - GREATEST((SELECT min(published_at) FROM kept), $2::text::timestamptz - interval '24 hours'))::float8 / 3600,
                    (SELECT sum(t.slots)::bigint FROM kept k JOIN core.revision_tally t ON t.revision_id IN (k.base, k.local)),
                    (SELECT sum(t.withheld)::bigint FROM kept k JOIN core.revision_tally t ON t.revision_id IN (k.base, k.local))"
            ),
            &[&profile, &now, &from, &to],
        )
        .await
        .map_err(db)?;
    // C-100: the whole readings withheld for a stated reason. Read apart and only where its tables are (asking
    // for a table that is not there would fail the line, and inside a transaction everything after it): without
    // them, or on any failure of the read, the line goes without this part.
    let there: bool = c
        .query_one("SELECT to_regclass('core.withheld_tally') IS NOT NULL AND to_regclass('core.withheld_counted') IS NOT NULL", &[])
        .await
        .map_err(db)?
        .get(0);
    let (stated, stated_days): (Option<i64>, i64) = if there {
        let engine: Vec<&str> = crate::watch::ENGINE_REASONS.to_vec();
        let w = c
            .query_one(
                &format!(
                    "{KEPT},
                  counted AS (SELECT k.base, k.local FROM kept k
                              WHERE (SELECT count(*) FROM core.withheld_counted x WHERE x.revision_id IN (k.base, k.local)) = 2)
             SELECT (SELECT COALESCE(sum(w.n), 0)::bigint FROM counted k JOIN core.withheld_tally w ON w.revision_id IN (k.base, k.local)
                     WHERE w.grain = 'slot' AND w.reason <> ALL ($5::text[])),
                    (SELECT count(*) FROM counted)"
                ),
                &[&profile, &now, &from, &to, &engine],
            )
            .await
            .map_err(db)?;
        (w.get(0), w.get(1))
    } else {
        (None, 0)
    };
    Ok(DatabaseCounts { total: r.get(0), kept: r.get(1), recent: r.get(2), hours: r.get(3), slots: r.get(4), withheld: r.get(5), stated, stated_days })
}

/// C-091: the finish is said once the build has kept this many days within the rate's window, and
/// run this many hours.
pub const ETA_MIN_DAYS: i64 = 3;
pub const ETA_MIN_HOURS: f64 = 2.0;

/// A window's post (C-073): its headline, then "+N this half hour:" and the days, the other news and
/// the state, each indented; with nothing new, the headline's one quiet line; nothing at all when no
/// work is left and nothing happened. Returns the post and how many lines it carried.
fn render(composed: &Composed, label: &str, heartbeat: bool) -> (Option<String>, usize) {
    // (owner, 2026-11-25) a quiet half hour posts nothing; the morning's first window posts the state
    // whatever happened, so silence is never ambiguous
    if !composed.news && !heartbeat {
        return (None, 0);
    }
    // C-091: the morning's post leads with the database's progress; any other, with its first event
    let _ = label;
    let lines: Vec<String> = composed.lines.iter().map(|(l, _)| crate::plain::plain(l)).collect();
    // C-100: the window's published days: with the count, or (the morning's post, whose first line is the
    // database's) without. Like `running` it is no event: it closes the post and never counts as carried.
    let published = composed.published.as_ref().map(|(counted, bare)| if heartbeat { bare.clone() } else { counted.clone() });
    let (header, rest, header_is_event) = match (heartbeat, lines.first()) {
        (true, _) => (composed.database.clone(), &lines[..], false),
        (false, Some(first)) => (first.clone(), &lines[1..], true),
        // nothing happened but days published: their line leads
        (false, None) => match &published {
            Some(p) => {
                let (posts, _) = split_counted(p, &[crate::plain::plain(&composed.running)]);
                return (posts.into_iter().next(), 0);
            }
            None => return (None, 0),
        },
    };
    // C-093: what is running closes every post; it is no event, so it never counts as carried
    let mut rest: Vec<String> = rest.to_vec();
    let fixed = 1 + usize::from(published.is_some());
    rest.extend(published);
    rest.push(crate::plain::plain(&composed.running));
    let (posts, carried) = split_counted(&header, &rest);
    let carried = carried.min(rest.len() - fixed);
    (posts.into_iter().next(), if header_is_event { carried + 1 } else { carried })
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
    preview_phase(c, profile, label, None, now).await
}

/// `preview_as` with the phase's range (`VYREON_CORE_PROGRESS_FROM`/`_TO`): its sessions are the headline's total.
pub async fn preview_phase(c: &mut impl GenericClient, profile: &str, label: &str, phase: Option<&(String, String)>, now: &str) -> Result<Vec<String>, String> {
    let start: String = c
        .query_one("SELECT ($1::text::timestamptz - make_interval(secs => $2::float8 * 60))::text", &[&now, &WINDOW_MINUTES])
        .await
        .map_err(db)?
        .get(0);
    let composed = compose(c, profile, phase, &start, now).await?;
    Ok(render(&composed, label, true).0.into_iter().collect())
}

/// When the window since the last one (or, the first time, the last `WINDOW_MINUTES`) is
/// `WINDOW_MINUTES` long at `now`, composes it and queues its post with it, in one transaction.
/// None when no window is due.
pub async fn tick(client: &mut Client, profile: &str, now: &str) -> Result<Option<Window>, String> {
    tick_as(client, profile, DEFAULT_LABEL, now).await
}

/// `tick`, its headline labelled `label` (C-073: "Phase 1").
pub async fn tick_as(client: &mut Client, profile: &str, label: &str, now: &str) -> Result<Option<Window>, String> {
    tick_phase(client, profile, label, None, now).await
}

/// `tick_as` with the phase's range (`preview_phase`).
pub async fn tick_phase(client: &mut Client, profile: &str, label: &str, phase: Option<&(String, String)>, now: &str) -> Result<Option<Window>, String> {
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
    let composed = compose(client, profile, phase, &start, now).await?;
    // the morning's heartbeat: from 09:00 in the owner's zone, the first window of the day posts
    let heartbeat: bool = client
        .query_one(
            "WITH t AS (SELECT ($1::text::timestamptz AT TIME ZONE $2) AS local)
             SELECT extract(hour FROM t.local) >= 9
                    AND NOT EXISTS (SELECT FROM core.progress_window w WHERE w.posts > 0
                                      AND w.window_end >= (date_trunc('day', t.local) + interval '9 hours') AT TIME ZONE $2)
             FROM t",
            &[&now, &ZONE],
        )
        .await
        .map_err(db)?
        .get(0);
    let (post, carried) = render(&composed, label, heartbeat);
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

#[cfg(test)]
mod tests {
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
        let untriaged = json!({"outcome": "COMPLETED", "unexplained": null});
        // (owner, 2026-11-25) plain words, the assets behind the engine faults named (3, then counted)
        // the real slice's AAPL day: 6 engine-fault slots and 288 provider absences (its 294 withheld less the 6)
        let assets: Vec<String> = ["ARKK", "IBIT", "MSTR", "TSLA"].map(String::from).to_vec();
        let t = watch::Tally { slots: 3904, withheld: 294, provider_withheld: 288, engine_faults: 6 };
        assert_eq!(
            day_line("Dec 16", t, &assets, &["ETHA".to_owned(), "IBIT".to_owned()], Some(&untriaged), Some(318.0)),
            "New: Dec 16 done in 5.3 h — 92.5% of readings; 6 withheld by engine bugs (ARKK, IBIT, MSTR and 1 more); 288 withheld: provider had no data (ETHA, IBIT)"
        );
        let clean = watch::Tally { slots: 3904, withheld: 0, provider_withheld: 0, engine_faults: 0 };
        assert_eq!(day_line("Aug 13", clean, &[], &[], None, Some(52.0)), "New: Aug 13 done in 52 min — 100% of readings");
        let disagrees = json!({"outcome": "COMPLETED", "unexplained": 254});
        assert_eq!(day_line("Oct 28", clean, &[], &[], Some(&disagrees), None), "New: Oct 28 done — 100% of readings; its reference check disagrees on 254");
        let timeout = json!({"outcome": "TIMEOUT"});
        assert_eq!(day_line("Oct 28", clean, &[], &[], Some(&timeout), None), "New: Oct 28 done — 100% of readings; its reference check did not finish");
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
        // (owner, 2026-11-25) the progress channel's words for it
        assert_eq!(draining.plain_line(), "A release started: 8 blocks of the old version finish their current day, then the new version takes over");
        assert_eq!(waiting_plain(), "A release started: the new version takes over the queued work (the new version is not switched on yet)");
        // audit c072, 2: a successor not admitted keeps the hand-over's instruction
        let waiting = HandOver { id: 5, from: "a+1".into(), to: "b+2".into(), reason: "x".into(), moved: [0, 3, 0], admitted: false, ..HandOver::default() };
        assert_eq!(waiting.line(), "Hand-over 5: a → b, 3 blocks moved — x · b not admitted yet");
    }

    fn waiting_plain() -> String {
        HandOver { id: 5, from: "a+1".into(), to: "b+2".into(), reason: "x".into(), moved: [0, 3, 0], admitted: false, ..HandOver::default() }.plain_line()
    }

    /// (owner, 2026-11-25) the events the owner follows in plain words; housekeeping not at all.
    #[test]
    fn the_events_read_in_plain_words() {
        assert_eq!(short_notice("Queue paused: release 3d7567c8+aa: release 4. No work is handed out until `vyreon-core queue resume`.").as_deref(), Some("Queue paused: a release is switching versions"));
        assert_eq!(short_notice("Queue resumed.").as_deref(), Some("Queue resumed"));
        assert_eq!(
            short_notice("Draining job 21823 of build 1ae946f7+715623e13454 sent 2024-09-06 after its drained day 2024-09-05 (a worker that does not stop at core's last day): refused, its lease ended, nothing paused; later days are build x's.").as_deref(),
            Some("The old version tried to send a day it no longer owns — handled automatically")
        );
        assert_eq!(
            short_notice("Day 2024-07-23 failed for the last time and is set aside (job 21818): stop: x. The build goes on.").as_deref(),
            Some("⚠ Jul 23 could not be computed after its tries: set aside, the rest goes on")
        );
        assert_eq!(short_notice("Engine build 1ae946f7+715623e13454 admitted: its workers may claim work."), None);
        assert_eq!(short_notice("Job 21823 lost its lease on 2024-09-05 without a stop (1 of 2 such losses free on that day): the attempt is not counted."), None);
        assert_eq!(short_notice("Reference-check re-runs cancelled: 3"), None);
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
        let one = split("Phase 1: 4 of 597 days done (0.7%) · 9 computing · nothing broken", &short);
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

    /// C-093 (owner, 2026-11-26): what runs closes every post, the nightly named apart.
    #[test]
    fn running_now_names_each_block_and_the_nightly() {
        let b = |f: &str, l: &str, n: bool| (f.to_owned(), l.to_owned(), n);
        // C-100: the days a window published, one line; six by name, then counted; the count beside it
        let d = |s: &str| s.to_owned();
        assert_eq!(published_line(&[], Some((5, 597))), None);
        assert_eq!(published_line(&[d("2026-09-18")], Some((2, 597))).unwrap(), "Published: 1 day (Sep 18, 2026) · 2 of 597 kept");
        assert_eq!(
            published_line(&[d("2026-09-18"), d("2024-07-09"), d("2024-09-18")], Some((5, 597))).unwrap(),
            "Published: 3 days (Sep 18, 2026; Jul 9, 2024; Sep 18, 2024) · 5 of 597 kept"
        );
        let eight: Vec<String> = (1..=8).map(|i| format!("2025-03-0{i}")).collect();
        assert_eq!(
            published_line(&eight, None).unwrap(),
            "Published: 8 days (Mar 1, 2025; Mar 2, 2025; Mar 3, 2025; Mar 4, 2025; Mar 5, 2025; Mar 6, 2025 and 2 more)"
        );
        assert_eq!(published_line(&[d("2026-09-18")], Some((3, 0))).unwrap(), "Published: 1 day (Sep 18, 2026) · 3 kept");
        // C-100: the whole readings withheld a day for a stated reason (the canary day's 128; the new build's
        // first day's 129), over the days counted
        assert_eq!(stated_a_day(Some(128), 1), Some(128));
        assert_eq!(stated_a_day(Some(257), 2), Some(129));
        assert_eq!((stated_a_day(Some(0), 3), stated_a_day(Some(40), 0), stated_a_day(None, 2)), (None, None, None));
        assert_eq!(running_words(&[]), "Running now: nothing");
        let o = |k: &str, n: i64| (k.to_owned(), n);
        assert_eq!(running_with(&[], &[o("TAPE", 3), o("ORDERED_PASS", 1)]), "Running now: 3 tape captures, 1 history pass");
        assert_eq!(running_with(&[b("2026-11-25", "2026-11-25", true)], &[o("REPUBLISH", 2)]), "Running now: the nightly (Nov 25, 2026) · also 2 recomputes");
        assert_eq!(running_with(&[], &[]), "Running now: nothing");
        assert_eq!(running_words(&[b("2026-11-25", "2026-11-25", true)]), "Running now: the nightly (Nov 25, 2026)");
        assert_eq!(
            running_words(&[b("2024-07-16", "2024-07-18", false), b("2024-09-04", "2024-09-04", false), b("2026-11-25", "2026-11-25", true)]),
            format!("Running now: 2 blocks ({}; {}) · the nightly (Nov 25, 2026)", span("2024-07-16", "2024-07-18"), span("2024-09-04", "2024-09-04"))
        );
    }
}
