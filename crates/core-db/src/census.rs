//! C-096: the census of what the published days withhold, for the phase-1 catalog. A reading states its
//! withholding at several grains, and the census counts each where it is stated (`stated`): the slot, a role of
//! it, a coordinate of it, a cell of a FIELD array, a company array. It reads the published revisions as they
//! are and judges nothing: the class is the stated reason's (`watch::ENGINE_REASONS` are the engine's own
//! faults; every other reason is a stated absence, the provider's or the method's).
//!
//! C-098 (audit 2026-11-26, core 9): a body states its withholdings where its schema keeps them, as the engine's
//! own reader of them does (`vyreon-coordinates`): `H4-INLINE-MEASUREMENT@1` and
//! `H5-DIRECT-OWNER-MEASUREMENT@1` under `measurement.value` (its `roles`, its `coordinates`, and the direct
//! schema's `array_ref_coordinates` not already among the roles); `H4-V03-MEASUREMENT@1` and
//! `H4-EXQ-TEMPORAL-MEASUREMENT@1` in top-level `roles`; `H4-PRINT-MEASUREMENT@1` in top-level `coordinates` and
//! `support_reason`; `H5-COMPANY-OWNER-RESULT@1` in `terminal_reason`, each coordinate's `support` and its
//! arrays' states; a FIELD body (no schema) in `coordinates` and each array's `cells`. The first version read
//! `measurement.value` alone and missed the rest (on 2026-11-25's three revisions: 61 lines and 21,224 statements for 83 and 22,210).
use tokio_postgres::Client;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// One line of the census: a measurement's withholdings for one reason at one grain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub measurement_id: String,
    /// `slot`, `role`, `coordinate`, `cell` (of a FIELD array) or `array` (a company array).
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

/// The published revisions of `profile` in the range ($1, $2, $3).
const KEPT: &str = "SELECT d.revision_id, d.represented_date, d.engine_build FROM vyreon.day_revision d
                    WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND d.represented_date BETWEEN $2::text::date AND $3::text::date";

/// The kept revisions' rows that `build` ($4) computed, or every row when no build is asked. C-098 (audit
/// 2026-11-26, core 13): a row a later build's revision carried forward byte for byte
/// (`carried_from_revision`) is the build's that computed it, not the build's that carried it; the first
/// version filtered on the publishing revision's build, so a carried row counted for the wrong one.
const ROWS: &str = "SELECT m.revision_id, m.measurement_id, m.object_id, m.represented_date, m.body AS b
                    FROM kept k JOIN vyreon.measurement m ON m.revision_id = k.revision_id AND m.represented_date = k.represented_date
                    WHERE $4::text IS NULL
                       OR COALESCE((SELECT o.engine_build FROM vyreon.day_revision o WHERE o.revision_id = m.carried_from_revision), k.engine_build) = $4";

/// The rows a withholding is stated on, read from the relation `m` (its columns, among them the body `b`, are
/// carried, and `mv`, the body's `measurement.value`, beside them): one row per slot, role, coordinate, cell or
/// array that states a reason, with its `grain` and `reason`. The census and the watch's counts (`runview`)
/// share it. A stated SUPPORTED is no withholding (the company and temporal schemas write it as the reason of a
/// supported body). A company array states its state and no reason: the state stands for it.
pub fn stated(m: &str) -> String {
    let array = |e: &str| format!("CASE WHEN jsonb_typeof({e}) = 'array' THEN {e} ELSE '[]'::jsonb END");
    let object = |e: &str| format!("CASE WHEN jsonb_typeof({e}) = 'object' THEN {e} ELSE '{{}}'::jsonb END");
    format!(
        "WITH x AS MATERIALIZED (SELECT v.*, v.b->'measurement'->'value' AS mv FROM {m} v)
         SELECT x.*, 'slot' AS grain, COALESCE(x.mv->>'terminal_reason', x.mv->>'failure_reason') AS reason
         FROM x
         WHERE COALESCE(x.mv->>'terminal_reason', x.mv->>'failure_reason') IS NOT NULL AND COALESCE(x.mv->>'terminal_reason', x.mv->>'failure_reason') <> 'SUPPORTED'
         UNION ALL
         SELECT x.*, 'slot', x.b->>'terminal_reason' FROM x
         WHERE x.mv IS NULL AND x.b->>'terminal_reason' IS NOT NULL AND x.b->>'terminal_reason' <> 'SUPPORTED' AND COALESCE(x.b->>'terminal_state', '') <> 'SUPPORTED'
         UNION ALL
         SELECT x.*, 'slot', x.b->>'support_reason' FROM x
         WHERE x.mv IS NULL AND x.b->>'support_reason' IS NOT NULL AND COALESCE(x.b->>'support_state', '') <> 'SUPPORTED'
         UNION ALL
         SELECT x.*, 'role', e->>'reason' FROM x,
                jsonb_array_elements({value_roles} || {top_roles}) e
         WHERE e->>'reason' IS NOT NULL
         UNION ALL
         SELECT x.*, 'role', e->>'reason' FROM x, jsonb_array_elements({array_refs}) e
         WHERE e->>'reason' IS NOT NULL
           AND NOT EXISTS (SELECT FROM jsonb_array_elements({value_roles}) r WHERE r->>'role_id' = e->>'role_id')
         UNION ALL
         SELECT x.*, 'coordinate', e->>'reason' FROM x, jsonb_array_elements({value_coordinates}) e
         WHERE e->>'reason' IS NOT NULL
         UNION ALL
         SELECT x.*, 'coordinate', COALESCE(e->>'reason', e->'support'->>'reason_code')
         FROM x, jsonb_array_elements({top_coordinates}) e
         WHERE x.mv IS NULL AND (e->>'reason' IS NOT NULL
                                 OR (e->'support'->>'reason_code' IS NOT NULL AND COALESCE(e->'support'->>'state', '') <> 'SUPPORTED'))
         UNION ALL
         SELECT x.*, 'cell', c->>'reason' FROM x,
                jsonb_array_elements({top_arrays}) a, jsonb_array_elements({cells}) c
         WHERE x.mv IS NULL AND jsonb_typeof(c) = 'object' AND c->>'reason' IS NOT NULL
         UNION ALL
         SELECT x.*, 'array', COALESCE(a.value->>'reason', a.value->>'state') FROM x,
                jsonb_each({named_arrays}) a
         WHERE x.mv IS NULL AND jsonb_typeof(a.value) = 'object' AND a.value->>'state' IS NOT NULL AND a.value->>'state' <> 'SUPPORTED'",
        value_roles = array("x.mv->'roles'"),
        top_roles = format!("CASE WHEN x.mv IS NULL THEN {} ELSE '[]'::jsonb END", array("x.b->'roles'")),
        array_refs = array("x.mv->'array_ref_coordinates'"),
        value_coordinates = array("x.mv->'coordinates'"),
        top_coordinates = array("x.b->'coordinates'"),
        top_arrays = array("x.b->'arrays'"),
        cells = array("a->'cells'"),
        named_arrays = object("x.b->'arrays'"),
    )
}

