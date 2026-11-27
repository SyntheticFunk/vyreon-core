//! C-096: the census of what the published days withhold, for the phase-1 catalog. A reading states its
//! withholding at three grains, and the census counts each where it is stated: the slot (the value's
//! `terminal_reason` or `failure_reason`), a role of it, a coordinate of it (each `reason`). It reads the
//! published revisions as they are and judges nothing: the class is the stated reason's (`watch::ENGINE_REASONS`
//! are the engine's own faults; every other reason is a stated absence, the provider's or the method's).
use tokio_postgres::Client;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// One line of the census: a measurement's withholdings for one reason at one grain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub measurement_id: String,
    /// `slot`, `role` or `coordinate`.
    pub grain: String,
    pub reason: String,
    /// Whether the reason is one of the engine's own faults.
    pub engine: bool,
    pub count: i64,
    pub assets: i64,
    pub days: i64,
    pub first: String,
    pub last: String,
    /// Up to five assets, by name.
    pub examples: Vec<String>,
}

/// The days the census read: published days of `profile` in the range (of `build`, if given), and their slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    pub days: i64,
    pub slots: i64,
}

const KEPT: &str = "SELECT d.revision_id, d.represented_date FROM vyreon.day_revision d
                    WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND d.represented_date BETWEEN $2::text::date AND $3::text::date
                      AND ($4::text IS NULL OR d.engine_build = $4)";

/// The rows a withholding is stated on, read from the relation `m` (its columns, among them the value `v`, are
/// carried): one row per slot, role or coordinate that states a reason, with its `grain` and `reason`. The census
/// and the watch's counts (`runview`) share it.
pub(crate) fn stated(m: &str) -> String {
    let array = |key: &str| format!("CASE WHEN jsonb_typeof(v.v->'{key}') = 'array' THEN v.v->'{key}' ELSE '[]'::jsonb END");
    format!(
        "SELECT v.*, 'slot' AS grain, COALESCE(v.v->>'terminal_reason', v.v->>'failure_reason') AS reason
         FROM {m} v WHERE COALESCE(v.v->>'terminal_reason', v.v->>'failure_reason') IS NOT NULL
         UNION ALL
         SELECT v.*, 'role', e->>'reason' FROM {m} v, jsonb_array_elements({roles}) e WHERE e->>'reason' IS NOT NULL
         UNION ALL
         SELECT v.*, 'coordinate', e->>'reason' FROM {m} v, jsonb_array_elements({coordinates}) e WHERE e->>'reason' IS NOT NULL",
        roles = array("roles"),
        coordinates = array("coordinates"),
    )
}

/// The census over `first..=last`, the most frequent first.
pub async fn withheld(client: &Client, profile: &str, first: &str, last: &str, build: Option<&str>) -> Result<(Read, Vec<Line>), String> {
    let read = client
        .query_one(
            &format!("WITH kept AS ({KEPT}) SELECT count(DISTINCT k.represented_date), count(m.measurement_id)
                      FROM kept k LEFT JOIN vyreon.measurement m ON m.revision_id = k.revision_id AND m.represented_date = k.represented_date"),
            &[&profile, &first, &last, &build],
        )
        .await
        .map_err(db)?;
    // C-097: when every kept revision's withholdings are counted (`core.withheld_tally`, the watch's), the census
    // is a sum over the counts; else the bodies are read as before. The lines are the same either way.
    let uncounted: i64 = client
        .query_one(
            &format!("WITH kept AS ({KEPT}) SELECT count(*) FROM kept k WHERE NOT EXISTS (SELECT FROM core.withheld_counted w WHERE w.revision_id = k.revision_id)"),
            &[&profile, &first, &last, &build],
        )
        .await
        .map_err(db)?
        .get(0);
    let sql = if uncounted == 0 {
        format!(
            "WITH kept AS ({KEPT}),
                  t AS (SELECT k.represented_date, w.* FROM kept k JOIN core.withheld_tally w USING (revision_id)),
                  s AS (SELECT measurement_id, grain, reason, sum(n)::bigint AS n, count(DISTINCT represented_date) AS days,
                               min(represented_date)::text AS first, max(represented_date)::text AS last FROM t GROUP BY 1, 2, 3),
                  a AS (SELECT t.measurement_id, t.grain, t.reason, count(DISTINCT o) AS assets, (array_agg(DISTINCT o ORDER BY o))[1:5] AS examples
                        FROM t, unnest(t.objects) o GROUP BY 1, 2, 3)
             SELECT s.measurement_id, s.grain, s.reason, s.reason = ANY($5::text[]), s.n, a.assets, s.days, s.first, s.last, a.examples
             FROM s JOIN a USING (measurement_id, grain, reason) ORDER BY 5 DESC, 1, 2, 3"
        )
    } else {
        format!(
            "WITH kept AS ({KEPT}),
                  m AS (SELECT m.measurement_id, m.object_id, m.represented_date, m.body->'measurement'->'value' AS v
                        FROM kept k JOIN vyreon.measurement m ON m.revision_id = k.revision_id AND m.represented_date = k.represented_date),
                  r AS ({stated})
             SELECT measurement_id, grain, reason, reason = ANY($5::text[]), count(*), count(DISTINCT object_id), count(DISTINCT represented_date),
                    min(represented_date)::text, max(represented_date)::text, (array_agg(DISTINCT object_id ORDER BY object_id))[1:5]
             FROM r GROUP BY 1, 2, 3 ORDER BY 5 DESC, 1, 2, 3",
            stated = stated("m"),
        )
    };
    let engine: Vec<&str> = crate::watch::ENGINE_REASONS.to_vec();
    let rows = client.query(&sql, &[&profile, &first, &last, &build, &engine]).await.map_err(db)?;
    let lines = rows
        .iter()
        .map(|r| Line {
            measurement_id: r.get(0),
            grain: r.get(1),
            reason: r.get(2),
            engine: r.get(3),
            count: r.get(4),
            assets: r.get(5),
            days: r.get(6),
            first: r.get(7),
            last: r.get(8),
            examples: r.get(9),
        })
        .collect();
    Ok((Read { days: read.get(0), slots: read.get(1) }, lines))
}

/// A lease's steps in order: the step, what it waited for, when it began and how many seconds it lasted (to the
/// next step, or to the lease's end; None while it is the current step of a running lease).
pub async fn steps(client: &Client, job_id: i64) -> Result<Vec<(i64, String, String, Option<String>, String, Option<f64>)>, String> {
    let rows = client
        .query(
            "SELECT s.lease_id, l.worker, s.step, s.reason, to_char(s.since AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'),
                    EXTRACT(EPOCH FROM COALESCE(lead(s.since) OVER (PARTITION BY s.lease_id ORDER BY s.since, s.lease_step_log_id), l.ended_at) - s.since)::float8
             FROM core.lease_step_log s JOIN core.lease l USING (lease_id)
             WHERE l.job_id = $1 ORDER BY s.lease_id, s.since, s.lease_step_log_id",
            &[&job_id],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5))).collect())
}
