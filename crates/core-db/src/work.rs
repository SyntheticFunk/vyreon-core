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
    /// Ordered passes: sessions whose unfinished pass of this build was kept (C-062).
    pub kept: u64,
    /// Ordered passes: unfinished passes of a build that does not run, cancelled and replaced.
    pub replaced: Vec<i64>,
}

/// Turns "these sessions, with this build" into jobs: one per session for tapes and the ordered
/// pass, blocks of `block` consecutive sessions for day blocks. The store's partitions for the
/// range are created first, in their own transaction (C-021). A range that touches an unfinished
/// tape job or day block of the profile is refused. Ordered passes (C-062): one unfinished pass
/// per session and profile; a session whose unfinished pass is of this build keeps it, one of a
/// build that does not run (not admitted, or withdrawn) has it cancelled and replaced, and one of
/// another build that runs refuses the range (withdraw that build or cancel its pass first). DONE
/// and CANCELLED passes of any build are left as they are.
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
    if kind == Kind::OrderedPass {
        let (kept, replaced) = passes_in_range(&tx, profile, build, first, last).await?;
        let jobs = tx
            .execute(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, priority)
                 SELECT 'ORDERED_PASS', $1, $2, s.session, s.session, s.session,
                        COALESCE((SELECT max(r.priority) FROM core.job r WHERE r.job_id = ANY($5) AND r.first_session = s.session), 0)
                 FROM core.session s
                 WHERE s.session BETWEEN $3::text::date AND $4::text::date
                   AND NOT EXISTS (SELECT FROM core.job u WHERE u.kind = 'ORDERED_PASS' AND u.profile_id = $1 AND u.first_session = s.session
                                     AND u.state IN ('QUEUED', 'LEASED', 'FAILED'))
                 ORDER BY s.session",
                &[&profile, &build, &first, &last, &replaced],
            )
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        return Ok(Scheduled { jobs, sessions, partitions, kept, replaced });
    }
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
    Ok(Scheduled { jobs, sessions, partitions, kept: 0, replaced: Vec::new() })
}

/// C-062: the unfinished ordered passes of the profile in `first..=last` before passes of `build`
/// are scheduled there. One of another build that runs refuses the range; one of a build that does
/// not run is cancelled (it would never be claimed, and a later pass could not run in its place);
/// one of `build` stays. Returns (kept, cancelled), with a logged notice when any was cancelled.
async fn passes_in_range(tx: &impl GenericClient, profile: &str, build: &str, first: &str, last: &str) -> Result<(u64, Vec<i64>), String> {
    let rows = tx
        .query(
            "SELECT job_id, engine_build, state, first_session::text,
                    EXISTS (SELECT FROM core.engine_build b WHERE b.engine_build = core.job.engine_build AND b.admitted_at IS NOT NULL AND b.withdrawn_at IS NULL)
             FROM core.job
             WHERE kind = 'ORDERED_PASS' AND profile_id = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED')
               AND first_session BETWEEN $2::text::date AND $3::text::date
             ORDER BY first_session FOR UPDATE",
            &[&profile, &first, &last],
        )
        .await
        .map_err(db)?;
    let (mut kept, mut replaced) = (0, Vec::new());
    for r in &rows {
        let (job, of, state, day, runs): (i64, String, String, String, bool) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
        if of == build {
            kept += 1;
        } else if runs || state == "LEASED" {
            let why = if runs { "a build that runs: withdraw it or cancel the job first" } else { "under a live lease: let it end or cancel the job first" };
            return Err(format!("{first} to {last}: {day} has unfinished ORDERED_PASS job {job} ({state}) of build {of}, {why}"));
        } else {
            replaced.push(job);
        }
    }
    if !replaced.is_empty() {
        tx.execute("UPDATE core.job SET state = 'CANCELLED', finished_at = now() WHERE job_id = ANY($1)", &[&replaced]).await.map_err(db)?;
        let shown: Vec<String> = replaced.iter().take(10).map(i64::to_string).collect();
        let more = if replaced.len() > 10 { format!(" and {} more", replaced.len() - 10) } else { String::new() };
        notice::log(
            tx,
            &format!("{} ordered passes of {profile} under builds that do not run are replaced by build {build}'s: jobs {}{more} cancelled.", replaced.len(), shown.join(" ")),
        )
        .await?;
    }
    Ok((kept, replaced))
}

