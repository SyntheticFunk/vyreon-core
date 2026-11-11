//! The job queue (migration 0005): scheduling work over trading sessions, claiming it under a
//! lease, renewing, finishing and failing it, and pausing the queue.
//!
//! A worker claims one job and gets a lease token; every later call names the token. A lease that
//! is not renewed expires: the job goes back to the queue and resumes at its first session not
//! done. A job that fails `max_attempts` times stays FAILED and pauses the queue, as does any
//! failed check: nothing more is claimed until someone has looked (ARCHITECTURE 5.1).
use serde_json::Value;
use crate::notice;
use tokio_postgres::{Client, GenericClient};

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tape,
    DayBlock,
    OrderedPass,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Tape => "TAPE",
            Kind::DayBlock => "DAY_BLOCK",
            Kind::OrderedPass => "ORDERED_PASS",
        }
    }

    pub fn parse(text: &str) -> Result<Kind, String> {
        match text.to_ascii_uppercase().replace('-', "_").as_str() {
            "TAPE" => Ok(Kind::Tape),
            "DAY_BLOCK" => Ok(Kind::DayBlock),
            "ORDERED_PASS" => Ok(Kind::OrderedPass),
            other => Err(format!("unknown job kind {other} (tape, day-block, ordered-pass)")),
        }
    }
}

/// Stores the engine's session list: `{"calendar_sha256": "...", "sessions": ["YYYY-MM-DD", ...]}`.
/// Sessions are only ever added; a list that lacks a stored session, or comes from another
/// calendar, is refused.
pub async fn load_calendar(client: &mut Client, calendar: &Value) -> Result<u64, String> {
    let digest = calendar["calendar_sha256"].as_str().ok_or("calendar: no calendar_sha256")?;
    if calendar["sessions"].as_array().is_none_or(Vec::is_empty) {
        return Err("calendar: no sessions".into());
    }
    let text = calendar.to_string();
    let tx = client.transaction().await.map_err(db)?;
    let dropped: i64 = tx
        .query_one(
            "SELECT count(*) FROM core.session s
             WHERE NOT EXISTS (SELECT FROM jsonb_array_elements_text($1::text::jsonb->'sessions') AS n(d) WHERE n.d::date = s.session)",
            &[&text],
        )
        .await
        .map_err(db)?
        .get(0);
    if dropped != 0 {
        return Err(format!("calendar: {dropped} stored sessions are not in the new list"));
    }
    let added = tx
        .execute(
            "INSERT INTO core.session (session, calendar_sha256)
             SELECT d::date, $2 FROM jsonb_array_elements_text($1::text::jsonb->'sessions') AS n(d)
             ON CONFLICT (session) DO UPDATE SET calendar_sha256 = EXCLUDED.calendar_sha256",
            &[&text, &digest],
        )
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(added)
}

/// What a scheduling request made.
#[derive(Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub jobs: u64,
    pub sessions: i64,
    pub partitions: i32,
}