/// The census over `first..=last`, the most frequent first. C-098 (audit 2026-11-26, core 11): its header and
/// its lines are read in one snapshot (a day published between two statements made them disagree).
pub async fn withheld(client: &Client, profile: &str, first: &str, last: &str, build: Option<&str>) -> Result<(Read, Vec<Line>), String> {
    client.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await.map_err(db)?;
    let read = read_census(client, profile, first, last, build).await;
    let ended = client.batch_execute("ROLLBACK").await.map_err(db);
    let read = read?;
    ended?;
    Ok(read)
}

async fn read_census(client: &Client, profile: &str, first: &str, last: &str, build: Option<&str>) -> Result<(Read, Vec<Line>), String> {
    let read = client
        .query_one(
            &format!("WITH kept AS ({KEPT}), m AS ({ROWS}) SELECT count(DISTINCT represented_date), count(*) FROM m"),
            &[&profile, &first, &last, &build],
        )
        .await
        .map_err(db)?;
    // C-097: when every kept revision's withholdings are counted (`core.withheld_tally`, the watch's), the census
    // is a sum over the counts; else the bodies are read as before. The lines are the same either way. With a
    // build asked the bodies are read: the counts are a revision's, carried rows among them (C-098 13).
    let uncounted: i64 = client
        .query_one(
            &format!("WITH kept AS ({KEPT}) SELECT count(*) FROM kept k WHERE NOT EXISTS (SELECT FROM core.withheld_counted w WHERE w.revision_id = k.revision_id)"),
            &[&profile, &first, &last],
        )
        .await
        .map_err(db)?
        .get(0);
    let engine: Vec<&str> = crate::watch::ENGINE_REASONS.to_vec();
    let rows = if uncounted == 0 && build.is_none() {
        client
            .query(
                &format!(
                    "WITH kept AS ({KEPT}),
                          t AS (SELECT k.represented_date, w.* FROM kept k JOIN core.withheld_tally w USING (revision_id)),
                          s AS (SELECT measurement_id, grain, reason, sum(n)::bigint AS n, count(DISTINCT represented_date) AS days,
                                       min(represented_date)::text AS first, max(represented_date)::text AS last FROM t GROUP BY 1, 2, 3),
                          a AS (SELECT t.measurement_id, t.grain, t.reason, count(DISTINCT o) AS assets, (array_agg(DISTINCT o ORDER BY o))[1:5] AS examples
                                FROM t, unnest(t.objects) o GROUP BY 1, 2, 3)
                     SELECT s.measurement_id, s.grain, s.reason, s.reason = ANY($4::text[]), s.n, a.assets, s.days, s.first, s.last, a.examples
                     FROM s JOIN a USING (measurement_id, grain, reason) ORDER BY 5 DESC, 1, 2, 3"
                ),
                &[&profile, &first, &last, &engine],
            )
            .await
    } else {
        client
            .query(
                &format!(
                    "WITH kept AS ({KEPT}), m AS ({ROWS}), r AS ({stated})
                     SELECT measurement_id, grain, reason, reason = ANY($5::text[]), count(*), count(DISTINCT object_id), count(DISTINCT represented_date),
                            min(represented_date)::text, max(represented_date)::text, (array_agg(DISTINCT object_id ORDER BY object_id))[1:5]
                     FROM r GROUP BY 1, 2, 3 ORDER BY 5 DESC, 1, 2, 3",
                    stated = stated("m"),
                ),
                &[&profile, &first, &last, &build, &engine],
            )
            .await
    }
    .map_err(db)?;
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