/// C-059: one verify job per chosen session (a TAPE job of scope `tape::VERIFY_SCOPE`): its worker
/// downloads the session's replay files again, recomputes each member's cache with `build` and
/// reports a check per member; nothing is stored in place of a cache. Each session must hold a
/// stored tape cache and no unfinished tape job of the profile; all or none are scheduled. Returns
/// the jobs, in the sessions' order.
pub async fn schedule_verify(client: &mut Client, profile: &str, build: &str, sessions: &[&str]) -> Result<Vec<i64>, String> {
    if sessions.is_empty() {
        return Err("name at least one session to verify".into());
    }
    if let Some(bad) = sessions.iter().find(|s| !core_contract::is_date(s)) {
        return Err(format!("{bad} is not a date written YYYY-MM-DD"));
    }
    if client.query_opt("SELECT FROM core.engine_build WHERE engine_build = $1", &[&build]).await.map_err(db)?.is_none() {
        return Err(format!("engine build {build} is not registered"));
    }
    let mut sorted: Vec<&str> = sessions.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    let mut jobs = Vec::new();
    for session in sorted {
        if tx.query_opt("SELECT FROM core.session WHERE session = $1::text::date", &[&session]).await.map_err(db)?.is_none() {
            return Err(format!("{session} is not a trading session"));
        }
        if tx.query_opt("SELECT FROM core.tape_cache WHERE session = $1::text::date AND body IS NOT NULL LIMIT 1", &[&session]).await.map_err(db)?.is_none() {
            return Err(format!("core holds no tape cache of {session}: nothing to verify"));
        }
        let unfinished: Option<i64> = tx
            .query_opt(
                "SELECT job_id FROM core.job WHERE kind = 'TAPE' AND profile_id = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED')
                   AND $2::text::date BETWEEN first_session AND last_session LIMIT 1",
                &[&profile, &session],
            )
            .await
            .map_err(db)?
            .map(|r| r.get(0));
        if let Some(job) = unfinished {
            return Err(format!("{session} has unfinished tape job {job}"));
        }
        let job: i64 = tx
            .query_one(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope)
                 VALUES ('TAPE', $1, $2, $3::text::date, $3::text::date, $3::text::date, $4::text::jsonb) RETURNING job_id",
                &[&profile, &build, &session, &crate::tape::VERIFY_SCOPE],
            )
            .await
            .map_err(db)?
            .get(0);
        jobs.push(job);
    }
    tx.commit().await.map_err(db)?;
    Ok(jobs)
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

/// Ends the leases that were not renewed in time. Their jobs go back to the queue, to FAILED
/// once out of attempts, or to DONE when every session was done. Each lease is ended and its job
/// moved in one transaction of its own, so a restart between the two cannot leave a job LEASED
/// under an ended lease, and a lease that cannot be ended does not keep the others from it.
/// Returns how many leases expired; an error names every lease that could not be ended (the
/// others are done).
pub async fn sweep(client: &mut Client) -> Result<u64, String> {
    let expired: Vec<i64> = client
        .query("SELECT lease_id FROM core.lease WHERE ended_at IS NULL AND expires_at < now() ORDER BY lease_id", &[])
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let (mut ended, mut errors) = (0, Vec::new());
    for lease in expired {
        match expire(client, lease).await {
            Ok(true) => ended += 1,
            Ok(false) => {}
            Err(e) => errors.push(format!("lease {lease}: {e}")),
        }
    }
    if errors.is_empty() { Ok(ended) } else { Err(errors.join("; ")) }
}

