//! C-097 (owner, 2026-11-26: "In general, you shouldn't have to go looking"): the run states its own timings
//! and withholdings. It only reads what is stored and writes its own counts (`core.withheld_tally`,
//! `core.withheld_counted`): what each published revision withholds, counted once in bounded batches by the
//! watch; a published day's flags for the progress channel (slow against its build's usual day, idle for the
//! captures' cutoffs, refused by the provider, a withholding on many more assets than usual); the digest's
//! "run" lines; and the timings table. The thresholds are ours, to be tuned on the run's first days. Nothing
//! here schedules, leases or publishes, and a failure of it is logged and never stops a post.
use std::time::Instant;
use tokio_postgres::GenericClient;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The revisions one counting statement reads (each is a day's bodies; 2026-11-25's three took 5.9 s on
/// production once every schema's withholdings were read, C-098 9, the base alone 1.6 s; the statement's limit is
/// `watch::TALLY_LIMITS`' 60 s).
pub const COUNT_BATCH: usize = 3;
/// A day is slow when its compute is over this many times its build's usual day.
pub const SLOW_FACTOR: f64 = 2.0;
/// The usual is the median over at most this many earlier days, and needs at least `USUAL_MIN`.
pub const USUAL_DAYS: i64 = 20;
pub const USUAL_MIN: i64 = 5;
/// The download idled when it slept longer than this for the captures' cutoffs (seconds).
pub const IDLE_SECONDS: f64 = 900.0;
/// A withholding is unusual when it is stated for this many more assets than usual.
pub const UNUSUAL_ASSETS: i64 = 16;
/// At most this many unusual withholdings are named on a day's line.
pub const UNUSUAL_MAX: usize = 2;
/// A day's line waits this long for its counts before it is composed without them (minutes).
pub const FLAG_WAIT_MINUTES: f64 = 30.0;

/// A day's compute seconds: the sum of its report's stages (`d` names a `vyreon.day_revision` row).
fn compute_of(d: &str) -> String {
    format!(
        "(SELECT sum((s->>'seconds')::float8) FROM jsonb_array_elements(CASE WHEN jsonb_typeof({d}.report->'stages') = 'array' THEN {d}.report->'stages' ELSE '[]'::jsonb END) s
          WHERE jsonb_typeof(s->'seconds') = 'number')"
    )
}

/// A day's download seconds: the sum of its report's `fill_seconds` by category.
fn download_of(d: &str) -> String {
    format!(
        "(SELECT sum(e.value::text::float8) FROM jsonb_each(CASE WHEN jsonb_typeof({d}.report->'fill_seconds') = 'object' THEN {d}.report->'fill_seconds' ELSE '{{}}'::jsonb END) e
          WHERE jsonb_typeof(e.value) = 'number')"
    )
}

/// The provider's rate refusals (HTTP 429) a report states for its block so far; NULL when it states none.
fn refusals_of(d: &str) -> String {
    format!(
        "(SELECT sum(e.value::text::float8)::bigint FROM jsonb_each(CASE WHEN jsonb_typeof({d}.report->'block_refusals_so_far') = 'object' THEN {d}.report->'block_refusals_so_far' ELSE '{{}}'::jsonb END) e
          WHERE jsonb_typeof(e.value) = 'number' AND e.key LIKE '% 429')"
    )
}

