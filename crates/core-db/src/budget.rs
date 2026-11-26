//! The shared Intrinio request budget (migration 0006, ARCHITECTURE 6). Workers draw short
//! leases of requests and report what they used, by category.
//!
//! The rule: at any moment, the leases that could still put a request inside the last rolling
//! minute sum to no more than the ceiling. A lease counts from its grant until 60 seconds after
//! its end (its validity, or its release if earlier, each with the requests' way to the provider,
//! `TRANSIT_SECONDS`): at its full size while open, at what was used once released.
//!
//! C-076: this is a sliding-window reservation (the sliding-log limiter, kept per lease), and it
//! bounds every rolling 60 s window, not a calendar minute: of the leases whose requests fall in a
//! window [a, a + 60 s], take the last granted, at t <= a + 60 s; every other one was granted before
//! it and ended at or after a >= t - 60 s, so the grant at t counted it, and the grant was made only
//! with the sum within the ceiling. The calendar minutes of `core.request_usage` are a ledger booked at
//! release, not the rule.
use serde_json::Value;
use tokio_postgres::Client;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The budget's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// Requests a minute core allows the account (below Intrinio's 2,000).
    pub per_minute: i32,
    /// How long a lease may be used.
    pub lease_seconds: f64,
    /// The largest lease.
    pub max_lease: i32,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { per_minute: 1800, lease_seconds: 30.0, max_lease: 600 }
    }
}

/// Seconds core counts a lease beyond the validity it tells the worker: the worker starts its
/// `valid_seconds` when the answer reaches it, after core has recorded the grant, so core's end of
/// the lease must not come before the worker's. A plain allowance for the answer's way back (on
/// the cluster's own network, well under a second), not a measured one; an answer later than this
/// could still let requests fall outside core's count.
pub const TRANSIT_SECONDS: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Grant {
    Granted { budget_lease_id: i64, requests: i32, valid_seconds: f64 },
    /// Nothing is free now; ask again after this many seconds.
    Wait { seconds: f64 },
}

/// The sum the open and recently ended leases hold against the ceiling.
/// C-076: a released lease ends `TRANSIT_SECONDS` after its release reached core (a request its worker
/// was permitted just before still travels to the provider); an open one at its validity, which holds
/// that allowance already.
const COMMITTED: &str = "SELECT COALESCE(sum(COALESCE(used, granted)), 0)::int,
                                EXTRACT(EPOCH FROM min(COALESCE(released_at + interval '5 seconds', valid_until)) + interval '60 seconds' - now())::float8
                         FROM core.budget_lease
                         WHERE COALESCE(released_at + interval '5 seconds', valid_until) > now() - interval '60 seconds'";

/// Draws up to `requests` for `worker`. A smaller lease than asked is granted when that is all
/// that is free.
pub async fn draw(client: &mut Client, budget: Budget, worker: &str, requests: i32) -> Result<Grant, String> {
    if requests < 1 {
        return Err("a budget lease is for at least one request".into());
    }
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.budget_lease', 0))", &[]).await.map_err(db)?;
    let row = tx.query_one(COMMITTED, &[]).await.map_err(db)?;
    let (committed, frees_in): (i32, Option<f64>) = (row.get(0), row.get(1));
    let free = budget.per_minute - committed;
    if free < 1 {
        // C-073: the wait is recorded, by minute: the scaler starts no pod that calls the provider
        // while one came within `PRESSURE_MINUTES` (`core.scaler_count`)
        tx.execute(
            "INSERT INTO core.budget_wait (minute, waits, last_at) VALUES (date_trunc('minute', clock_timestamp()), 1, clock_timestamp())
             ON CONFLICT (minute) DO UPDATE SET waits = core.budget_wait.waits + 1, last_at = EXCLUDED.last_at",
            &[],
        )
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        return Ok(Grant::Wait { seconds: frees_in.unwrap_or(1.0).max(0.1) });
    }
    let granted = requests.min(free).min(budget.max_lease);
    // the clock, not the transaction's start (which is before the lock was had), plus the answer's
    // way to the worker
    let id: i64 = tx
        .query_one(
            "INSERT INTO core.budget_lease (worker, granted, granted_at, valid_until)
             VALUES ($1, $2, clock_timestamp(), clock_timestamp() + make_interval(secs => $3)) RETURNING budget_lease_id",
            &[&worker, &granted, &(budget.lease_seconds + TRANSIT_SECONDS)],
        )
        .await
        .map_err(db)?
        .get(0);
    tx.commit().await.map_err(db)?;
    Ok(Grant::Granted { budget_lease_id: id, requests: granted, valid_seconds: budget.lease_seconds })
}

