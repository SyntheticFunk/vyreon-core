//! C-087 (owner, 2026-11-26): the backfill's stop, one flag in the database (`core.queue_control`, migration
//! 0064). "Stop the backfill" was done by hand twice on 2026-11-25/26: every queued or leased backfill day
//! block and ordered pass cancelled, the queued reference-check re-runs cancelled, and the re-run watch
//! switched off by a setting that needs core restarted. `stop` does all of it in one transaction and sets
//! the flag, which the scaler (`core.scaler_count`) and the re-run watch (`watch`, `refcheck::queue_reruns`)
//! read: no pod is started for the backfill's day blocks, ordered passes or re-runs, and no re-run is
//! queued, until `resume` clears it. The daily session's work (priority 1, C-051) is never touched; a
//! running re-run finishes. `resume` queues nothing again: the cancelled blocks are scheduled again by hand
//! (`vyreon-core schedule`), as the owner decides.
use crate::notice;
use crate::work::{self, PUBLISH_LOCK};
use tokio_postgres::{Client, GenericClient};

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The flag as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub stopped: bool,
    pub reason: Option<String>,
    /// When it was last stopped or resumed (RFC 3339, UTC); None: never.
    pub changed_at: Option<String>,
}

impl Status {
    pub fn line(&self) -> String {
        match (self.stopped, &self.reason, &self.changed_at) {
            (true, Some(r), Some(at)) => format!("backfill STOPPED since {at}: {r}"),
            (true, Some(r), None) => format!("backfill STOPPED: {r}"),
            (_, _, Some(at)) => format!("backfill running (resumed {at})"),
            _ => "backfill running".to_owned(),
        }
    }
}

/// A job `stop` cancelled: (job, kind, first session, last session, state it was in).
pub type Cancelled = (i64, String, String, String, String);

pub async fn status(c: &impl GenericClient) -> Result<Status, String> {
    let r = c
        .query_one(
            "SELECT backfill_stopped, backfill_reason, to_char(backfill_changed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') FROM core.queue_control",
            &[],
        )
        .await
        .map_err(db)?;
    Ok(Status { stopped: r.get(0), reason: r.get(1), changed_at: r.get(2) })
}

/// Stops the backfill: the flag set with `reason`, and cancelled (`work::cancel_in`) every QUEUED or LEASED
/// day block and ordered pass that is not a daily session's (priority 0), and every QUEUED reference-check
/// re-run. Stopping a stopped backfill keeps its first reason and cancels what was queued since. Returns
/// the jobs cancelled.
pub async fn stop(client: &mut Client, reason: &str) -> Result<Vec<Cancelled>, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("a stop needs a reason".into());
    }
    let tx = client.transaction().await.map_err(db)?;
    // the locks a cancel and a schedule take, in their order (`work::cancel`, `work::schedule`): a block
    // queued or a re-run queued (`refcheck::queue_reruns` tries the scheduler's lock) is either seen here
    // and cancelled, or comes after and reads the flag
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    tx.execute("SELECT FROM core.queue_control FOR UPDATE", &[]).await.map_err(db)?;
    let set = tx
        .execute(
            "UPDATE core.queue_control SET backfill_stopped = true, backfill_reason = $1, backfill_changed_at = clock_timestamp() WHERE NOT backfill_stopped",
            &[&reason],
        )
        .await
        .map_err(db)?;
    let rows = tx
        .query(
            "SELECT job_id, kind, first_session::text, last_session::text, state FROM core.job
             WHERE (kind IN ('DAY_BLOCK', 'ORDERED_PASS') AND priority <> 1 AND state IN ('QUEUED', 'LEASED'))
                OR (kind = 'REFCHECK' AND state = 'QUEUED')
             ORDER BY kind, first_session, job_id",
            &[],
        )
        .await
        .map_err(db)?;
    let jobs: Vec<Cancelled> = rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4))).collect();
    let ids: Vec<i64> = jobs.iter().map(|j| j.0).collect();
    let cancelled = work::cancel_in(&tx, &ids).await?;
    let jobs: Vec<Cancelled> = jobs.into_iter().filter(|j| cancelled.contains(&j.0)).collect();
    let count = |kind: &str| jobs.iter().filter(|j| j.1 == kind).count();
    let what = format!(
        "{} day blocks, {} ordered passes and {} reference-check re-runs cancelled",
        count("DAY_BLOCK"),
        count("ORDERED_PASS"),
        count("REFCHECK")
    );
    if set == 1 {
        notice::log(&tx, &format!("Backfill stopped ({what}; no backfill block, pass or re-run is started or queued until `vyreon-core backfill resume`): {reason}")).await?;
    } else {
        notice::log(&tx, &format!("Backfill already stopped, stopped again ({what}): {reason}")).await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(jobs)
}

/// Clears the flag. True when it was set.
pub async fn resume(client: &mut Client) -> Result<bool, String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT FROM core.queue_control FOR UPDATE", &[]).await.map_err(db)?;
    let was: Option<String> = tx.query_one("SELECT backfill_reason FROM core.queue_control", &[]).await.map_err(db)?.get(0);
    let resumed = tx
        .execute("UPDATE core.queue_control SET backfill_stopped = false, backfill_reason = NULL, backfill_changed_at = clock_timestamp() WHERE backfill_stopped", &[])
        .await
        .map_err(db)?;
    if resumed == 1 {
        let was = was.unwrap_or_default();
        notice::log(&tx, &format!("Backfill resumed (its cancelled blocks are not queued again by this: schedule them); it was stopped: {was}")).await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(resumed == 1)
}

/// Whether the backfill is stopped (the watch's read).
pub async fn stopped(c: &impl GenericClient) -> Result<bool, String> {
    Ok(c.query_one("SELECT backfill_stopped FROM core.queue_control", &[]).await.map_err(db)?.get(0))
}