/// Counts what the published revisions of `profile` not yet counted withhold, the newest first, in statements of
/// `COUNT_BATCH` revisions, each committed on its own under the watch's limits. With `until`, it stops after the
/// first batch that ends past it. True when none is left.
pub async fn count_uncounted(c: &mut impl GenericClient, profile: &str, now: &str, until: Option<Instant>) -> Result<bool, String> {
    loop {
        let tx = c.transaction().await.map_err(db)?;
        tx.batch_execute(crate::watch::TALLY_LIMITS).await.map_err(db)?;
        let next: Vec<i64> = tx
            .query(
                "SELECT d.revision_id FROM vyreon.day_revision d WHERE d.profile_id = $2 AND d.published_at <= $1::text::timestamptz
                   AND NOT EXISTS (SELECT FROM core.withheld_counted w WHERE w.revision_id = d.revision_id)
                 ORDER BY d.revision_id DESC LIMIT $3",
                &[&now, &profile, &(COUNT_BATCH as i64 + 1)],
            )
            .await
            .map_err(db)?
            .iter()
            .map(|r| r.get(0))
            .collect();
        let batch: Vec<i64> = next.iter().copied().take(COUNT_BATCH).collect();
        if !batch.is_empty() {
            tx.execute(
                &format!(
                    "INSERT INTO core.withheld_tally (revision_id, measurement_id, grain, reason, n, objects)
                     WITH m AS (SELECT m.revision_id, m.measurement_id, m.object_id, m.represented_date, m.body AS b
                                FROM vyreon.measurement m WHERE m.revision_id = ANY($1)),
                          r AS ({})
                     SELECT revision_id, measurement_id, grain, reason, count(*)::int, array_agg(DISTINCT object_id ORDER BY object_id) FROM r GROUP BY 1, 2, 3, 4",
                    crate::census::stated("m")
                ),
                &[&batch],
            )
            .await
            .map_err(db)?;
            tx.execute("INSERT INTO core.withheld_counted (revision_id) SELECT unnest($1::bigint[])", &[&batch]).await.map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        if next.len() <= COUNT_BATCH {
            return Ok(true);
        }
        if until.is_some_and(|t| Instant::now() >= t) {
            return Ok(false);
        }
    }
}

/// What is worth saying of a published day beyond its block's lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayFlags {
    /// Its compute and its build's usual, in hours, when over `SLOW_FACTOR` times the usual.
    pub slow: Option<(f64, f64)>,
    /// Minutes its download slept for the captures' cutoffs, when over `IDLE_SECONDS`.
    pub idled_minutes: Option<i64>,
    /// The provider's rate refusals: on this day (false), or in its block so far when there is no earlier day of
    /// the same worker to take off (true).
    pub refused: Option<(i64, bool)>,
    /// (measurement, reason, assets that day, usual assets), the largest excess first.
    pub unusual: Vec<(String, String, i64, i64)>,
}

impl DayFlags {
    pub fn is_empty(&self) -> bool {
        *self == DayFlags::default()
    }

    /// Each flag in plain words, in the order they are said.
    pub fn parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some((took, usual)) = self.slow {
            parts.push(format!("slow: compute {took:.1} h, usual {usual:.1} h"));
        }
        if let Some(minutes) = self.idled_minutes {
            parts.push(format!("idled {minutes} min waiting for captures"));
        }
        match self.refused {
            Some((n, false)) => parts.push(format!("provider refused {} requests", crate::progress::thousands(n))),
            Some((n, true)) => parts.push(format!("provider refused {} requests so far in its block", crate::progress::thousands(n))),
            None => {}
        }
        for (measurement, reason, assets, usual) in &self.unusual {
            parts.push(format!("unusual withholding: {} ({}) on {assets} assets (usual {usual})", said(measurement), reason_words(reason)));
        }
        parts
    }
}

/// A stated reason in lower-case words ("SOURCE_VOLUME_RECONCILIATION_FAILED": "source volume reconciliation failed").
pub fn reason_words(reason: &str) -> String {
    said(reason).replace('_', " ").to_lowercase()
}

/// C-098 (the class of audit 2026-11-26's core 1, read for in this unaudited module): a word a worker's body or
/// report supplied (a measurement, a reason, a stage's name) as a post says it: one line, at most 80
/// characters, nothing a credential pattern matches.
pub fn said(word: &str) -> String {
    crate::watch::one_line(word, 80)
}