/// Turns "these sessions, with this build" into jobs: one per session for tapes and the ordered
/// pass, blocks of `block` consecutive sessions for day blocks. The store's partitions for the
/// range are created first, in their own transaction (C-021). A range that touches an unfinished
/// job of the same kind and profile is refused.
pub async fn schedule(client: &mut Client, kind: Kind, profile: &str, build: &str, first: &str, last: &str, block: i32) -> Result<Scheduled, String> {
    if kind == Kind::DayBlock && block < 1 {
        return Err("a day block holds at least one session".into());
    }
    let block = if kind == Kind::DayBlock { block } else { 1 };
    if !core_contract::is_date(first) || !core_contract::is_date(last) {
        return Err(format!("{first} to {last}: dates are written YYYY-MM-DD"));
    }
    let sessions: i64 = client
        .query_one("SELECT count(*) FROM core.session WHERE session BETWEEN $1::text::date AND $2::text::date", &[&first, &last])
        .await
        .map_err(db)?
        .get(0);
    if sessions == 0 {
        return Err(format!("no trading session from {first} to {last}"));
    }
    if client.query_opt("SELECT FROM core.engine_build WHERE engine_build = $1", &[&build]).await.map_err(db)?.is_none() {
        return Err(format!("engine build {build} is not registered"));
    }
    let partitions: i32 = if kind == Kind::Tape {
        0
    } else {
        client.query_one("SELECT vyreon.ensure_months($1::text::date, $2::text::date)", &[&first, &last]).await.map_err(db)?.get(0)
    };
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    let overlap: Option<i64> = tx
        .query_opt(
            "SELECT job_id FROM core.job
             WHERE kind = $1 AND profile_id = $2 AND state IN ('QUEUED', 'LEASED', 'FAILED')
               AND first_session <= $4::text::date AND last_session >= $3::text::date LIMIT 1",
            &[&kind.name(), &profile, &first, &last],
        )
        .await
        .map_err(db)?
        .map(|r| r.get(0));
    if let Some(job) = overlap {
        return Err(format!("{first} to {last} overlaps unfinished {} job {job}", kind.name()));
    }
    let jobs = tx
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session)
             SELECT $1, $2, $3, min(session), max(session), min(session)
             FROM (SELECT session, (row_number() OVER (ORDER BY session) - 1) / $6 AS block
                   FROM core.session WHERE session BETWEEN $4::text::date AND $5::text::date) s
             GROUP BY block ORDER BY min(session)",
            &[&kind.name(), &profile, &build, &first, &last, &i64::from(block)],
        )
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Scheduled { jobs, sessions, partitions })
}

/// What a worker holds after a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub job_id: i64,
    pub kind: String,
    pub profile_id: String,
    pub engine_build: String,
    /// The sessions still to do, in order: a reclaimed block starts at its first undone session.
    pub sessions: Vec<String>,
    pub scope: Option<String>,
    pub attempt: i32,
    /// Names the lease in every later call. Shown once; core keeps only its digest.
    pub token: String,
    pub expires_at: String,
}

/// Ends the leases that were not renewed in time. Their jobs go back to the queue, or to FAILED
/// (pausing the queue) once out of attempts. Returns how many leases expired.
pub async fn sweep(db_client: &impl GenericClient) -> Result<u64, String> {
    let expired = db_client
        .query(
            "UPDATE core.lease SET ended_at = now(), outcome = 'EXPIRED', failure = 'the lease was not renewed'
             WHERE ended_at IS NULL AND expires_at < now() RETURNING job_id",
            &[],
        )
        .await
        .map_err(db)?;
    for row in &expired {
        requeue(db_client, row.get(0), "its lease expired").await?;
    }
    Ok(expired.len() as u64)
}

/// A job whose attempt ended without finishing: queued again, or FAILED with the queue paused.
async fn requeue(db_client: &impl GenericClient, job: i64, why: &str) -> Result<bool, String> {
    let again: bool = db_client
        .query_one(
            "UPDATE core.job SET state = CASE WHEN attempts < max_attempts THEN 'QUEUED' ELSE 'FAILED' END,
                                 finished_at = CASE WHEN attempts < max_attempts THEN NULL ELSE now() END
             WHERE job_id = $1 AND state = 'LEASED' RETURNING state = 'QUEUED'",
            &[&job],
        )
        .await
        .map_err(db)?
        .get(0);
    if !again {
        pause(db_client, &format!("job {job} failed for the last time: {why}")).await?;
    }
    Ok(again)
}

