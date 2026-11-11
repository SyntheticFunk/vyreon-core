//! The shared Intrinio request budget (migration 0006, ARCHITECTURE 6). Workers draw short
//! leases of requests and report what they used, by category.
//!
//! The rule: at any moment, the leases that could still put a request inside the last rolling
//! minute sum to no more than the ceiling. A lease counts from its grant until 60 seconds after
//! its end (its validity, or its release if earlier): at its full size while open, at what was
//! used once released.
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

#[derive(Debug, Clone, PartialEq)]
pub enum Grant {
    Granted { budget_lease_id: i64, requests: i32, valid_seconds: f64 },
    /// Nothing is free now; ask again after this many seconds.
    Wait { seconds: f64 },
}

/// The sum the open and recently ended leases hold against the ceiling.
const COMMITTED: &str = "SELECT COALESCE(sum(COALESCE(used, granted)), 0)::int,
                                EXTRACT(EPOCH FROM min(COALESCE(released_at, valid_until)) + interval '60 seconds' - now())::float8
                         FROM core.budget_lease
                         WHERE COALESCE(released_at, valid_until) > now() - interval '60 seconds'";

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
        return Ok(Grant::Wait { seconds: frees_in.unwrap_or(1.0).max(0.1) });
    }
    let granted = requests.min(free).min(budget.max_lease);
    let id: i64 = tx
        .query_one(
            "INSERT INTO core.budget_lease (worker, granted, valid_until) VALUES ($1, $2, now() + make_interval(secs => $3)) RETURNING budget_lease_id",
            &[&worker, &granted, &budget.lease_seconds],
        )
        .await
        .map_err(db)?
        .get(0);
    tx.commit().await.map_err(db)?;
    Ok(Grant::Granted { budget_lease_id: id, requests: granted, valid_seconds: budget.lease_seconds })
}

/// Releases a lease with what was used, by category (`{"daily": 12, "options": 300}`). The usage
/// is added to the worker's minute. A lease is released once, by its worker, for no more than it
/// was granted.
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
    // more than granted is recorded as it was reported, and counted in full: the ceiling's margin is for this
    let counted = total.min(i64::from(granted)) as i32;
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

/// What is committed against the ceiling now.
pub async fn committed(client: &Client) -> Result<i32, String> {
    Ok(client.query_one(COMMITTED, &[]).await.map_err(db)?.get(0))
}