/// The flags of the published day whose base revision is `base`, and whether its withholdings were counted
/// (the unusual ones are read only then).
pub async fn day_flags(c: &impl GenericClient, base: i64) -> Result<(DayFlags, bool), String> {
    let mut flags = DayFlags::default();
    let r = c
        .query_opt(
            &format!(
                "WITH d AS (SELECT d.*, {compute} AS compute, {refused} AS refused FROM vyreon.day_revision d WHERE d.revision_id = $1),
                      prior AS (SELECT {pcompute} AS compute FROM vyreon.day_revision p, d
                                WHERE p.base_revision IS NULL AND p.published_at IS NOT NULL AND p.profile_id = d.profile_id AND p.engine_build = d.engine_build
                                  AND p.published_at < d.published_at AND p.revision_id <> d.revision_id
                                ORDER BY p.published_at DESC LIMIT $2),
                      before AS (SELECT {prefused} AS refused FROM vyreon.day_revision p JOIN core.lease_revision pl USING (revision_id), d
                                 WHERE p.base_revision IS NULL AND p.published_at < d.published_at
                                   AND pl.lease_id = (SELECT lease_id FROM core.lease_revision WHERE revision_id = d.revision_id)
                                 ORDER BY p.published_at DESC LIMIT 1)
                 SELECT d.compute, (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY compute) FROM prior WHERE compute IS NOT NULL),
                        (SELECT count(*) FROM prior WHERE compute IS NOT NULL),
                        CASE WHEN jsonb_typeof(d.report->'fill'->'cutoff_wait_seconds') = 'number' THEN (d.report->'fill'->>'cutoff_wait_seconds')::float8 END,
                        d.refused, (SELECT count(*) FROM before), (SELECT refused FROM before)
                 FROM d",
                compute = compute_of("d"),
                refused = refusals_of("d"),
                pcompute = compute_of("p"),
                prefused = refusals_of("p"),
            ),
            &[&base, &USUAL_DAYS],
        )
        .await
        .map_err(db)?;
    let Some(r) = r else { return Ok((flags, false)) };
    let (compute, usual, days, idle, refused, earlier, before): (Option<f64>, Option<f64>, i64, Option<f64>, Option<i64>, i64, Option<i64>) =
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
    if let (Some(took), Some(usual)) = (compute, usual)
        && days >= USUAL_MIN
        && usual > 0.0
        && took > SLOW_FACTOR * usual
    {
        flags.slow = Some((took / 3600.0, usual / 3600.0));
    }
    if let Some(idle) = idle.filter(|s| *s > IDLE_SECONDS) {
        flags.idled_minutes = Some((idle / 60.0).round() as i64);
    }
    if let Some(now) = refused {
        // the count is the worker's since its block began: an earlier day of the same lease is taken off
        let on_the_day = now - before.unwrap_or(0);
        match earlier {
            0 if now > 0 => flags.refused = Some((now, true)),
            n if n > 0 && on_the_day > 0 => flags.refused = Some((on_the_day, false)),
            _ => {}
        }
    }
    // a withholding on many more assets than usual, of the day's base and day-local revisions
    let engine: Vec<&str> = crate::watch::ENGINE_REASONS.to_vec();
    let rows = c
        .query(
            "WITH d AS (SELECT revision_id, profile_id, published_at FROM vyreon.day_revision WHERE revision_id = $1),
                  day AS (SELECT revision_id FROM d UNION SELECT l.revision_id FROM vyreon.day_revision l WHERE l.base_revision = $1 AND l.day_local AND l.published_at IS NOT NULL),
                  today AS (SELECT w.measurement_id, w.reason, count(DISTINCT o) AS assets
                            FROM core.withheld_tally w JOIN day USING (revision_id), unnest(w.objects) o WHERE w.reason <> ALL($2::text[]) GROUP BY 1, 2),
                  prior AS (SELECT p.revision_id FROM vyreon.day_revision p, d
                            WHERE p.base_revision IS NULL AND p.state = 'PUBLISHED' AND p.profile_id = d.profile_id AND p.published_at < d.published_at
                              AND EXISTS (SELECT FROM core.withheld_counted k WHERE k.revision_id = p.revision_id)
                            ORDER BY p.published_at DESC LIMIT $3),
                  was AS (SELECT p.revision_id AS base, w.measurement_id, w.reason, count(DISTINCT o) AS assets
                          FROM prior p JOIN vyreon.day_revision x ON x.revision_id = p.revision_id OR (x.base_revision = p.revision_id AND x.day_local)
                          JOIN core.withheld_tally w ON w.revision_id = x.revision_id, unnest(w.objects) o GROUP BY 1, 2, 3)
             SELECT t.measurement_id, t.reason, t.assets,
                    (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY COALESCE(w.assets, 0))
                     FROM prior p LEFT JOIN was w ON w.base = p.revision_id AND w.measurement_id = t.measurement_id AND w.reason = t.reason),
                    (SELECT count(*) FROM prior),
                    (SELECT count(*) FROM day) = (SELECT count(*) FROM core.withheld_counted k JOIN day USING (revision_id))
             FROM today t
             UNION ALL
             SELECT NULL, NULL, NULL, NULL, (SELECT count(*) FROM prior),
                    (SELECT count(*) FROM day) = (SELECT count(*) FROM core.withheld_counted k JOIN day USING (revision_id))",
            &[&base, &engine, &USUAL_DAYS],
        )
        .await
        .map_err(db)?;
    let counted = rows.iter().all(|r| r.get::<_, bool>(5));
    if counted {
        let mut unusual: Vec<(String, String, i64, i64)> = rows
            .iter()
            .filter_map(|r| {
                let (m, reason, assets, usual, days): (Option<String>, Option<String>, Option<i64>, Option<f64>, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
                let usual = usual.unwrap_or(0.0).round() as i64;
                (days >= USUAL_MIN).then_some(())?;
                Some((m?, reason?, assets?, usual)).filter(|(_, _, assets, usual)| assets - usual >= UNUSUAL_ASSETS)
            })
            .collect();
        unusual.sort_by(|a, b| (b.2 - b.3).cmp(&(a.2 - a.3)).then_with(|| a.0.cmp(&b.0)).then_with(|| a.1.cmp(&b.1)));
        unusual.truncate(UNUSUAL_MAX);
        flags.unusual = unusual;
    }
    Ok((flags, counted))
}