/// Claims the earliest claimable job of `kind` for the worker's own build, if there is one.
pub async fn claim(client: &mut Client, kind: Kind, build: &str, worker: &str, lease_seconds: f64) -> Result<Option<Claim>, String> {
    let tx = client.transaction().await.map_err(db)?;
    sweep(&tx).await?;
    let Some(job) = tx
        .query_opt(
            "UPDATE core.job SET state = 'LEASED', attempts = attempts + 1
             WHERE job_id = (SELECT j.job_id FROM core.job j
                             WHERE j.job_id IN (SELECT job_id FROM core.claimable_job WHERE kind = $1 AND engine_build = $2)
                             ORDER BY j.first_session, j.job_id LIMIT 1 FOR UPDATE SKIP LOCKED)
               AND state = 'QUEUED'
             RETURNING job_id, profile_id, scope::text, attempts, next_session::text, last_session::text",
            &[&kind.name(), &build],
        )
        .await
        .map_err(db)?
    else {
        tx.commit().await.map_err(db)?; // keeps the sweep
        return Ok(None);
    };
    let (job_id, profile_id, scope, attempt, next, last): (i64, String, Option<String>, i32, String, String) =
        (job.get(0), job.get(1), job.get(2), job.get(3), job.get(4), job.get(5));
    let lease = tx
        .query_one(
            "WITH t AS (SELECT replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '') AS token)
             INSERT INTO core.lease (job_id, worker, token_sha256, expires_at)
             SELECT $1, $2, encode(sha256(convert_to(token, 'UTF8')), 'hex'), now() + make_interval(secs => $3) FROM t
             RETURNING (SELECT token FROM t), to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')",
            &[&job_id, &worker, &lease_seconds],
        )
        .await
        .map_err(db)?;
    let sessions = tx
        .query("SELECT session::text FROM core.session WHERE session BETWEEN $1::text::date AND $2::text::date ORDER BY 1", &[&next, &last])
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    tx.commit().await.map_err(db)?;
    Ok(Some(Claim { job_id, kind: kind.name().to_owned(), profile_id, engine_build: build.to_owned(), sessions, scope, attempt, token: lease.get(0), expires_at: lease.get(1) }))
}

/// Why a call on a lease was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    /// No live lease has this token: it expired, ended, or never was. The worker must stop; the
    /// job may already be someone else's.
    Gone,
    /// The call does not fit the lease's job.
    Refused(String),
    Failed(String),
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LeaseError::Gone => write!(f, "no live lease has this token"),
            LeaseError::Refused(e) => write!(f, "refused: {e}"),
            LeaseError::Failed(e) => write!(f, "failed: {e}"),
        }
    }
}

fn failed(e: tokio_postgres::Error) -> LeaseError {
    LeaseError::Failed(db(e))
}

/// A live lease and its job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub lease_id: i64,
    pub worker: String,
    pub job_id: i64,
    pub kind: String,
    pub profile_id: String,
    pub engine_build: String,
    pub next_session: Option<String>,
    pub last_session: String,
}

async fn find(db_client: &impl GenericClient, token: &str, lock: &str) -> Result<Held, LeaseError> {
    let row = db_client
        .query_opt(
            &format!(
                "SELECT l.lease_id, l.worker, j.job_id, j.kind, j.profile_id, j.engine_build, j.next_session::text, j.last_session::text
                 FROM core.lease l JOIN core.job j USING (job_id)
                 WHERE l.token_sha256 = encode(sha256(convert_to($1, 'UTF8')), 'hex') AND l.ended_at IS NULL AND l.expires_at >= now()
                   AND j.state = 'LEASED' {lock}"
            ),
            &[&token],
        )
        .await
        .map_err(failed)?
        .ok_or(LeaseError::Gone)?;
    Ok(Held {
        lease_id: row.get(0),
        worker: row.get(1),
        job_id: row.get(2),
        kind: row.get(3),
        profile_id: row.get(4),
        engine_build: row.get(5),
        next_session: row.get(6),
        last_session: row.get(7),
    })
}

/// Finds the live lease the token names and locks it and its job for the transaction.
pub async fn held(db_client: &impl GenericClient, token: &str) -> Result<Held, LeaseError> {
    find(db_client, token, "FOR UPDATE OF l, j").await
}