/// Releases a lease with what was used, by category (`{"daily": 12, "options": 300}`). The usage
/// is added to the worker's minute. A lease is released once, by its worker. More than was granted
/// is counted as reported, and the release then answers with an error saying so.
pub async fn release(client: &mut Client, budget_lease_id: i64, worker: &str, used: &Value) -> Result<i32, String> {
    let categories = used.as_object().ok_or("usage is a JSON object of category: requests")?;
    let mut total: i64 = 0;
    for (category, n) in categories {
        total += n.as_i64().filter(|n| *n >= 0).ok_or_else(|| format!("usage of {category} is not a count"))?;
    }
    let tx = client.transaction().await.map_err(db)?;
    let granted: i32 = tx
        .query_opt("SELECT granted FROM core.budget_lease WHERE budget_lease_id = $1 AND worker = $2 AND released_at IS NULL FOR UPDATE", &[&budget_lease_id, &worker])
        .await
        .map_err(db)?
        .ok_or_else(|| format!("{worker} has no open budget lease {budget_lease_id}"))?
        .get(0);
    // more than granted is recorded as it was reported, and counted in full: the ceiling's margin
    // is for this, and the next grants see it (migration 0015; the column held at most the grant)
    let counted = total.min(i64::from(i32::MAX)) as i32;
    tx.execute("UPDATE core.budget_lease SET used = $2, released_at = now() WHERE budget_lease_id = $1", &[&budget_lease_id, &counted]).await.map_err(db)?;
    tx.execute(
        "INSERT INTO core.request_usage (minute, worker, category, requests)
         SELECT date_trunc('minute', now()), $1, key, value::bigint FROM jsonb_each_text($2::text::jsonb) WHERE value::bigint > 0
         ON CONFLICT (minute, worker, category) DO UPDATE SET requests = core.request_usage.requests + EXCLUDED.requests",
        &[&worker, &used.to_string()],
    )
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    if total > i64::from(granted) {
        return Err(format!("{worker} reports {total} requests on a lease of {granted}"));
    }
    Ok(counted)
}

/// C-073: how long after a draw had to wait the scaler counts no work that calls the provider
/// (`core.scaler_count`, migration 0055, which states the same five minutes in its SQL). A wait means
/// the workers already ask for more than the ceiling; another pod would only wait in the budget, at
/// node cost. Five minutes is ten of KEDA's 30-second polls: long enough that a pod started at the
/// end of one burst does not meet the next (a day's download bursts for minutes, 2026-11-22), short
/// enough that a quiet budget is used again within the time a node takes to come up.
pub const PRESSURE_MINUTES: f64 = 5.0;

/// C-092: after `PRESSURE_MINUTES`, the backfill's provider work of a kind starts at most one pod a minute until this
/// many minutes have passed since the last wait (`core.scaler_count`, migration 0066, which states the numbers).
pub const SLOW_START_MINUTES: f64 = 30.0;