/// The day's line on the progress channel, when it has a flag: "To watch: Nov 25, 2026 · slow: compute 4.2 h,
/// usual 1.9 h; idled 59 min waiting for captures".
pub fn flagged_line(date: &str, flags: &DayFlags) -> Option<String> {
    (!flags.is_empty()).then(|| format!("To watch: {} · {}", crate::progress::span(date, date), flags.parts().join("; ")))
}

/// What the run block states, as values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    /// Days (base revisions' dates) published in the last 24 hours.
    pub published: i64,
    /// The median download and compute seconds over the last `USUAL_DAYS` published days, and the stage with the
    /// largest median.
    pub download: Option<f64>,
    pub compute: Option<f64>,
    pub longest: Option<String>,
    /// The provider's rate refusals in the last 24 hours.
    pub refusals: i64,
    /// (measurement, reason, assets a day): the largest withholdings of the last 24 hours' days that are not the
    /// engine's own faults and not for history.
    pub withheld: Vec<(String, String, i64)>,
}

/// The digest's run lines (C-097), at most four.
pub fn run_lines(r: &Run) -> Vec<String> {
    if r.published == 0 {
        return vec!["Last 24 h: no day published".to_owned()];
    }
    let hours = |s: f64| format!("{:.1} h", s / 3600.0);
    let mut first = format!("Last 24 h: {} published", if r.published == 1 { "1 day".to_owned() } else { format!("{} days", crate::progress::thousands(r.published)) });
    if let (Some(d), Some(c)) = (r.download, r.compute) {
        first.push_str(&format!(" · a usual day: {} download, {} compute", hours(d), hours(c)));
        if let Some(stage) = &r.longest {
            first.push_str(&format!(" (longest step: {})", said(stage)));
        }
    }
    // C-098 (audit 2026-11-26, core 17): core holds the reports that reached it, a lower bound (a worker sends at
    // most 8 at once and drops the rest)
    let refusals = if r.refusals > 0 { format!("at least {}", crate::progress::thousands(r.refusals)) } else { "0".to_owned() };
    let mut lines = vec![first, format!("Provider refusals in 24 h: {refusals}")];
    if !r.withheld.is_empty() {
        let parts: Vec<String> = r.withheld.iter().map(|(m, reason, assets)| format!("{}: {} on {assets} assets a day", said(m), reason_words(reason))).collect();
        lines.push(format!("Most withheld: {}", parts.join("; ")));
    }
    lines
}

