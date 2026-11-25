//! C-084: the owner's phase-1 stop rule (docs/MILESTONES.md, "PHASE-1 RUN RULE", 2026-11-25), the part core
//! counts. Over the first `SAMPLE_DAYS` published days of the newest running build, and then continuously, a
//! day looks bad when
//!   * an attempt of the build failed on it and the build has not published it (a failed lease of its day
//!     block naming the day, after the build's admission; an input-plan refusal is the provider's inputs,
//!     not counted: C-075's DAY_REFUSED says it), or
//!   * its current reference check completed with unexplained failures (`failed_unexplained`, or `failed`
//!     from a suite without rulings).
//!
//! The judged days are those and the published days whose current check completed. Past half of them bad,
//! the watch raises STOP_RULE. Core's count is an upper bound: whether a failure or a disagreement is an
//! engine fault that changes a published value (the rule's bad day) and not the suite's or the provider's is
//! the coordinator's classification, verified before stopping; the alert says so.
use tokio_postgres::GenericClient;

/// The rule's sample: it judges once the build has published this many days.
pub const SAMPLE_DAYS: i64 = 20;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The count for one build.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tally {
    pub build: String,
    /// Whole days the build computed and published (its base and a day-local revision on it; a
    /// republish's copy-forward is not the build's own computation).
    pub published: i64,
    /// Of those, the days whose current reference check completed.
    pub checked: i64,
    /// Of those, the days with unexplained failures.
    pub flagged: i64,
    /// Days an attempt of the build failed on and the build has not published.
    pub unpublished: i64,
    /// The oldest bad day.
    pub oldest: Option<String>,
}

impl Tally {
    pub fn judged(&self) -> i64 {
        self.checked + self.unpublished
    }
    pub fn bad(&self) -> i64 {
        self.flagged + self.unpublished
    }
    fn percent(&self) -> i64 {
        if self.judged() == 0 { 0 } else { (self.bad() * 100 + self.judged() / 2) / self.judged() }
    }
    /// Past the rule's line: the sample reached, and more than half of the judged days bad.
    pub fn over(&self) -> bool {
        self.published >= SAMPLE_DAYS && self.bad() * 2 > self.judged()
    }
    fn counts(&self) -> String {
        format!("{} not published after a failure, {} with unexplained reference failures", self.unpublished, self.flagged)
    }
    /// The progress channel's line (None until a day is judged).
    pub fn line(&self) -> Option<String> {
        if self.judged() == 0 {
            return None;
        }
        if self.published < SAMPLE_DAYS {
            return Some(format!(
                "Stop rule: so far {} of {} judged days look bad ({}); it judges from the {SAMPLE_DAYS}th published day ({} so far)",
                self.bad(),
                self.judged(),
                self.counts(),
                self.published
            ));
        }
        Some(format!(
            "Stop rule: {} of {} judged days look bad ({}%): {} · the alert is at over half; engine, suite or provider is the coordinator's call",
            self.bad(),
            self.judged(),
            self.percent(),
            self.counts()
        ))
    }
    /// The alert's summary, past the line.
    pub fn summary(&self) -> Option<String> {
        self.over().then(|| {
            format!(
                "Phase-1 stop rule: {} of {} judged days look bad ({}%), over half: {}{}. This is core's count only: whether each is an engine fault that changes a published value, and not the suite's or the provider's, is the coordinator's classification, verified before stopping",
                self.bad(),
                self.judged(),
                self.percent(),
                self.counts(),
                self.oldest.as_deref().map(|d| format!(", the oldest {d}")).unwrap_or_default()
            )
        })
    }
}

