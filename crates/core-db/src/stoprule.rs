//! C-084: the owner's phase-1 stop rule (docs/MILESTONES.md, "PHASE-1 RUN RULE", 2026-11-25), the part core
//! counts; C-090 (owner, 2026-11-26): it stops only for SYSTEMATIC breakage. For the newest running build:
//!   * days not published after a failure: an attempt of the build failed on the day and the build has not
//!     published it (a failed lease of its day block naming the day, after the build's admission; an
//!     input-plan refusal is the provider's inputs, C-075's DAY_REFUSED, not counted). From the build's
//!     `SAMPLE_DAYS`th published day, past half of the days it attempted (published or not published after a
//!     failure) raises STOP_RULE;
//!   * days with a whole measurement faulted: on a published day, one measurement withheld for the engine's
//!     stated reason on every object it has (`core.revision_tally.whole_faults`, C-090; as M0-ANALYTICS and
//!     INT-IVSC01 on all ~62 assets). One such day raises STOP_RULE.
//!
//! Local value errors are counted and listed for the fix pass, and raise nothing: published days whose
//! current reference check completed with unexplained failures (`failed_unexplained`, or `failed` from a
//! suite without rulings), and published days with engine-fault slots that are not a whole measurement.
//! Core's count is an upper bound: whether a failure is the engine's and not the provider's is the
//! coordinator's classification, verified before stopping; the alert says so.
use tokio_postgres::GenericClient;

/// The rule's sample: failures to publish are judged once the build has published this many days.
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
    /// Days an attempt of the build failed on and the build has not published (systematic past half).
    pub unpublished: i64,
    /// Published days with a measurement withheld for an engine fault on every object (systematic).
    pub systematic: i64,
    /// Published days with engine-fault slots, none a whole measurement (local: the fix pass's list).
    pub faulted: i64,
    /// Published days whose current reference check completed.
    pub checked: i64,
    /// Of those, the days with unexplained failures (local: the fix pass's list).
    pub flagged: i64,
    /// The oldest day that counts toward the stop (not published, or a whole measurement faulted).
    pub oldest: Option<String>,
}

impl Tally {
    /// The days the build attempted: published, or not published after a failure.
    pub fn attempted(&self) -> i64 {
        self.published + self.unpublished
    }
    fn percent(&self) -> i64 {
        if self.attempted() == 0 { 0 } else { (self.unpublished * 100 + self.attempted() / 2) / self.attempted() }
    }
    /// Past the rule's line: a whole measurement faulted on any day, or from the sample on, more than half
    /// of the attempted days not published after a failure.
    pub fn over(&self) -> bool {
        self.systematic > 0 || (self.published >= SAMPLE_DAYS && self.unpublished * 2 > self.attempted())
    }
    fn local(&self) -> String {
        format!("for the fix pass: {} days with single-asset engine faults, {} with unexplained reference failures", self.faulted, self.flagged)
    }
    /// The progress channel's line (None until the build attempted a day).
    pub fn line(&self) -> Option<String> {
        if self.attempted() == 0 {
            return None;
        }
        let whole = format!("{} with a whole measurement faulted", self.systematic);
        if self.published < SAMPLE_DAYS {
            return Some(format!(
                "Stop rule: so far {} of {} days not published after a failure, {whole}; {}; it judges failures from the {SAMPLE_DAYS}th published day ({} so far)",
                self.unpublished,
                self.attempted(),
                self.local(),
                self.published
            ));
        }
        Some(format!(
            "Stop rule: {} of {} days not published after a failure ({}%), {whole}; {} · the alert is at over half not published, or any whole measurement faulted",
            self.unpublished,
            self.attempted(),
            self.percent(),
            self.local()
        ))
    }
    /// The alert's summary, past the line.
    pub fn summary(&self) -> Option<String> {
        self.over().then(|| {
            let mut why = Vec::new();
            if self.systematic > 0 {
                why.push(format!("{} days with a measurement withheld for an engine fault on every asset", self.systematic));
            }
            if self.published >= SAMPLE_DAYS && self.unpublished * 2 > self.attempted() {
                why.push(format!("{} of {} days not published after a failure ({}%), over half", self.unpublished, self.attempted(), self.percent()));
            }
            format!(
                "Phase-1 stop rule, systematic breakage: {}{}. This is core's count only: that each is the engine's and not the provider's is the coordinator's classification, verified before stopping",
                why.join("; "),
                self.oldest.as_deref().map(|d| format!(", the oldest {d}")).unwrap_or_default()
            )
        })
    }
}