/// Finds the live lease the token names, without locking: who is calling.
pub async fn identify(db_client: &impl GenericClient, token: &str) -> Result<Held, LeaseError> {
    find(db_client, token, "").await
}

/// Renews a lease. Returns its new expiry and whether the queue is paused (a worker may finish
/// the day it is on, then should stop).
pub async fn heartbeat(client: &mut Client, token: &str, lease_seconds: f64) -> Result<(String, bool), LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    let row = tx
        .query_one(
            "UPDATE core.lease SET expires_at = now() + make_interval(secs => $2) WHERE lease_id = $1
             RETURNING to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), (SELECT paused FROM core.queue_control)",
            &[&lease.lease_id, &lease_seconds],
        )
        .await
        .map_err(failed)?;
    tx.commit().await.map_err(failed)?;
    Ok((row.get(0), row.get(1)))
}

/// Marks the lease's next session done and moves the job to the following one. The sessions of a
/// job are done strictly in order: `session` must be the job's next.
pub async fn session_done(db_client: &impl GenericClient, lease: &Held, session: &str) -> Result<(), LeaseError> {
    if lease.next_session.as_deref() != Some(session) {
        return Err(LeaseError::Refused(format!("job {}'s next session is {}, not {session}", lease.job_id, lease.next_session.as_deref().unwrap_or("none: all are done"))));
    }
    db_client
        .execute(
            "UPDATE core.job SET next_session = (SELECT min(session) FROM core.session WHERE session > $2::text::date AND session <= last_session)
             WHERE job_id = $1",
            &[&lease.job_id, &session],
        )
        .await
        .map_err(failed)?;
    Ok(())
}

/// Marks a day block's next day done, under its lease, once the worker has published every bundle
/// of it: the day's base revision from the job's build must be the published one. What else a
/// build publishes with a day (its day-local derived profile, C-031) is the engine's to send
/// before this; the ordered pass refuses a day without it.
pub async fn day_done(client: &mut Client, token: &str, session: &str) -> Result<(), LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    if lease.kind != "DAY_BLOCK" {
        return Err(LeaseError::Refused(format!("job {} is not a day block", lease.job_id)));
    }
    let published = tx
        .query_opt(
            "SELECT FROM vyreon.day_revision
             WHERE represented_date = $1::text::date AND profile_id = $2 AND engine_build = $3 AND state = 'PUBLISHED' AND base_revision IS NULL",
            &[&session, &lease.profile_id, &lease.engine_build],
        )
        .await
        .map_err(failed)?;
    if published.is_none() {
        return Err(LeaseError::Refused(format!("job {} has not published {session}", lease.job_id)));
    }
    session_done(&tx, &lease, session).await?;
    tx.commit().await.map_err(failed)
}

/// Ends the lease with the job done. Refused while sessions remain.
pub async fn finish(client: &mut Client, token: &str) -> Result<i64, LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    if let Some(next) = &lease.next_session {
        return Err(LeaseError::Refused(format!("job {} is not done: {next} to {} remain", lease.job_id, lease.last_session)));
    }
    tx.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE lease_id = $1", &[&lease.lease_id]).await.map_err(failed)?;
    let job = tx
        .query_one(
            "UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1
             RETURNING first_session::text, last_session::text,
                       (SELECT count(*) FROM core.job o WHERE o.kind = core.job.kind AND o.job_id <> core.job.job_id AND o.state IN ('QUEUED', 'LEASED', 'FAILED'))",
            &[&lease.job_id],
        )
        .await
        .map_err(failed)?;
    let (first, last, remaining): (String, String, i64) = (job.get(0), job.get(1), job.get(2));
    // one line per finished day block; for the many small jobs, only when the last one is in
    if lease.kind == "DAY_BLOCK" {
        let text = format!("Day block {first} to {last} done by {} ({remaining} blocks to go).", lease.worker);
        notice::post(&tx, &text).await.map_err(LeaseError::Failed)?;
    } else if remaining == 0 {
        let what = if lease.kind == "TAPE" { "tape" } else { "ordered pass" };
        notice::post(&tx, &format!("Every scheduled {what} job is done (the last: {last}).")).await.map_err(LeaseError::Failed)?;
    }
    tx.commit().await.map_err(failed)?;
    Ok(lease.job_id)
}