/// The run at `now`, read from what is stored.
pub async fn run(c: &impl GenericClient, profile: &str, now: &str) -> Result<Run, String> {
    let engine: Vec<&str> = crate::watch::ENGINE_REASONS.to_vec();
    let r = c
        .query_one(
            &format!(
                "WITH recent AS (SELECT d.report, {download} AS download, {compute} AS compute FROM vyreon.day_revision d
                                 WHERE d.profile_id = $2 AND d.base_revision IS NULL AND d.published_at IS NOT NULL AND d.published_at <= $1::text::timestamptz
                                 ORDER BY d.published_at DESC LIMIT $3)
                 SELECT (SELECT count(DISTINCT represented_date) FROM vyreon.day_revision
                         WHERE profile_id = $2 AND base_revision IS NULL AND published_at > $1::text::timestamptz - interval '24 hours' AND published_at <= $1::text::timestamptz),
                        (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY download) FROM recent WHERE download IS NOT NULL),
                        (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY compute) FROM recent WHERE compute IS NOT NULL),
                        (SELECT stage FROM (SELECT s->>'stage' AS stage, percentile_cont(0.5) WITHIN GROUP (ORDER BY (s->>'seconds')::float8) AS m
                                            FROM recent, jsonb_array_elements(CASE WHEN jsonb_typeof(report->'stages') = 'array' THEN report->'stages' ELSE '[]'::jsonb END) s
                                            WHERE jsonb_typeof(s->'seconds') = 'number' AND s->>'stage' IS NOT NULL GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT 1) z),
                        (SELECT count(*) FROM core.provider_refusal WHERE status = 429 AND at > $1::text::timestamptz - interval '24 hours' AND at <= $1::text::timestamptz)",
                download = download_of("d"),
                compute = compute_of("d"),
            ),
            &[&now, &profile, &USUAL_DAYS],
        )
        .await
        .map_err(db)?;
    let withheld = c
        .query(
            "WITH day AS (SELECT COALESCE(d.base_revision, d.revision_id) AS base, d.revision_id FROM vyreon.day_revision d
                          WHERE d.profile_id = $2 AND d.state = 'PUBLISHED' AND (d.base_revision IS NULL OR d.day_local)
                            AND d.published_at > $1::text::timestamptz - interval '24 hours' AND d.published_at <= $1::text::timestamptz),
                  a AS (SELECT day.base, w.measurement_id, w.reason, count(DISTINCT o) AS assets
                        FROM day JOIN core.withheld_tally w USING (revision_id), unnest(w.objects) o
                        WHERE w.reason <> ALL($3::text[]) AND w.reason NOT LIKE '%INSUFFICIENT%' AND w.reason NOT LIKE '%HISTORY%' AND w.reason <> 'DEPENDENCY_WITHHELD'
                        GROUP BY 1, 2, 3)
             SELECT measurement_id, reason, round(sum(assets)::numeric / (SELECT count(DISTINCT base) FROM day))::bigint
             FROM a GROUP BY 1, 2 ORDER BY 3 DESC, 1, 2 LIMIT 3",
            &[&now, &profile, &engine],
        )
        .await
        .map_err(db)?;
    Ok(Run {
        published: r.get(0),
        download: r.get(1),
        compute: r.get(2),
        longest: r.get(3),
        refusals: r.get(4),
        withheld: withheld.iter().map(|w| (w.get(0), w.get(1), w.get(2))).collect(),
    })
}

/// `run` and `run_lines` under a savepoint of their own: a failure is logged and says nothing (the digest goes on).
pub async fn run_block(c: &mut impl GenericClient, profile: &str, now: &str) -> Vec<String> {
    let read = match c.transaction().await {
        Ok(tx) => {
            let r = run(&tx, profile, now).await;
            let _ = tx.rollback().await;
            r
        }
        Err(e) => Err(db(e)),
    };
    match read {
        Ok(r) => run_lines(&r),
        Err(e) => {
            eprintln!("vyreon-core: the digest's run lines were not read (the digest goes on without them): {e}");
            Vec::new()
        }
    }
}

/// One line of the timings table: what, how many days, the median and the 90th percentile in seconds.
pub type Timing = (String, i64, f64, f64);

