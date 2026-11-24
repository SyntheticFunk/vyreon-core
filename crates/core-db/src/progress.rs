//! The progress channel (C-065, owner 2026-11-22): what core logs and what the run did, batched every
//! `WINDOW_MINUTES` into a few silent posts for a second, optional Discord channel. The owner's
//! alerts channel (C-050) is unchanged: it says whether to look; this one says what moved.
//!
//! A window holds, each from core's own rows (nothing is computed that core does not already hold):
//!   * each day published (its day-local revision, which completes the day a block publishes):
//!     its slots, those with a value, those withheld whole by reason (the provider's input, an
//!     engine fault: the engine's own convention, `watch::COUNTED`), its reference check when it
//!     has come, the minutes the day took in its block, the block and its worker;
//!   * the reference checks that came in for days published earlier;
//!   * the day blocks started (leases granted), in one line of their dates (C-072);
//!   * every line core logged (`notice::log`): blocks done, days set aside with their cause, dates
//!     omitted, builds admitted or withdrawn, retirements, the queue paused or resumed; a hand-over
//!     is one line, with its withdrawal, its successor's admission and the operator's pause and
//!     resume of its window folded into it (C-072);
//!   * the queue's counts by kind: done in the window, done in all, running, waiting, failed.
//!
//! A window in which nothing happened posts nothing. A window's posts are queued with the window,
//! in one transaction, so a window is never posted twice nor lost (C-027's outbox).
use crate::notice;
use crate::watch::{self, LIMIT, ZONE, kind_words, one_line};
use serde_json::Value;
use tokio_postgres::{Client, GenericClient};

/// A window's length (the owner's "every 30 minutes").
pub const WINDOW_MINUTES: f64 = 30.0;
/// At most this many posts a window; what does not fit is counted.
pub const MAX_POSTS: usize = 3;
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

/// One published day's line.
pub fn day_line(words: &str, t: watch::Tally, refcheck: Option<&Value>, minutes: Option<f64>, block: Option<(&str, &str, &str, i64)>) -> String {
    let with_value = t.slots - t.withheld;
    let engine = t.withheld - t.provider_withheld;
    let withheld = match (t.provider_withheld, engine) {
        (0, 0) => "none withheld".to_string(),
        (p, 0) => format!("withheld: {} provider input", thousands(p)),
        (0, e) => format!("withheld: {} engine fault", thousands(e)),
        (p, e) => format!("withheld: {} provider input, {} engine fault", thousands(p), thousands(e)),
    };
    let check = refcheck.map_or_else(|| "reference check not in yet".to_string(), refcheck_words);
    let minutes = minutes.map_or(String::new(), |m| format!(" · {m:.0} min"));
    let block = block.map_or(String::new(), |(first, last, worker, job)| {
        let span = if first == last { first.to_string() } else { format!("{first} to {last}") };
        format!(" · block {span} on {worker} (job {job})")
    });
    format!("Published {words}: {} slots, {} with a value, {withheld} · {check}{minutes}{block}", thousands(t.slots), thousands(with_value))
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
    /// The old build's tape verify jobs cancelled, not moved.
    pub verify_cancelled: i64,
    /// The running jobs its withdrawal stopped (moved, to run again under the successor).
    pub restarted: i64,
    /// The steps of its window folded into its line ("queue paused 03:28").
    pub folded: Vec<String>,
    /// Its successor runs (admitted, not withdrawn) as the window is composed; when not, the line
    /// keeps the hand-over's instruction to admit it (audit c072, 2).
    pub admitted: bool,
}

