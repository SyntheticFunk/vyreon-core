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
//!   * each day block started (a lease granted), with its worker;
//!   * every line core logged (`notice::log`): blocks done, days set aside with their cause, dates
//!     omitted, builds admitted or withdrawn, retirements, the queue paused or resumed;
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
/// Progress posts not delivered within this many hours are not sent (`notice::drop_stale_progress`).
pub const STALE_HOURS: f64 = 6.0;

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

/// A logged line as a post holds it: one line (its own line breaks as " / "), cut to `LINE_MAX`,
/// and never one a credential pattern matches.
fn flat(text: &str) -> String {
    let joined = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" / ");
    one_line(&joined, LINE_MAX)
}

/// The window's lines, in sections: days published, reference checks come in, blocks started, the
/// log, the queue's counts. Empty when nothing happened in it (the counts alone are not news).
pub async fn lines(c: &impl GenericClient, profile: &str, start: &str, end: &str) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let mut happened = false;
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
                            WHERE l.day_local AND l.profile_id = $3 AND l.published_at > $1::text::timestamptz AND l.published_at <= $2::text::timestamptz)
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
                words = day("d.day")
            ),
            &[&start, &end, &profile],
        )
        .await
        .map_err(db)?;
    // the reports a day line already states (C-054: a report is of the base and the day-local
    // revision it checked, both: audit round 2 of core-watch, 2)
    let mut stated: Vec<i64> = Vec::new();
    if !days.is_empty() {
        let revisions: Vec<i64> = days.iter().flat_map(|r| [r.get::<_, i64>(1), r.get::<_, i64>(2)]).collect();
        watch::tally(c, &revisions).await?;
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
                        r.base_revision, r.day_local_revision
                 FROM core.refcheck_report r JOIN vyreon.day_revision b ON b.revision_id = r.base_revision
                 WHERE b.profile_id = $3 AND r.stored_at > $1::text::timestamptz AND r.stored_at <= $2::text::timestamptz AND NOT r.refcheck_report_id = ANY($4)
                 ORDER BY r.stored_at, r.refcheck_report_id",
                words = day("r.session")
            ),
            &[&start, &end, &profile, &stated],
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
        happened = true;
    }

    // day blocks started: a lease granted, with its worker
    let started = c
        .query(
            "SELECT j.first_session::text, j.last_session::text, l.worker, l.job_id, l.start_session::text,
                    (SELECT count(*) FROM core.lease p WHERE p.job_id = l.job_id AND p.lease_id <= l.lease_id)
             FROM core.lease l JOIN core.job j USING (job_id)
             WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $3 AND l.granted_at > $1::text::timestamptz AND l.granted_at <= $2::text::timestamptz
             ORDER BY l.granted_at, l.lease_id",
            &[&start, &end, &profile],
        )
        .await
        .map_err(db)?;
    for r in &started {
        let (first, last, worker, job, from, attempt): (String, String, String, i64, Option<String>, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        let span = if first == last { first.clone() } else { format!("{first} to {last}") };
        let from = from.filter(|f| *f != first).map_or(String::new(), |f| format!(", from {f}"));
        lines.push(format!("Started block {span} on {worker} (job {job}, attempt {attempt}{from})"));
        happened = true;
    }

    // everything core logged in the window, as it logged it
    let logged = c
        .query(
            "SELECT to_char(created_at AT TIME ZONE $3, 'HH24:MI'), message FROM core.notice
             WHERE NOT post AND created_at > $1::text::timestamptz AND created_at <= $2::text::timestamptz ORDER BY notice_id",
            &[&start, &end, &ZONE],
        )
        .await
        .map_err(db)?;
    for r in &logged {
        let (at, message): (String, String) = (r.get(0), r.get(1));
        lines.push(format!("{at} {}", flat(&message)));
        happened = true;
    }

    // the queue's counts by kind, for the builds that run (a tape verify job is not a tape)
    let kinds = c
        .query(
            "SELECT kind, count(*) FILTER (WHERE state = 'DONE' AND finished_at > $1::text::timestamptz AND finished_at <= $2::text::timestamptz),
                    count(*) FILTER (WHERE state = 'DONE'), count(*) FILTER (WHERE state = 'LEASED'), count(*) FILTER (WHERE state = 'QUEUED'),
                    count(*) FILTER (WHERE state = 'FAILED'), count(*)
             FROM core.job WHERE state <> 'CANCELLED' AND core.build_runs(engine_build) AND NOT COALESCE(scope @> '{\"verify\": true}', false)
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
    Ok(if happened { lines } else { Vec::new() })
}

/// The window's posts: the header, then the lines, each post at most `LIMIT` characters, at most
/// `MAX_POSTS`; the lines that do not fit are counted on the last.
pub fn split(header: &str, lines: &[String]) -> Vec<String> {
    let mut posts: Vec<String> = Vec::new();
    let mut post = header.to_string();
    for (i, line) in lines.iter().enumerate() {
        let last_post = posts.len() + 1 == MAX_POSTS;
        let rest = lines.len() - i - 1;
        let tail = |n: usize| format!("\n… and {n} more lines (`vyreon-core log 100`, `vyreon-core queue`)");
        let room = if last_post && rest > 0 { tail(rest).chars().count() } else { 0 };
        if post.chars().count() + 1 + line.chars().count() + room > LIMIT {
            if last_post {
                post.push_str(&tail(rest + 1));
                posts.push(post);
                return posts;
            }
            posts.push(post);
            post = format!("{header} (continued)");
        }
        post.push('\n');
        post.push_str(line);
    }
    posts.push(post);
    posts
}

/// The posts of the last `WINDOW_MINUTES` before `now`, as the progress channel would carry them,
/// queued nowhere (`vyreon-core progress`): to read what the channel will say before it is on.
pub async fn preview(c: &impl GenericClient, profile: &str, now: &str) -> Result<Vec<String>, String> {
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
    let lines = lines(&*client, profile, &start, now).await?;
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
    for (i, p) in posts.iter().enumerate() {
        notice::progress(&tx, &format!("progress {now} {}", i + 1), p).await?;
    }
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
}