/// Ends one expired lease and moves its job, in one transaction. False when it was renewed or
/// ended meanwhile.
async fn expire(client: &mut Client, lease: i64) -> Result<bool, String> {
    let tx = client.transaction().await.map_err(db)?;
    let Some(row) = tx
        .query_opt(
            "UPDATE core.lease SET ended_at = now(), outcome = 'EXPIRED', failure = 'the lease was not renewed'
             WHERE lease_id = $1 AND ended_at IS NULL AND expires_at < now() RETURNING job_id",
            &[&lease],
        )
        .await
        .map_err(db)?
    else {
        return Ok(false);
    };
    requeue(&tx, row.get(0), "its lease expired").await?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}

/// How many day jobs may stand FAILED before the queue stops: one bad day is set aside and the
/// rest of the build goes on; several at once look like the engine, not the day (C-035).
pub const FAILED_DAYS_PAUSE: i64 = 3;

/// A job whose attempt ended without finishing: DONE, queued again, or FAILED. A job whose every
/// session is done is DONE, whatever became of its worker's `finish` (lost, never sent, or sent
/// after a failure): there is nothing left to claim. A day block out of attempts keeps only the
/// day it stopped on: the days before it are done, the days after it become a new queued block,
/// and the queue pauses only at `FAILED_DAYS_PAUSE` failed days. An ordered pass out of attempts
/// stays FAILED without a pause; a tape job pauses the queue at once (the day blocks wait on it).
/// Returns true when the job was queued again.
async fn requeue(db_client: &impl GenericClient, job: i64, why: &str) -> Result<bool, String> {
    if let Some(done) = db_client
        .query_opt(
            &format!("UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1 AND state = 'LEASED' AND next_session IS NULL RETURNING {FINISHED}"),
            &[&job],
        )
        .await
        .map_err(db)?
    {
        finished(db_client, &done, "its worker, whose finish never arrived").await?;
        return Ok(false);
    }
    let Some(again) = db_client
        .query_opt(
            "UPDATE core.job SET state = CASE WHEN attempts < max_attempts THEN 'QUEUED' ELSE 'FAILED' END,
                                 finished_at = CASE WHEN attempts < max_attempts THEN NULL ELSE now() END
             WHERE job_id = $1 AND state = 'LEASED' RETURNING state = 'QUEUED'",
            &[&job],
        )
        .await
        .map_err(db)?
    else {
        // not under a lease any more (cancelled, or moved by a publish): nothing to move
        return Ok(false);
    };
    if again.get(0) {
        return Ok(true);
    }
    let kind: String = db_client.query_one("SELECT kind FROM core.job WHERE job_id = $1", &[&job]).await.map_err(db)?.get(0);
    if kind == "TAPE" && crate::tape::is_verify(db_client, job).await? {
        // C-059: a sample check holds nothing back (no day block waits on it, no cache is stored)
        notice::log(db_client, &format!("Tape verify job {job} failed for the last time: {why}. Nothing waits on it.")).await?;
        return Ok(false);
    }
    if kind == "ORDERED_PASS" {
        // the later days' passes wait on this one by themselves (`core.claimable_job`); the day
        // blocks do not read it, and a pass is seconds to run again once its cause is fixed
        let day: String = db_client.query_one("SELECT first_session::text FROM core.job WHERE job_id = $1", &[&job]).await.map_err(db)?.get(0);
        let local = missing_day_local(db_client, job, &day).await?;
        let wait = if local.is_empty() { "Later passes wait for it; the day blocks go on" } else { "The day blocks go on" };
        notice::log(db_client, &format!("Ordered pass failed for the last time (job {job}): {why}{local}. {wait}.")).await?;
        return Ok(false);
    }
    if kind != "DAY_BLOCK" {
        pause(db_client, &format!("job {job} failed for the last time: {why}")).await?;
        return Ok(false);
    }
    // the days before the failed one: a finished job of their own
    db_client
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, state, attempts, max_attempts, finished_at, priority)
             SELECT kind, profile_id, engine_build, first_session,
                    (SELECT max(session) FROM core.session WHERE session < j.next_session), NULL, scope, 'DONE', attempts, max_attempts, now(), priority
             FROM core.job j WHERE job_id = $1 AND next_session > first_session",
            &[&job],
        )
        .await
        .map_err(db)?;
    // the days after it: queued afresh
    let rest = db_client
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, max_attempts, priority)
             SELECT kind, profile_id, engine_build, n.session, last_session, n.session, scope, max_attempts, priority
             FROM core.job j, LATERAL (SELECT min(session) AS session FROM core.session WHERE session > j.next_session) n
             WHERE job_id = $1 AND next_session < last_session",
            &[&job],
        )
        .await
        .map_err(db)?;
    let day: String = db_client
        .query_one("UPDATE core.job SET first_session = next_session, last_session = next_session WHERE job_id = $1 RETURNING next_session::text", &[&job])
        .await
        .map_err(db)?
        .get(0);
    let failed: i64 = db_client.query_one("SELECT count(*) FROM core.job WHERE kind = 'DAY_BLOCK' AND state = 'FAILED'", &[]).await.map_err(db)?.get(0);
    let local = missing_day_local(db_client, job, &day).await?;
    if failed >= FAILED_DAYS_PAUSE {
        pause(db_client, &format!("{failed} days stand failed; the last, job {job} ({day}): {why}{local}")).await?;
    } else {
        let after = if rest == 1 { "; the rest of its block is queued again" } else { "" };
        notice::log(db_client, &format!("Day {day} failed for the last time and is set aside (job {job}): {why}{after}{local}. The build goes on.")).await?;
    }
    Ok(false)
}