/// Ends the lease with the worker's account of its failure: the stage, a message and the day's
/// log. Returns true when the job was queued again, false when it is out of attempts (the queue
/// is then paused).
pub async fn fail(client: &mut Client, token: &str, stage: &str, message: &str, log: &str) -> Result<bool, LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    tx.execute(
        "UPDATE core.lease SET ended_at = now(), outcome = 'FAILED', failed_stage = $2, failure = $3, failure_log = $4 WHERE lease_id = $1",
        &[&lease.lease_id, &stage, &message, &log],
    )
    .await
    .map_err(failed)?;
    let again = requeue(&tx, lease.job_id, &format!("{stage}: {message}")).await.map_err(LeaseError::Failed)?;
    tx.commit().await.map_err(failed)?;
    Ok(again)
}

/// Stops claims until `resume`. The first reason is kept: it is what stopped the queue.
pub async fn pause(db_client: &impl GenericClient, reason: &str) -> Result<(), String> {
    let paused = db_client
        .execute("UPDATE core.queue_control SET paused = true, reason = $1, changed_at = now() WHERE NOT paused", &[&reason])
        .await
        .map_err(db)?;
    if paused == 1 {
        notice::post(db_client, &format!("Queue paused: {reason}. No work is handed out until `vyreon-core queue resume`.")).await?;
    }
    Ok(())
}

/// Lets work be claimed again. Returns the reason the queue was paused for, if it was.
pub async fn resume(db_client: &impl GenericClient) -> Result<Option<String>, String> {
    let was: Option<String> = db_client.query_one("SELECT reason FROM core.queue_control", &[]).await.map_err(db)?.get(0);
    let resumed = db_client.execute("UPDATE core.queue_control SET paused = false, reason = NULL, changed_at = now() WHERE paused", &[]).await.map_err(db)?;
    if resumed == 1 {
        notice::post(db_client, "Queue resumed.").await?;
    }
    Ok(was)
}

/// Puts FAILED jobs back in the queue with their attempts reset (after the cause is fixed).
pub async fn retry_failed(db_client: &impl GenericClient) -> Result<u64, String> {
    db_client.execute("UPDATE core.job SET state = 'QUEUED', attempts = 0, finished_at = NULL WHERE state = 'FAILED'", &[]).await.map_err(db)
}

/// The queue at a glance: jobs by kind and state, what is claimable, and the pause.
pub async fn status(db_client: &impl GenericClient) -> Result<Value, String> {
    let row = db_client
        .query_one(
            "SELECT jsonb_build_object(
                 'paused', (SELECT paused FROM core.queue_control),
                 'pause_reason', (SELECT reason FROM core.queue_control),
                 'jobs', COALESCE((SELECT jsonb_agg(jsonb_build_object('kind', kind, 'state', state, 'jobs', n, 'first', first, 'last', last) ORDER BY kind, state)
                                   FROM (SELECT kind, state, count(*) AS n, min(first_session) AS first, max(last_session) AS last
                                         FROM core.job GROUP BY kind, state) j), '[]'::jsonb),
                 'claimable', COALESCE((SELECT jsonb_object_agg(kind, n) FROM (SELECT kind, count(*) AS n FROM core.claimable_job GROUP BY kind) c), '{}'::jsonb),
                 'live_leases', (SELECT count(*) FROM core.lease WHERE ended_at IS NULL))::text",
            &[],
        )
        .await
        .map_err(db)?;
    serde_json::from_str(row.get(0)).map_err(|e| e.to_string())
}