/// C-073: when the budget last made a draw wait, if within `PRESSURE_MINUTES` of `now` (as text, UTC),
/// and how many draws waited in the whole minutes inside that time (release audit, Codex 10: a minute
/// that began before it also held earlier waits; its count is left out, so the number is never more
/// than the waits in the time, and the CLI says "at least").
pub async fn pressed(client: &impl tokio_postgres::GenericClient, now: &str) -> Result<Option<(String, i64)>, String> {
    let r = client
        .query_one(
            "SELECT to_char(max(last_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'),
                    COALESCE(sum(waits) FILTER (WHERE minute >= $1::text::timestamptz - make_interval(secs => $2::float8 * 60)), 0)::bigint
             FROM core.budget_wait
             WHERE last_at > $1::text::timestamptz - make_interval(secs => $2::float8 * 60) AND last_at <= $1::text::timestamptz",
            &[&now, &PRESSURE_MINUTES],
        )
        .await
        .map_err(db)?;
    Ok(r.get::<_, Option<String>>(0).map(|at| (at, r.get(1))))
}

/// What is committed against the ceiling now.
pub async fn committed(client: &Client) -> Result<i32, String> {
    Ok(client.query_one(COMMITTED, &[]).await.map_err(db)?.get(0))
}

/// One minute of the request ledger (C-068, audit opsurface ops.md 12; round 2, 17). Booked by the
/// minute each lease was GRANTED: `released` what its worker reported using, `unreleased` the full
/// size of the leases never released (a worker that died, or whose release never reached core).
/// A lease's requests are made between its grant and its end, at most `lease_seconds` (30) plus its
/// answer's way (5) later, so the requests made in a minute come only from leases granted in that
/// minute or the one before: `bound` (this minute's and the previous minute's leases, used or, if
/// unreleased, their full size) is an upper bound on the requests made in the minute. The minutes of
/// `core.request_usage` (booked at release) are not: two releases either side of a minute's end
/// split one minute's requests across two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerMinute {
    pub minute: String,
    pub released: i64,
    pub unreleased: i64,
    pub bound: i64,
    pub workers: i64,
}

/// The `top` minutes of the last `hours` hours with the highest `bound`, highest first.
pub async fn ledger(client: &Client, hours: f64, top: i64) -> Result<Vec<LedgerMinute>, String> {
    let rows = client
        .query(
            "WITH m AS (SELECT date_trunc('minute', granted_at) AS minute,
                               COALESCE(sum(used) FILTER (WHERE released_at IS NOT NULL), 0)::bigint AS released,
                               COALESCE(sum(granted) FILTER (WHERE released_at IS NULL), 0)::bigint AS unreleased,
                               count(DISTINCT worker) AS workers
                        FROM core.budget_lease WHERE granted_at > now() - make_interval(secs => $1::float8 * 3600) - interval '1 minute'
                        GROUP BY 1),
                  b AS (SELECT m.*, m.released + m.unreleased
                                    + COALESCE((SELECT p.released + p.unreleased FROM m p WHERE p.minute = m.minute - interval '1 minute'), 0) AS bound
                        FROM m)
             SELECT to_char(minute AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI'), released, unreleased, bound, workers FROM b
             WHERE minute > now() - make_interval(secs => $1::float8 * 3600)
             ORDER BY bound DESC, minute LIMIT $2",
            &[&hours, &top],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| LedgerMinute { minute: r.get(0), released: r.get(1), unreleased: r.get(2), bound: r.get(3), workers: r.get(4) }).collect())
}

/// The leases granted in the last `hours` hours that ended unreleased, and their requests (each
/// was held against the ceiling at its full size for its validity and a minute after, so the budget
/// never under-counted it; only the ledger misses what it used).
pub async fn unreleased(client: &Client, hours: f64) -> Result<(i64, i64), String> {
    let r = client
        .query_one(
            "SELECT count(*), COALESCE(sum(granted), 0)::bigint FROM core.budget_lease
             WHERE released_at IS NULL AND valid_until < now() AND granted_at > now() - make_interval(secs => $1::float8 * 3600)",
            &[&hours],
        )
        .await
        .map_err(db)?;
    Ok((r.get(0), r.get(1)))
}