/// C-061 amended (audit 2026-11-22 changes-core 2): when the job's day has its base published but
/// no day-local revision on it, the clause that says so (the ordered pass refuses such a day, so it
/// is a missing day for the passes); else empty.
async fn missing_day_local(db_client: &impl GenericClient, job: i64, day: &str) -> Result<String, String> {
    let base: Option<i64> = db_client
        .query_one("SELECT core.base_without_day_local($2::text::date, (SELECT profile_id FROM core.job WHERE job_id = $1))", &[&job, &day])
        .await
        .map_err(db)?
        .get(0);
    Ok(base.map_or_else(String::new, |b| {
        format!(
            "; its base revision {b} is published but no day-local derived revision on it: the ordered passes read {day} as missing \
             (`vyreon-core queue retry-failed` computes the day again)"
        )
    }))
}

/// Taken by every publish before any row lock, and by the claim of an ordered pass: a pass is then
/// claimed either before a publish (which stops it if it reads that day) or after it (and reads
/// what it published), never in between (`publish`).
pub const PUBLISH_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended('vyreon publish', 0))";

/// Claims the earliest claimable job of `kind` for the worker's own build, if there is one; a daily
/// run's job (priority 1, C-051) before any other.
pub async fn claim(client: &mut Client, kind: Kind, build: &str, worker: &str, lease_seconds: f64) -> Result<Option<Claim>, String> {
    claim_as(client, kind, build, worker, lease_seconds, false).await
}