/// The days of `build` that look bad (C-084's two counts), as (day, why), oldest first; `$1` the profile, `$2`
/// the build. The columns: day, kind ('unpublished' or 'flagged'), detail.
const BAD_DAYS: &str = "
    WITH published AS (
        SELECT d.represented_date AS day, d.revision_id AS base, l.revision_id AS local
        FROM vyreon.day_revision d
        JOIN LATERAL (SELECT l.revision_id FROM vyreon.day_revision l
                      WHERE l.base_revision = d.revision_id AND l.day_local AND l.state = 'PUBLISHED' ORDER BY l.revision_id DESC LIMIT 1) l ON true
        WHERE d.profile_id = $1 AND d.engine_build = $2 AND d.state = 'PUBLISHED' AND d.base_revision IS NULL AND d.republish_scope IS NULL),
    checks AS (
        SELECT DISTINCT ON (p.day) p.day, r.outcome, COALESCE(r.failed_unexplained, r.failed) AS unexplained
        FROM published p JOIN core.refcheck_report r ON r.base_revision = p.base AND (r.day_local_revision IS NULL OR r.day_local_revision = p.local)
        ORDER BY p.day, r.stored_at DESC, r.refcheck_report_id DESC),
    failed AS (
        SELECT COALESCE(substring(l.failed_stage FROM '^day ([0-9]{4}-[0-9]{2}-[0-9]{2})$')::date, l.start_session) AS day,
               max(l.failure) AS failure
        FROM core.lease l JOIN core.job j USING (job_id) JOIN core.engine_build e ON e.engine_build = j.engine_build
        WHERE j.kind = 'DAY_BLOCK' AND j.profile_id = $1 AND j.engine_build = $2 AND l.outcome = 'FAILED' AND l.granted_at >= e.admitted_at
          AND strpos(COALESCE(l.failure, ''), 'refused by the input plan') = 0
        GROUP BY 1)
    SELECT f.day::text, 'unpublished', f.failure FROM failed f WHERE f.day IS NOT NULL AND f.day NOT IN (SELECT day FROM published)
    UNION ALL
    SELECT c.day::text, 'flagged', c.unexplained::text FROM checks c WHERE c.outcome = 'COMPLETED' AND c.unexplained > 0
    UNION ALL
    SELECT c.day::text, 'good', NULL FROM checks c WHERE c.outcome = 'COMPLETED' AND c.unexplained = 0
    UNION ALL
    SELECT p.day::text, 'published', NULL FROM published p
    ORDER BY 1, 2";

/// The newest running build (admitted last, not withdrawn), whose days the rule counts.
pub async fn newest_build(c: &impl GenericClient) -> Result<Option<String>, String> {
    Ok(c.query_opt("SELECT engine_build FROM core.engine_build WHERE core.build_runs(engine_build) ORDER BY admitted_at DESC, engine_build LIMIT 1", &[])
        .await
        .map_err(db)?
        .map(|r| r.get(0)))
}

/// `build`'s count and its bad days ((day, "not published: <failure>" or "N unexplained reference failures")).
pub async fn days(c: &impl GenericClient, profile: &str, build: &str) -> Result<(Tally, Vec<(String, String)>), String> {
    let rows = c.query(BAD_DAYS, &[&profile, &build]).await.map_err(db)?;
    let mut t = Tally { build: build.to_owned(), ..Tally::default() };
    let mut bad = Vec::new();
    for r in &rows {
        let (day, kind, detail): (String, &str, Option<String>) = (r.get(0), r.get(1), r.get(2));
        match kind {
            "published" => t.published += 1,
            "good" => t.checked += 1,
            "flagged" => {
                t.checked += 1;
                t.flagged += 1;
                bad.push((day, format!("{} unexplained reference failures", detail.unwrap_or_default())));
            }
            _ => {
                t.unpublished += 1;
                bad.push((day, format!("not published: {}", crate::watch::plain_cause(&detail.unwrap_or_default()))));
            }
        }
    }
    bad.sort();
    t.oldest = bad.first().map(|(d, _)| d.clone());
    Ok((t, bad))
}

/// The newest running build's count (None without one).
pub async fn tally(c: &impl GenericClient, profile: &str) -> Result<Option<Tally>, String> {
    let Some(build) = newest_build(c).await? else { return Ok(None) };
    Ok(Some(days(c, profile, &build).await?.0))
}