/// The timings of the published base days of `profile` (of `build`, if given), from their stored reports: the
/// download, the compute, each stage (the longest median first), and a worker's first published day against its
/// later ones (download and compute together; the first day pays the empty disk).
pub async fn timings(c: &impl GenericClient, profile: &str, build: Option<&str>) -> Result<Vec<Timing>, String> {
    let rows = c
        .query(
            &format!(
                "WITH days AS (SELECT d.revision_id, d.report, d.published_at, {download} AS download, {compute} AS compute,
                                      (SELECT lease_id FROM core.lease_revision r WHERE r.revision_id = d.revision_id) AS lease_id
                               FROM vyreon.day_revision d
                               WHERE d.profile_id = $1 AND d.base_revision IS NULL AND d.state = 'PUBLISHED' AND ($2::text IS NULL OR d.engine_build = $2)),
                      placed AS (SELECT days.*, published_at = min(published_at) OVER (PARTITION BY lease_id) AS first FROM days WHERE lease_id IS NOT NULL),
                      t AS (SELECT 0 AS o, 'download' AS what, download AS s FROM days
                            UNION ALL SELECT 1, 'compute', compute FROM days
                            UNION ALL SELECT 2, 'a worker''s first day (download + compute)', download + compute FROM placed WHERE first
                            UNION ALL SELECT 3, 'a worker''s later days (download + compute)', download + compute FROM placed WHERE NOT first
                            UNION ALL SELECT 4, 'stage: ' || (s->>'stage'), (s->>'seconds')::float8
                                      FROM days, jsonb_array_elements(CASE WHEN jsonb_typeof(report->'stages') = 'array' THEN report->'stages' ELSE '[]'::jsonb END) s
                                      WHERE jsonb_typeof(s->'seconds') = 'number' AND s->>'stage' IS NOT NULL)
                 SELECT what, count(*), percentile_cont(0.5) WITHIN GROUP (ORDER BY s), percentile_cont(0.9) WITHIN GROUP (ORDER BY s)
                 FROM t WHERE s IS NOT NULL GROUP BY o, what ORDER BY o, 3 DESC, what",
                download = download_of("d"),
                compute = compute_of("d"),
            ),
            &[&profile, &build],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C-097: a day's flags in plain words, in their order; a day with none says nothing.
    #[test]
    fn a_days_flags_read_in_plain_words() {
        assert_eq!(flagged_line("2026-11-25", &DayFlags::default()), None);
        let flags = DayFlags {
            slow: Some((4.24, 1.9)),
            idled_minutes: Some(59),
            refused: Some((1_240, false)),
            unusual: vec![("PCR-UACT-I01".into(), "SOURCE_VOLUME_RECONCILIATION_FAILED".into(), 55, 3)],
        };
        assert_eq!(
            flagged_line("2026-11-25", &flags).unwrap(),
            "To watch: Nov 25, 2026 · slow: compute 4.2 h, usual 1.9 h; idled 59 min waiting for captures; provider refused 1,240 requests; \
             unusual withholding: PCR-UACT-I01 (source volume reconciliation failed) on 55 assets (usual 3)"
        );
        let block = DayFlags { refused: Some((45, true)), ..DayFlags::default() };
        assert_eq!(flagged_line("2024-07-15", &block).unwrap(), "To watch: Jul 15, 2024 · provider refused 45 requests so far in its block");
        // C-098: a word of a body or a report is said on one line, and never a credential
        let odd = DayFlags { unusual: vec![("X\nY".into(), "A_B\nC".into(), 20, 0)], ..DayFlags::default() };
        assert_eq!(flagged_line("2024-07-15", &odd).unwrap(), "To watch: Jul 15, 2024 · unusual withholding: X (a b) on 20 assets (usual 0)");
        assert!(!run_lines(&Run { published: 1, download: Some(1.0), compute: Some(1.0), longest: Some("stage api_key=abcdef0123".into()), ..Run::default() })[0].contains("abcdef"));
    }

    /// C-097: the digest's run lines; with no day published, one line.
    #[test]
    fn the_run_reads_in_at_most_four_lines() {
        assert_eq!(run_lines(&Run::default()), ["Last 24 h: no day published"]);
        let run = Run {
            published: 4,
            download: Some(4_980.0),
            compute: Some(6_177.0),
            longest: Some("intent B (T)".into()),
            refusals: 0,
            withheld: vec![("PCR-UQTR-I01".into(), "QUOTE_COMPLETENESS_UNVERIFIED".into(), 64), ("CDD-MACRO01".into(), "INVALID_REQUIRED_INPUT".into(), 64)],
        };
        assert_eq!(
            run_lines(&run),
            [
                "Last 24 h: 4 days published · a usual day: 1.4 h download, 1.7 h compute (longest step: intent B (T))",
                "Provider refusals in 24 h: 0",
                "Most withheld: PCR-UQTR-I01: quote completeness unverified on 64 assets a day; CDD-MACRO01: invalid required input on 64 assets a day",
            ]
        );
        assert_eq!(run_lines(&Run { published: 1, refusals: 1_240, ..Run::default() })[1], "Provider refusals in 24 h: at least 1,240");
        let one = Run { published: 1, ..Run::default() };
        assert_eq!(run_lines(&one), ["Last 24 h: 1 day published", "Provider refusals in 24 h: 0"]);
    }
}