impl HandOver {
    /// One line: its builds, what it moved, the running jobs restarted, its reason cut to
    /// `REASON_MAX`, then its window's steps.
    pub fn line(&self) -> String {
        let HandOver { id, from, to, reason, moved, verify_cancelled, restarted, folded, admitted } = self;
        let (verify_cancelled, restarted) = (*verify_cancelled, *restarted);
            let parts: Vec<String> = [(moved[0], "tape", "tapes"), (moved[1], "block", "blocks"), (moved[2], "pass", "passes")]
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
        if verify_cancelled > 0 {
            line.push_str(&format!(", {} cancelled", counted(verify_cancelled, "tape verify job", "tape verify jobs")));
        }
        line.push_str(&format!(" — {}", one_line(reason, REASON_MAX)));
        if !folded.is_empty() {
            line.push_str(&format!(" · {}", folded.join(" · ")));
        }
        if !admitted {
            line.push_str(&format!(" · build {to} is not admitted: admit it now (`vyreon-core build admit {to} <evidence.json>`)"));
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

/// The window's lines, in sections: days published, reference checks come in, blocks started, the
/// log, the queue's counts. Empty when nothing happened in it (the counts alone are not news).
pub async fn lines(c: &mut impl GenericClient, profile: &str, start: &str, end: &str) -> Result<Vec<String>, String> {
    Ok(compose(c, profile, start, end).await?.0)
}

/// What a window states, as `progress_shown` records it: (source, id).
type Shown = Vec<(&'static str, i64)>;

/// `lines`, and what they state. Each event (a day published, a report, a lease granted, a line
/// logged) is stamped inside the transaction that writes it, which may commit after a window that
/// covers its time was composed; the next window would then leave it out by its time (audit round
/// 2 of core-watch, 9). So a window states each event once (`core.progress_shown`), and takes
/// every one not stated yet from `STALE_HOURS` before its start (never before the first window's
/// start, nor after its own) to its end.
async fn compose(c: &mut impl GenericClient, profile: &str, start: &str, end: &str) -> Result<(Vec<String>, Shown), String> {
    let mut lines = Vec::new();
    let mut shown: Shown = Vec::new();
    let mut happened = false;
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
    // the year only when it is not the window's
    let day = |e: &str| {
        format!(
            "CASE WHEN extract(year FROM {e}) = extract(year FROM ($2::text::timestamptz AT TIME ZONE '{ZONE}'))
                  THEN to_char({e}, 'Dy Mon FMDD') ELSE to_char({e}, 'Dy Mon FMDD, YYYY') END"
        )
    };

    // each day published: its day-local revision (a block publishes the base, then the day-local
    // revision on it, C-031), with the lease that published it (`core.lease_revision`; none when
    // it was published by hand: no block, no minutes), the minutes counted from that lease's
    // previous day-local revision or its grant
    let days = c
        .query(
            &format!(
                "WITH d AS (SELECT b.represented_date AS day, b.revision_id AS base, l.revision_id AS local, l.published_at, b.engine_build, b.profile_id
                            FROM vyreon.day_revision l JOIN vyreon.day_revision b ON b.revision_id = l.base_revision AND b.represented_date = l.represented_date
                            WHERE l.day_local AND l.profile_id = $3 AND l.published_at > $1::text::timestamptz AND l.published_at <= $2::text::timestamptz
                              AND {fresh})
                 SELECT {words}, d.base, d.local, x.job_id, x.worker, x.first_session::text, x.last_session::text,
                        extract(epoch FROM d.published_at - GREATEST(x.granted_at,
                            (SELECT max(p.published_at) FROM core.lease_revision pr JOIN vyreon.day_revision p USING (revision_id)
                             WHERE pr.lease_id = x.lease_id AND p.day_local AND p.published_at < d.published_at)))::float8 / 60,
                        (SELECT jsonb_build_object('id', r.refcheck_report_id, 'outcome', r.outcome, 'checked', r.checked, 'failed', r.failed,
                                                   'unexplained', r.failed_unexplained, 'reference_errors', r.reference_errors, 'why', r.why)
                         FROM core.refcheck_report r
                         WHERE r.base_revision = d.base AND (r.day_local_revision IS NULL OR r.day_local_revision = d.local)
                           AND r.stored_at <= $2::text::timestamptz
                         ORDER BY r.stored_at DESC, r.refcheck_report_id DESC LIMIT 1)::text
                 FROM d LEFT JOIN LATERAL (
                     SELECT l.lease_id, l.job_id, l.worker, l.granted_at, j.first_session, j.last_session
                     FROM core.lease_revision lr JOIN core.lease l USING (lease_id) JOIN core.job j USING (job_id)
                     WHERE lr.revision_id = d.local AND j.kind = 'DAY_BLOCK') x ON true
                 ORDER BY d.published_at, d.day",
                words = day("d.day"),
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
            let (words, base, local): (String, i64, i64) = (r.get(0), r.get(1), r.get(2));
            let t = c
                .query_one(
                    "SELECT COALESCE(sum(slots), 0)::bigint, COALESCE(sum(withheld), 0)::bigint, COALESCE(sum(provider_withheld), 0)::bigint, COALESCE(sum(engine_faults), 0)::bigint
                     FROM core.revision_tally WHERE revision_id = ANY($1)",
                    &[&vec![base, local]],
                )
                .await
                .map_err(db)?;
            let tally = watch::Tally { slots: t.get(0), withheld: t.get(1), provider_withheld: t.get(2), engine_faults: t.get(3) };
            let job: Option<i64> = r.get(3);
            let (worker, first, last): (Option<String>, Option<String>, Option<String>) = (r.get(4), r.get(5), r.get(6));
            let minutes: Option<f64> = r.get(7);
            let check: Option<Value> = r.get::<_, Option<String>>(8).and_then(|s| serde_json::from_str(&s).ok());
            let block = match (&first, &last, &worker, job) {
                (Some(f), Some(l), Some(w), Some(j)) => Some((f.as_str(), l.as_str(), w.as_str(), j)),
                _ => None,
            };
            stated.extend(check.as_ref().and_then(|c| c["id"].as_i64()));
            shown.push(("revision", local));
            lines.push(day_line(&words, tally, check.as_ref(), minutes, block));
        }
        happened = true;
    }

    // reference checks that came in for days published before, each said of the revisions it
    // checked when they are no longer the day's published ones
    let checks = c
        .query(
            &format!(
                "SELECT {words}, jsonb_build_object('outcome', r.outcome, 'checked', r.checked, 'failed', r.failed, 'unexplained', r.failed_unexplained,
                                                   'reference_errors', r.reference_errors, 'why', r.why)::text,
                        b.state = 'PUBLISHED' AND (r.day_local_revision IS NULL
                            OR EXISTS (SELECT FROM vyreon.day_revision x WHERE x.revision_id = r.day_local_revision AND x.state = 'PUBLISHED')),
                        r.base_revision, r.day_local_revision, r.refcheck_report_id
                 FROM core.refcheck_report r JOIN vyreon.day_revision b ON b.revision_id = r.base_revision
                 WHERE b.profile_id = $3 AND r.stored_at > $1::text::timestamptz AND r.stored_at <= $2::text::timestamptz AND NOT r.refcheck_report_id = ANY($4)
                   AND {fresh}
                 ORDER BY r.stored_at, r.refcheck_report_id",
                words = day("r.session"),
                fresh = not_shown("refcheck", "r.refcheck_report_id")
            ),
            &[&from, &end, &profile, &stated],
        )
        .await
        .map_err(db)?;
    for r in &checks {
        let (words, check, current, base, local): (String, String, bool, i64, Option<i64>) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        let check: Value = serde_json::from_str(&check).map_err(|e| e.to_string())?;
        let of = match (current, local) {
            (true, _) => String::new(),
            (false, Some(l)) => format!(" (revisions {base} and {l}, since replaced)"),
            (false, None) => format!(" (revision {base}, since replaced)"),
        };
        lines.push(format!("Reference check of {words}{of}: {}", refcheck_words(&check).trim_start_matches("reference check ")));
        shown.push(("refcheck", r.get(5)));
        happened = true;
    }

    shown.extend(stated.iter().map(|id| ("refcheck", *id)));

    // day blocks started: leases granted, in one line (C-072)
    let started = c
        .query(
            &format!(
                "SELECT j.first_session::text, j.last_session::text, l.worker, l.job_id, l.start_session::text,
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
    if !started.is_empty() {
        let blocks: Vec<Started> = started.iter().map(|r| (r.get(0), r.get(1), r.get(5), r.get(4))).collect();
        lines.push(started_line(&blocks));
        shown.extend(started.iter().map(|r| ("lease", r.get::<_, i64>(6))));
        happened = true;
    }

    // the hand-overs of the window (C-072), each in one line: what it moved, the running jobs its
    // withdrawal stopped (carried to the successor, to run again)
    let hand_overs = c
        .query(
            &format!(
                "SELECT h.hand_over_id, h.from_build, h.to_build, h.reason,
                        count(*) FILTER (WHERE j.kind = 'TAPE' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE j.kind = 'DAY_BLOCK' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE j.kind = 'ORDERED_PASS' AND x.new_job IS NOT NULL),
                        count(*) FILTER (WHERE x.old_job IS NOT NULL AND x.new_job IS NULL),
                        (SELECT count(DISTINCT l.job_id) FROM core.hand_over_job y JOIN core.lease l ON l.job_id = y.old_job
                         WHERE y.hand_over_id = h.hand_over_id AND y.new_job IS NOT NULL
                           AND l.outcome = 'CANCELLED' AND l.failure = 'its engine build was withdrawn'),
                        core.build_runs(h.to_build)
                 FROM core.hand_over h LEFT JOIN core.hand_over_job x USING (hand_over_id) LEFT JOIN core.job j ON j.job_id = x.old_job
                 WHERE h.handed_at > $1::text::timestamptz AND h.handed_at <= $2::text::timestamptz AND {fresh}
                 GROUP BY h.hand_over_id ORDER BY h.handed_at, h.hand_over_id",
                fresh = not_shown("hand_over", "h.hand_over_id")
            ),
            &[&from, &end],
        )
        .await
        .map_err(db)?;
    let mut hand_overs: Vec<HandOver> = hand_overs
        .iter()
        .map(|r| HandOver {
            id: r.get(0),
            from: r.get(1),
            to: r.get(2),
            reason: r.get(3),
            moved: [r.get(4), r.get(5), r.get(6)],
            verify_cancelled: r.get(7),
            restarted: r.get(8),
            folded: Vec::new(),
            admitted: r.get(9),
        })
        .collect();
    // the time of each one's own notice, where its line goes
    let mut placed: Vec<Option<String>> = vec![None; hand_overs.len()];
    shown.extend(hand_overs.iter().map(|h| ("hand_over", h.id)));

    // everything core logged in the window, as it logged it, but what belongs to a hand-over of the
    // window: its withdrawal, its own notice, its successor's admission, and (the operator's steps
    // around it) the operator's pause, the resume and the planned steps' lines, folded into its line
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
    // where each hand-over's line goes among the logged lines (at its own notice), and the lines.
    // Audit c072, 3: only what is the hand-over's own is folded: its own withdrawal (its reason
    // "handed over to build …"; a withdrawal of its own keeps its line and reason), its successor's
    // admission, and the operator's pause the hand-over was made in (paused before its notice and
    // not resumed before it) with that pause's resume and planned-step lines; any other pause keeps
    // its line and reason
    let rows: Vec<(String, String, i64)> = logged.iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect();
    const OPERATOR_PAUSE: &str = "Queue paused by the operator: ";
    const PAUSE_END: &str = ". No work is handed out until `vyreon-core queue resume`.";
    let own_at: Vec<Option<usize>> = hand_overs.iter().map(|h| rows.iter().position(|r| r.1.starts_with(&format!("Hand-over {}: ", h.id)))).collect();
    // each operator pause of the window: (its row, its resume's row, the hand-over it was made in)
    let mut pauses: Vec<(usize, Option<usize>, usize, String)> = Vec::new();
    for (p, row) in rows.iter().enumerate().filter(|(_, r)| r.1.starts_with(OPERATOR_PAUSE)) {
        let resume = rows.iter().skip(p + 1).position(|r| r.1 == "Queue resumed.").map(|i| i + p + 1);
        let within = own_at.iter().rposition(|o| o.is_some_and(|o| o > p && resume.is_none_or(|r| r > o)));
        if let Some(i) = within {
            let reason = row.1[OPERATOR_PAUSE.len()..].trim_end_matches(PAUSE_END).to_owned();
            pauses.push((p, resume, i, reason));
        }
    }
    let mut logged_lines: Vec<(Option<usize>, String)> = Vec::new();
    for (n, (at, message, id)) in rows.iter().enumerate() {
        shown.push(("notice", *id));
        happened = true;
        let step = |what: &str| format!("{what} {at}");
        let own = own_at.iter().position(|o| *o == Some(n));
        let withdrawn = hand_overs.iter().position(|h| message.starts_with(&format!("Engine build {} withdrawn: handed over to build {}: ", h.from, h.to)));
        let admitted = hand_overs.iter().position(|h| message.starts_with(&format!("Engine build {} admitted: ", h.to)));
        let paused = pauses.iter().find(|p| p.0 == n);
        let resumed = pauses.iter().find(|p| p.1 == Some(n));
        // a planned step's line of this hand-over (its daily runs) or of its pause
        let planned = message.starts_with(watch::PLANNED)
            && (hand_overs.iter().any(|h| message.contains(&format!("(hand-over {}, with build ", h.id)))
                || pauses.iter().any(|p| message.contains("Work queue paused by the operator") && message.contains(one_line(&p.3, 40).trim_end_matches('…'))));
        if let Some(i) = own {
            placed[i] = Some(at.clone());
            logged_lines.push((Some(i), String::new()));
        } else if withdrawn.is_some() {
            // its own withdrawal's stopped jobs are the line's "restarted"
        } else if let Some(i) = admitted {
            let to = build_words(&hand_overs[i].to, &hand_overs[i].from);
            hand_overs[i].folded.push(step(&format!("{to} admitted")));
        } else if let Some((_, _, i, reason)) = paused {
            // the pause's reason, unless it is exactly the hand-over's own (round 3, audit c072-r2,
            // 6: a reason that only contains it says more, so it is printed)
            let words = if *reason == hand_overs[*i].reason { String::new() } else { format!(" ({})", one_line(reason, REASON_MAX)) };
            hand_overs[*i].folded.push(step(&format!("queue paused{words}")));
        } else if let Some((_, _, i, _)) = resumed {
            hand_overs[*i].folded.push(step("queue resumed"));
        } else if planned {
            // said by its hand-over's line
        } else {
            logged_lines.push((None, format!("{at} {}", flat(message))));
        }
    }
    // a hand-over whose own notice was stated before (or not at all) still has its line
    for (i, at) in placed.iter().enumerate() {
        if at.is_none() {
            logged_lines.push((Some(i), String::new()));
        }
    }
    for (i, line) in logged_lines {
        lines.push(match i {
            None => line,
            Some(i) => {
                let line = hand_overs[i].line();
                placed[i].as_ref().map_or(line.clone(), |at| format!("{at} {line}"))
            }
        });
        happened = true;
    }

    // the queue's counts by kind, for the builds that run (a tape verify job is not a tape); a job
    // done in the window is one DONE now and finished in it, or one whose lease ended DONE in it
    // (audit round 2 of core-watch, 12: an ordered pass queued again by a later publish has no
    // `finished_at`, yet it was done in the window)
    let kinds = c
        .query(
            "SELECT kind, count(*) FILTER (WHERE (state = 'DONE' AND finished_at > $1::text::timestamptz AND finished_at <= $2::text::timestamptz) OR w.job_id IS NOT NULL),
                    count(*) FILTER (WHERE state = 'DONE'), count(*) FILTER (WHERE state = 'LEASED'), count(*) FILTER (WHERE state = 'QUEUED'),
                    count(*) FILTER (WHERE state = 'FAILED'), count(*)
             FROM core.job
             LEFT JOIN (SELECT DISTINCT job_id FROM core.lease WHERE outcome = 'DONE' AND ended_at > $1::text::timestamptz AND ended_at <= $2::text::timestamptz) w USING (job_id)
             WHERE state <> 'CANCELLED' AND core.build_runs(engine_build) AND NOT COALESCE(scope @> '{\"verify\": true}', false)
             GROUP BY kind ORDER BY CASE kind WHEN 'TAPE' THEN 0 WHEN 'DAY_BLOCK' THEN 1 ELSE 2 END",
            &[&start, &end],
        )
        .await
        .map_err(db)?;
    for r in &kinds {
        let (kind, now_done, done, running, waiting, failed, all): (String, i64, i64, i64, i64, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
        if now_done == 0 && running == 0 && waiting == 0 && failed == 0 {
            continue;
        }
        happened |= now_done > 0;
        let mut parts = vec![format!("{} done in this window", thousands(now_done)), format!("{} of {} done", thousands(done), thousands(all))];
        for (n, w) in [(running, "running"), (waiting, "waiting"), (failed, "failed")] {
            if n > 0 {
                parts.push(format!("{} {w}", thousands(n)));
            }
        }
        let words = kind_words(&kind);
        let mut name = words.to_string();
        name[..1].make_ascii_uppercase();
        lines.push(format!("{name}: {}", parts.join(" · ")));
    }
    Ok(if happened { (lines, shown) } else { (Vec::new(), Vec::new()) })
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
    let len = |s: &str| s.chars().count();
    let tail = |n: usize| format!("\n… and {n} more lines (`vyreon-core log 100`, `vyreon-core queue`)");
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
                return posts;
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
    posts
}

/// The posts of the last `WINDOW_MINUTES` before `now`, as the progress channel would carry them,
/// queued nowhere (`vyreon-core progress`): to read what the channel will say before it is on.
pub async fn preview(c: &mut impl GenericClient, profile: &str, now: &str) -> Result<Vec<String>, String> {
    let r = c
        .query_one(
            "SELECT ($1::text::timestamptz - make_interval(secs => $2::float8 * 60))::text,
                    to_char(($1::text::timestamptz - make_interval(secs => $2::float8 * 60)) AT TIME ZONE $3, 'Dy Mon FMDD HH24:MI') || '–'
                        || to_char($1::text::timestamptz AT TIME ZONE $3, 'HH24:MI') || ' MT'",
            &[&now, &WINDOW_MINUTES, &ZONE],
        )
        .await
        .map_err(db)?;
    let (start, span): (String, String) = (r.get(0), r.get(1));
    let lines = lines(c, profile, &start, now).await?;
    Ok(if lines.is_empty() { Vec::new() } else { split(&format!("Progress · {span}"), &lines) })
}

/// When the window since the last one (or, the first time, the last `WINDOW_MINUTES`) is
/// `WINDOW_MINUTES` long at `now`, composes it and queues its posts with it, in one transaction;
/// a window with nothing in it is recorded with no post. None when no window is due.
pub async fn tick(client: &mut Client, profile: &str, now: &str) -> Result<Option<Window>, String> {
    let r = client
        .query_one(
            "WITH w AS (SELECT COALESCE((SELECT max(window_end) FROM core.progress_window WHERE window_end <= $1::text::timestamptz),
                                        $1::text::timestamptz - make_interval(secs => $2::float8 * 60)) AS start)
             SELECT w.start::text, $1::text::timestamptz - w.start >= make_interval(secs => $2::float8 * 60),
                    to_char(w.start AT TIME ZONE $3, 'Dy Mon FMDD HH24:MI') || '–' || to_char($1::text::timestamptz AT TIME ZONE $3, 'HH24:MI') || ' MT'
             FROM w",
            &[&now, &WINDOW_MINUTES, &ZONE],
        )
        .await
        .map_err(db)?;
    let (start, due, span): (String, bool, String) = (r.get(0), r.get(1), r.get(2));
    if !due {
        return Ok(None);
    }
    let (lines, shown) = compose(client, profile, &start, now).await?;
    let posts = if lines.is_empty() { Vec::new() } else { split(&format!("Progress · {span}"), &lines) };
    let tx = client.transaction().await.map_err(db)?;
    let n = tx
        .execute(
            "INSERT INTO core.progress_window (window_start, window_end, lines, posts) VALUES ($1::text::timestamptz, $2::text::timestamptz, $3, $4)
             ON CONFLICT (window_end) DO NOTHING",
            &[&start, &now, &(lines.len() as i32), &(posts.len() as i32)],
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
    Ok(Some(Window { start, end: now.to_owned(), lines: lines.len(), posts: posts.len() }))
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

    /// The day line says the slots, those with a value, the withheld by reason, the check, the
    /// minutes and the block: the counts are the real 2025-12-16 report's (B0 census) and the real
    /// slice's AAPL day; the minutes are made up for the test.
    #[test]
    fn a_published_day_reads_in_one_line() {
        let t = watch::Tally { slots: 3904, withheld: 294, provider_withheld: 290, engine_faults: 6 };
        let check = json!({"outcome": "COMPLETED", "checked": 91066, "failed": 3958, "unexplained": null, "why": null});
        assert_eq!(
            day_line("Tue Dec 16", t, Some(&check), Some(212.4), Some(("2025-12-15", "2025-12-19", "day-7f2c", 53))),
            "Published Tue Dec 16: 3,904 slots, 3,610 with a value, withheld: 290 provider input, 4 engine fault · reference check: 3,958 failures of 91,066, not triaged against rulings · 212 min · block 2025-12-15 to 2025-12-19 on day-7f2c (job 53)"
        );
        let clean = watch::Tally { slots: 3904, withheld: 0, provider_withheld: 0, engine_faults: 0 };
        assert_eq!(day_line("Tue Dec 16", clean, None, None, None), "Published Tue Dec 16: 3,904 slots, 3,904 with a value, none withheld · reference check not in yet");
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
    /// withdrawal stopped, its reason cut, its window's steps folded); the counts are hand-over 26's
    /// shape (blocks and passes), its reason the operator's words that night, cut.
    #[test]
    fn a_hand_over_reads_in_one_line() {
        let reason = "D-460 r3 fixes: UPATH bin-0 false zero removed on every new day; strictly better than fc36eefb on wrong numbers";
        let folded = ["queue paused 04:28".to_string(), "2dde1132 admitted 04:39".to_string(), "queue resumed 04:41".to_string()];
        let h = |id: i64, from: &str, to: &str, moved: [i64; 3], verify_cancelled: i64, restarted: i64, reason: &str, folded: &[String]| {
            HandOver { id, from: from.into(), to: to.into(), reason: reason.into(), moved, verify_cancelled, restarted, folded: folded.to_vec(), admitted: true }.line()
        };
        let line = h(26, "fc36eefb+b1c8d00b5d79", "2dde1132+5f8d87f49bfc", [0, 42, 593], 0, 8, reason, &folded);
        assert_eq!(
            line,
            "Hand-over 26: fc36eefb → 2dde1132, 42 blocks + 593 passes moved, 8 running jobs restarted — D-460 r3 fixes: UPATH bin-0 false zero removed on every new day; strictly bette… · queue paused 04:28 · 2dde1132 admitted 04:39 · queue resumed 04:41"
        );
        // two images of one engine commit are named whole; one of each, nothing restarted
        assert_eq!(
            h(3, "2dde1132+aaa", "2dde1132+bbb", [1, 1, 1], 2, 0, "image rebuilt", &[]),
            "Hand-over 3: 2dde1132+aaa → 2dde1132+bbb, 1 tape + 1 block + 1 pass moved, 2 tape verify jobs cancelled — image rebuilt"
        );
        assert_eq!(h(4, "a", "b", [0, 0, 0], 0, 1, "x", &[]), "Hand-over 4: a → b, nothing moved, 1 running job restarted — x");
        // audit c072, 2: a successor not admitted keeps the hand-over's instruction
        let waiting = HandOver { id: 5, from: "a+1".into(), to: "b+2".into(), reason: "x".into(), moved: [0, 3, 0], admitted: false, ..HandOver::default() };
        assert_eq!(waiting.line(), "Hand-over 5: a → b, 3 blocks moved — x · build b+2 is not admitted: admit it now (`vyreon-core build admit b+2 <evidence.json>`)");
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

    /// Posts stay under Discord's limit, continue under the header, stop at `MAX_POSTS`, and count
    /// what they leave out.
    #[test]
    fn a_long_window_is_split_and_capped() {
        let line = |i: usize| format!("{i:03} Day block 2025-12-15 to 2025-12-19 done by day-7f2c (40 blocks to go) {}", "·".repeat(60));
        let short: Vec<String> = (0..3).map(line).collect();
        let one = split("Progress · Sat Nov 22 14:00–14:30 MT", &short);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].lines().count(), 4);
        let long: Vec<String> = (0..200).map(line).collect();
        let posts = split("Progress · Sat Nov 22 14:00–14:30 MT", &long);
        assert_eq!(posts.len(), MAX_POSTS);
        assert!(posts.iter().all(|p| p.chars().count() <= LIMIT), "{:?}", posts.iter().map(|p| p.chars().count()).collect::<Vec<_>>());
        assert!(posts[1].starts_with("Progress · Sat Nov 22 14:00–14:30 MT (continued)\n"));
        let shown: usize = posts.iter().map(|p| p.lines().filter(|l| l.contains("Day block")).count()).sum();
        let tail = posts[2].lines().last().unwrap();
        assert_eq!(tail, format!("… and {} more lines (`vyreon-core log 100`, `vyreon-core queue`)", 200 - shown));
    }

    /// Audit round 2 of core-watch, 14: a line longer than a post made a post of the header alone,
    /// then a post over the limit. Now every post carries a line, stays under the limit, and a line
    /// cut to fit says how much was cut.
    #[test]
    fn a_line_longer_than_a_post_is_cut_and_counted() {
        let header = "Progress · Sat Nov 22 14:00–14:30 MT";
        let long = format!("Started block 2025-12-15 to 2025-12-19 on {} (job 53, attempt 1)", "x".repeat(2500));
        let lines = vec!["14:02 Queue resumed.".to_string(), long.clone(), "14:20 Queue resumed.".to_string()];
        let posts = split(header, &lines);
        assert!(posts.iter().all(|p| p.chars().count() <= LIMIT), "{:?}", posts.iter().map(|p| p.chars().count()).collect::<Vec<_>>());
        assert!(posts.iter().all(|p| p.lines().count() >= 2), "no post of the header alone");
        assert_eq!(posts.len(), 3);
        let cut = posts[1].lines().nth(1).unwrap();
        let n: usize = cut.rsplit("… (").next().unwrap().trim_end_matches(" characters cut)").replace(',', "").parse().unwrap();
        assert_eq!(cut.chars().count() - format!("… ({} characters cut)", thousands(n as i64)).chars().count() + n, long.chars().count(), "what is kept and what is counted make the line");
        assert_eq!(posts[2], format!("{header} (continued)\n14:20 Queue resumed."));
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit(&"y".repeat(30), 25), format!("{}… (26 characters cut)", "y".repeat(4)));
    }
}