/// `claim`; with `daily_only`, only a daily run's job (priority 1): the daily CronJob's worker
/// never takes a backfill block (C-051 amended, audit round x, 13).
pub async fn claim_as(client: &mut Client, kind: Kind, build: &str, worker: &str, lease_seconds: f64, daily_only: bool) -> Result<Option<Claim>, String> {
    // expired leases end first, each in its own transaction; one that cannot be ended is reported
    // by the service's sweeper every half minute and does not stop this claim
    let _ = sweep(client).await;
    let tx = client.transaction().await.map_err(db)?;
    if kind == Kind::OrderedPass {
        tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    }
    let Some(job) = tx
        .query_opt(
            "UPDATE core.job SET state = 'LEASED', attempts = attempts + 1
             WHERE job_id = (SELECT j.job_id FROM core.job j
                             WHERE j.job_id IN (SELECT job_id FROM core.claimable_job WHERE kind = $1 AND engine_build = $2 AND next_session IS NOT NULL)
                               AND (NOT $3 OR j.priority = 1)
                             ORDER BY j.priority DESC, j.first_session, j.job_id LIMIT 1 FOR UPDATE SKIP LOCKED)
               AND state = 'QUEUED'
             RETURNING job_id, profile_id, scope::text, attempts, next_session::text, last_session::text",
            &[&kind.name(), &build, &daily_only],
        )
        .await
        .map_err(db)?
    else {
        return Ok(None);
    };
    let (job_id, profile_id, scope, attempt, next, last): (i64, String, Option<String>, i32, Option<String>, String) =
        (job.get(0), job.get(1), job.get(2), job.get(3), job.get(4), job.get(5));
    let next = next.ok_or_else(|| format!("job {job_id} has no session left to do"))?;
    let lease = tx
        .query_one(
            "WITH t AS (SELECT replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '') AS token)
             INSERT INTO core.lease (job_id, worker, token_sha256, granted_at, expires_at)
             SELECT $1, $2, encode(sha256(convert_to(token, 'UTF8')), 'hex'), clock_timestamp(), now() + make_interval(secs => $3) FROM t
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
    pub first_session: String,
    pub next_session: Option<String>,
    pub last_session: String,
}

async fn find(db_client: &impl GenericClient, token: &str, lock: &str) -> Result<Held, LeaseError> {
    let row = db_client
        .query_opt(
            &format!(
                "SELECT l.lease_id, l.worker, j.job_id, j.kind, j.profile_id, j.engine_build, j.next_session::text, j.last_session::text, j.first_session::text
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
        first_session: row.get(8),
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
/// job are done strictly in order: `session` must be the job's next. A session of the job already
/// done is accepted and changes nothing: the call was sent again after its answer was lost.
pub async fn session_done(db_client: &impl GenericClient, lease: &Held, session: &str) -> Result<(), LeaseError> {
    let already = session >= lease.first_session.as_str() && session <= lease.last_session.as_str() && lease.next_session.as_deref().is_none_or(|next| session < next);
    if already {
        return Ok(());
    }
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

/// Ends the lease with the job done. Refused while sessions remain. Sent again, or late (after the
/// lease expired with every session done, when the sweep has made the job DONE), it is answered
/// the same way and changes nothing.
pub async fn finish(client: &mut Client, token: &str) -> Result<i64, LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = match held(&tx, token).await {
        Ok(lease) => lease,
        Err(LeaseError::Gone) => {
            let done = tx
                .query_opt(
                    "SELECT j.job_id FROM core.lease l JOIN core.job j USING (job_id)
                     WHERE l.token_sha256 = encode(sha256(convert_to($1, 'UTF8')), 'hex') AND j.state = 'DONE'",
                    &[&token],
                )
                .await
                .map_err(failed)?;
            return done.map(|r| r.get(0)).ok_or(LeaseError::Gone);
        }
        Err(e) => return Err(e),
    };
    if let Some(next) = &lease.next_session {
        return Err(LeaseError::Refused(format!("job {} is not done: {next} to {} remain", lease.job_id, lease.last_session)));
    }
    tx.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE lease_id = $1", &[&lease.lease_id]).await.map_err(failed)?;
    let job = tx
        .query_one(&format!("UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1 RETURNING {FINISHED}"), &[&lease.job_id])
        .await
        .map_err(failed)?;
    finished(&tx, &job, &lease.worker).await.map_err(LeaseError::Failed)?;
    tx.commit().await.map_err(failed)?;
    Ok(lease.job_id)
}

/// What `finished` reads of a job just made DONE.
const FINISHED: &str = "kind, first_session::text, last_session::text,
    (SELECT count(*) FROM core.job o WHERE o.kind = core.job.kind AND o.job_id <> core.job.job_id AND o.state IN ('QUEUED', 'LEASED', 'FAILED'))";

/// The log line of a finished job (C-050: logged, not posted): one per day block; for the many
/// small jobs, only when the last one is in.
async fn finished(db_client: &impl GenericClient, job: &tokio_postgres::Row, by: &str) -> Result<(), String> {
    let (kind, first, last, remaining): (String, String, String, i64) = (job.get(0), job.get(1), job.get(2), job.get(3));
    if kind == "DAY_BLOCK" {
        notice::log(db_client, &format!("Day block {first} to {last} done by {by} ({remaining} blocks to go).")).await
    } else if remaining == 0 {
        let what = if kind == "TAPE" { "tape" } else { "ordered pass" };
        notice::log(db_client, &format!("Every scheduled {what} job is done (the last: {last}).")).await
    } else {
        Ok(())
    }
}

/// Ends the lease with the worker's account of its failure: the stage, a message and the day's
/// log. Returns true when the job was queued again, false when it is out of attempts (the queue
/// is then paused).
pub async fn fail(client: &mut Client, token: &str, stage: &str, message: &str, log: &str) -> Result<bool, LeaseError> {
    // round cc, 11 (C-049's principle): a credential pattern the worker's redaction missed is never
    // stored, logged or printed: the text is withheld whole
    let (stage, message, log) = (&scrubbed(stage), &scrubbed(message), &scrubbed(log));
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

/// A worker's text as core keeps it: whole, or withheld whole when a credential pattern is in it
/// (`evidence::unredacted`, the patterns C-049's evidence is refused on).
pub fn scrubbed(text: &str) -> String {
    match crate::evidence::unredacted(text) {
        Some(found) => format!("(not kept: it carried a credential pattern, {found})"),
        None => text.to_owned(),
    }
}

/// Stops claims until `resume`. The first reason is kept: it is what stopped the queue. Call it in
/// a transaction: the pause and its notice are one event (C-027), and a restart between two
/// autocommitted statements would leave the queue paused with no notice (a second `pause` then
/// changes nothing, so would not repair it).
pub async fn pause(db_client: &impl GenericClient, reason: &str) -> Result<(), String> {
    let reason = &scrubbed(reason);
    let paused = db_client
        .execute("UPDATE core.queue_control SET paused = true, reason = $1, changed_at = now() WHERE NOT paused", &[&reason])
        .await
        .map_err(db)?;
    if paused == 1 {
        notice::log(db_client, &format!("Queue paused: {reason}. No work is handed out until `vyreon-core queue resume`.")).await?;
    }
    Ok(())
}

/// Lets work be claimed again. Returns the reason the queue was paused for, if it was. Call it in
/// a transaction, as `pause`.
pub async fn resume(db_client: &impl GenericClient) -> Result<Option<String>, String> {
    let was: Option<String> = db_client.query_one("SELECT reason FROM core.queue_control", &[]).await.map_err(db)?.get(0);
    let resumed = db_client.execute("UPDATE core.queue_control SET paused = false, reason = NULL, changed_at = now() WHERE paused", &[]).await.map_err(db)?;
    if resumed == 1 {
        notice::log(db_client, "Queue resumed.").await?;
    }
    Ok(was)
}

/// Puts FAILED jobs back in the queue with their attempts reset (after the cause is fixed). One
/// whose every session was done is DONE instead: there is nothing left to claim. Returns how many
/// were queued again.
pub async fn retry_failed(db_client: &impl GenericClient) -> Result<u64, String> {
    let rows = db_client
        .query(
            "UPDATE core.job SET state = CASE WHEN next_session IS NULL THEN 'DONE' ELSE 'QUEUED' END,
                                 attempts = CASE WHEN next_session IS NULL THEN attempts ELSE 0 END,
                                 finished_at = CASE WHEN next_session IS NULL THEN finished_at END
             WHERE state = 'FAILED' RETURNING state = 'QUEUED'",
            &[],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().filter(|r| r.get::<_, bool>(0)).count() as u64)
}

/// Withdraws jobs that are not finished: each becomes CANCELLED and any live lease on it ends, so
/// its worker is refused at its next heartbeat or publish. Finished jobs are left as they are.
/// An ordered pass cancelled on a day whose base is published, with no history revision of its
/// profile published (as the watch resolves NOT_DERIVED) and no other pass of that day to come,
/// leaves the day's history absent: a NOT_DERIVED problem, in the same transaction (audit round
/// x, 2: a repair a republish queued, then cancelled). Returns how many jobs were cancelled.
pub async fn cancel(client: &mut Client, jobs: &[i64]) -> Result<u64, String> {
    let tx = client.transaction().await.map_err(db)?;
    let cancelled = cancel_in(&tx, jobs).await?;
    tx.commit().await.map_err(db)?;
    Ok(cancelled)
}

/// `cancel` inside the caller's transaction.
pub(crate) async fn cancel_in(tx: &impl GenericClient, jobs: &[i64]) -> Result<u64, String> {
    let cancelled: Vec<i64> = tx
        .query(
            "UPDATE core.job SET state = 'CANCELLED', finished_at = now() WHERE job_id = ANY($1) AND state IN ('QUEUED', 'LEASED', 'FAILED') RETURNING job_id",
            &[&jobs],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    tx.execute(
        "UPDATE core.lease SET ended_at = now(), outcome = 'CANCELLED', failure = 'the job was cancelled' WHERE job_id = ANY($1) AND ended_at IS NULL",
        &[&jobs],
    )
    .await
    .map_err(db)?;
    let absent: Vec<(String, String, bool)> = tx
        .query(
            "SELECT DISTINCT j.first_session::text, j.profile_id FROM core.job j
             WHERE j.job_id = ANY($1) AND j.kind = 'ORDERED_PASS'
               AND EXISTS (SELECT FROM vyreon.day_revision b WHERE b.represented_date = j.first_session AND b.state = 'PUBLISHED' AND b.base_revision IS NULL)
               AND NOT EXISTS (SELECT FROM vyreon.day_revision r WHERE r.represented_date = j.first_session AND r.profile_id = j.profile_id AND r.state = 'PUBLISHED'
                                 AND r.base_revision IS NOT NULL AND NOT r.day_local)
               AND NOT EXISTS (SELECT FROM core.job u WHERE u.kind = 'ORDERED_PASS' AND u.profile_id = j.profile_id AND u.first_session = j.first_session
                                 AND u.state IN ('QUEUED', 'LEASED', 'FAILED'))
             ORDER BY 2, 1",
            &[&cancelled],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1), false))
        .collect();
    for profile in absent.iter().map(|(_, p, _)| p.clone()).collect::<std::collections::BTreeSet<String>>() {
        let days: Vec<(String, String, bool)> = absent.iter().filter(|(_, p, _)| *p == profile).cloned().collect();
        crate::watch::not_derived(tx, &days, "Cancelling their ordered pass left").await?;
    }
    Ok(cancelled.len() as u64)
}

/// What `omit` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omitted {
    pub omission_id: i64,
    /// The FAILED day job it cancelled.
    pub day_job: i64,
    /// The session's queued ordered passes it cancelled.
    pub passes: Vec<i64>,
    /// The day's cause when it was omitted (`core.job_failure_cause`): its code and detail.
    pub cause: (String, String),
}

/// C-061: omits a date whose day failed for the provider. Allowed only while the session's day job
/// of the profile stands FAILED with nothing of the day published, and never for the engine's fault
/// hold (an engine fault is fixed and recomputed, never omitted). Records the session, profile,
/// reason and time (`core.omitted_session`), cancels the FAILED day job and the session's queued
/// ordered pass, and logs a notice, in one transaction. The date then reads OMITTED, with the
/// reason, in the reader API; later passes never waited for it (`core.claimable_job`, 0031).
pub async fn omit(client: &mut Client, profile: &str, session: &str, reason: &str) -> Result<Omitted, String> {
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
    let reason = scrubbed(reason.trim());
    if reason.is_empty() {
        return Err("an omitted date needs a reason".into());
    }
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    if tx.query_opt("SELECT FROM core.session WHERE session = $1::text::date", &[&session]).await.map_err(db)?.is_none() {
        return Err(format!("{session} is not a trading session"));
    }
    let jobs: Vec<(i64, String)> = tx
        .query(
            "SELECT job_id, state FROM core.job WHERE kind = 'DAY_BLOCK' AND profile_id = $1 AND $2::text::date BETWEEN first_session AND last_session
               AND state <> 'CANCELLED' ORDER BY job_id DESC FOR UPDATE",
            &[&profile, &session],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let only = "only a day whose job stands FAILED may be omitted";
    if let Some((job, state)) = jobs.iter().find(|(_, s)| s == "QUEUED" || s == "LEASED") {
        return Err(format!("{session}'s day job {job} is {state}: {only}"));
    }
    let Some(day_job) = jobs.iter().find(|(_, s)| s == "FAILED").map(|(j, _)| *j) else {
        return Err(match jobs.first() {
            Some((job, state)) => format!("{session}'s day job {job} is {state}: {only}"),
            None => format!("no day job of {profile} covers {session}: {only}"),
        });
    };
    if tx
        .query_opt(
            "SELECT FROM vyreon.day_revision WHERE represented_date = $1::text::date AND profile_id = $2 AND state = 'PUBLISHED' AND base_revision IS NULL",
            &[&session, &profile],
        )
        .await
        .map_err(db)?
        .is_some()
    {
        return Err(format!("{session} of {profile} has a published day: nothing to omit"));
    }
    let row = tx.query_one("SELECT c->>'code', c->>'detail' FROM core.job_failure_cause($1) c", &[&day_job]).await.map_err(db)?;
    let cause: (String, String) = (row.get(0), row.get(1));
    if cause.0 == "ENGINE_FAULT" {
        return Err(format!("{session} was held for an engine fault ({}): an engine fault is fixed and recomputed, never omitted", cause.1));
    }
    let omission_id: i64 = tx
        .query_one(
            "INSERT INTO core.omitted_session (session, profile_id, reason, job_id) VALUES ($1::text::date, $2, $3, $4) RETURNING omission_id",
            &[&session, &profile, &reason, &day_job],
        )
        .await
        .map_err(db)?
        .get(0);
    let passes: Vec<i64> = tx
        .query(
            "SELECT job_id FROM core.job WHERE kind = 'ORDERED_PASS' AND profile_id = $1 AND first_session = $2::text::date AND state = 'QUEUED' ORDER BY job_id",
            &[&profile, &session],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut cancelled = passes.clone();
    cancelled.push(day_job);
    cancel_in(&tx, &cancelled).await?;
    let pass = match passes.len() {
        0 => "it had no queued ordered pass".to_owned(),
        1 => "its ordered pass is cancelled".to_owned(),
        n => format!("its {n} ordered passes are cancelled"),
    };
    notice::log(&tx, &format!("Day {session} of {profile} omitted (job {day_job}, {}: {}): {reason}. {pass}; later passes read it as missing.", cause.0, cause.1))
        .await?;
    tx.commit().await.map_err(db)?;
    Ok(Omitted { omission_id, day_job, passes, cause })
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