/// The days of `build` the rule counts, oldest first; `$1` the profile, `$2` the build. The columns: day, kind
/// ('unpublished', 'systematic', 'faulted', 'flagged', 'good' or 'published'), detail.
const BAD_DAYS: &str = "
    WITH published AS (
        SELECT d.represented_date AS day, d.revision_id AS base, l.revision_id AS local
        FROM vyreon.day_revision d
        JOIN LATERAL (SELECT l.revision_id FROM vyreon.day_revision l
                      WHERE l.base_revision = d.revision_id AND l.day_local AND l.state = 'PUBLISHED' ORDER BY l.revision_id DESC LIMIT 1) l ON true
        WHERE d.profile_id = $1 AND d.engine_build = $2 AND d.state = 'PUBLISHED' AND d.base_revision IS NULL AND d.republish_scope IS NULL),
    faults AS (
        SELECT p.day, COALESCE(tb.whole_faults, '{}') || COALESCE(tl.whole_faults, '{}') AS whole,
               COALESCE(tb.engine_faults, 0) + COALESCE(tl.engine_faults, 0) AS slots
        FROM published p LEFT JOIN core.revision_tally tb ON tb.revision_id = p.base LEFT JOIN core.revision_tally tl ON tl.revision_id = p.local),
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
    SELECT f.day::text, 'systematic', array_to_string(f.whole, ', ') FROM faults f WHERE cardinality(f.whole) > 0
    UNION ALL
    SELECT f.day::text, 'faulted', f.slots::text FROM faults f WHERE cardinality(f.whole) = 0 AND f.slots > 0
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

/// `build`'s count and the days it counts, oldest first: those toward the stop (`stop`: "not published:
/// <failure>", "a whole measurement faulted: <ids>") and the local ones for the fix pass (`local`: "N
/// single-asset engine-fault slots", "N unexplained reference failures").
pub async fn days(c: &impl GenericClient, profile: &str, build: &str) -> Result<(Tally, Days), String> {
    let rows = c.query(BAD_DAYS, &[&profile, &build]).await.map_err(db)?;
    let mut t = Tally { build: build.to_owned(), ..Tally::default() };
    let mut days = Days::default();
    for r in &rows {
        let (day, kind, detail): (String, &str, Option<String>) = (r.get(0), r.get(1), r.get(2));
        let detail = detail.unwrap_or_default();
        match kind {
            "published" => t.published += 1,
            "good" => t.checked += 1,
            "flagged" => {
                t.checked += 1;
                t.flagged += 1;
                days.local.push((day, format!("{detail} unexplained reference failures")));
            }
            "faulted" => {
                t.faulted += 1;
                days.local.push((day, format!("{detail} single-asset engine-fault slots")));
            }
            "systematic" => {
                t.systematic += 1;
                days.stop.push((day, format!("a whole measurement faulted on every asset: {detail}")));
            }
            _ => {
                t.unpublished += 1;
                days.stop.push((day, format!("not published: {}", crate::watch::plain_cause(&detail))));
            }
        }
    }
    days.stop.sort();
    days.local.sort();
    t.oldest = days.stop.first().map(|(d, _)| d.clone());
    Ok((t, days))
}

/// The days a build's count names: toward the stop, and local (the fix pass's list), each (day, why).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Days {
    pub stop: Vec<(String, String)>,
    pub local: Vec<(String, String)>,
}

/// The newest running build's count (None without one).
pub async fn tally(c: &impl GenericClient, profile: &str) -> Result<Option<Tally>, String> {
    let Some(build) = newest_build(c).await? else { return Ok(None) };
    Ok(Some(days(c, profile, &build).await?.0))
}
