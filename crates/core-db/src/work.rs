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
    /// C-073: a published day's reference check run again, against the revisions core holds
    /// published; queued by the watch (`refcheck::queue_reruns`), never scheduled by hand.
    Refcheck,
    /// C-074: one session's scoped recompute (engine D-462), scheduled by `republish::schedule`.
    Republish,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Tape => "TAPE",
            Kind::DayBlock => "DAY_BLOCK",
            Kind::OrderedPass => "ORDERED_PASS",
            Kind::Refcheck => "REFCHECK",
            Kind::Republish => "REPUBLISH",
        }
    }

    pub fn parse(text: &str) -> Result<Kind, String> {
        match text.to_ascii_uppercase().replace('-', "_").as_str() {
            "TAPE" => Ok(Kind::Tape),
            "DAY_BLOCK" => Ok(Kind::DayBlock),
            "ORDERED_PASS" => Ok(Kind::OrderedPass),
            "REFCHECK" => Ok(Kind::Refcheck),
            "REPUBLISH" => Ok(Kind::Republish),
            other => Err(format!("unknown job kind {other} (tape, day-block, ordered-pass, refcheck, republish)")),
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
    schedule_scoped(client, kind, profile, build, first, last, block, None).await
}

/// `schedule`, each new job carrying `scope` (C-079: a recompute's day blocks and passes name their
/// plan and, for a day block, the measurements it recomputes, so a claim past the prints' retention
/// can turn its days into scoped republishes, `guard_perished_blocks`).
#[allow(clippy::too_many_arguments)]
pub async fn schedule_scoped(client: &mut Client, kind: Kind, profile: &str, build: &str, first: &str, last: &str, block: i32, scope: Option<&Value>) -> Result<Scheduled, String> {
    let scope: Option<String> = scope.map(Value::to_string);
    if kind == Kind::Refcheck {
        return Err("reference-check re-runs are queued by core's watch (C-073), not scheduled".into());
    }
    if kind == Kind::Republish {
        return Err("a republish is scheduled with its scope and reason: `vyreon-core republish` (C-074)".into());
    }
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
    if kind == Kind::OrderedPass {
        // a publish queues ordered passes too (`publish::rederive`, under the publish lock): one at
        // a time, so neither reads "no unfinished pass" while the other inserts one (audit
        // 2026-11-22 changes-core 1); the two locks in the order `omit` and `retire` take them
        tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    }
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    if kind == Kind::OrderedPass {
        let (kept, replaced) = passes_in_range(&tx, profile, build, first, last).await?;
        let jobs = tx
            .execute(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, priority, scope)
                 SELECT 'ORDERED_PASS', $1, $2, s.session, s.session, s.session,
                        COALESCE((SELECT max(r.priority) FROM core.job r WHERE r.job_id = ANY($5) AND r.first_session = s.session), 0), $6::text::jsonb
                 FROM core.session s
                 WHERE s.session BETWEEN $3::text::date AND $4::text::date
                   AND NOT EXISTS (SELECT FROM core.job u WHERE u.kind = 'ORDERED_PASS' AND u.profile_id = $1 AND u.first_session = s.session
                                     AND u.state IN ('QUEUED', 'LEASED', 'FAILED'))
                 ORDER BY s.session",
                &[&profile, &build, &first, &last, &replaced, &scope],
            )
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        return Ok(Scheduled { jobs, sessions, partitions, kept, replaced });
    }
    // C-074 (release-2 audit, Codex 5, Gemini 4): a day block and a republish of a day never run at once,
    // whichever was scheduled first (`republish::schedule` checks the other way)
    let kinds: Vec<&str> = if kind == Kind::DayBlock { vec!["DAY_BLOCK", "REPUBLISH"] } else { vec![kind.name()] };
    let overlap: Option<(i64, String)> = tx
        .query_opt(
            "SELECT job_id, kind FROM core.job
             WHERE kind = ANY($1) AND profile_id = $2 AND state IN ('QUEUED', 'LEASED', 'FAILED')
               AND first_session <= $4::text::date AND last_session >= $3::text::date LIMIT 1",
            &[&kinds, &profile, &first, &last],
        )
        .await
        .map_err(db)?
        .map(|r| (r.get(0), r.get(1)));
    if let Some((job, other)) = overlap {
        return Err(format!("{first} to {last} overlaps unfinished {other} job {job}"));
    }
    let jobs = tx
        .execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope)
             SELECT $1, $2, $3, min(session), max(session), min(session), $7::text::jsonb
             FROM (SELECT session, (row_number() OVER (ORDER BY session) - 1) / $6 AS block
                   FROM core.session WHERE session BETWEEN $4::text::date AND $5::text::date) s
             GROUP BY block ORDER BY min(session)",
            &[&kind.name(), &profile, &build, &first, &last, &i64::from(block), &scope],
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
    // C-077: the drains past their bound stop, and every drain whose lease ended is settled
    if let Err(e) = settle_drains(client).await {
        errors.push(e);
    }
    if errors.is_empty() { Ok(ended) } else { Err(errors.join("; ")) }
}

/// C-088 (2026-11-26: core was down 13:28 to 13:48 MT with InvalidImageName; the daily block's worker, job
/// 30062, downloaded on with its heartbeats failing, and the restarted core ended its lease for missed
/// heartbeats, about 3 h of work lost): core's own downtime costs no lease. At the service's start, after it
/// holds the single-instance lock and before its sweeper or its API runs, every live lease gets at least
/// `lease_seconds` from now, the time a heartbeat has while core is up. A worker that died meanwhile is
/// found one lease period later than it would have been. Returns the leases whose expiry had passed (each
/// would have been ended by the first sweep), with one log line naming them.
pub async fn grace_on_start(client: &mut Client, lease_seconds: f64) -> Result<Vec<i64>, String> {
    let tx = client.transaction().await.map_err(db)?;
    let rows = tx
        .query(
            "WITH g AS (SELECT lease_id, expires_at < clock_timestamp() AS lapsed FROM core.lease
                        WHERE ended_at IS NULL AND expires_at < clock_timestamp() + make_interval(secs => $1) FOR UPDATE)
             UPDATE core.lease l SET expires_at = clock_timestamp() + make_interval(secs => $1)
             FROM g WHERE l.lease_id = g.lease_id
             RETURNING l.lease_id, g.lapsed",
            &[&lease_seconds],
        )
        .await
        .map_err(db)?;
    let mut lapsed: Vec<i64> = rows.iter().filter(|r| r.get::<_, bool>(1)).map(|r| r.get(0)).collect();
    lapsed.sort_unstable();
    if !lapsed.is_empty() {
        let ids: Vec<String> = lapsed.iter().map(i64::to_string).collect();
        notice::log(
            &tx,
            &format!("Core started with {} live leases past their expiry (core was not answering their heartbeats): each given {lease_seconds:.0} s to renew, leases {}.", lapsed.len(), ids.join(" ")),
        )
        .await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(lapsed)
}

/// C-079 (release-3 audit, Codex 1 and 15; coordinator: "print retention rechecked at claim for every job
/// kind, FULL included; old days scoped regardless"): before a day block of `build` is claimed, each queued
/// one whose remaining sessions include a published day past the prints' retention (`core.prints_perished`,
/// as a republish's claim reads it) is cut there: the block is cancelled, its done days kept as a DONE
/// job, its other days queued again as blocks of their own; each such day becomes a scoped republish of
/// the measurements the block recomputes (a C-079 block's scope names them) unless that scope reads the
/// prints itself (no faithful recompute), and a day no scope names is left as published. Either way it is
/// said on the alerts channel when the owner must act. A first computation (an unpublished day) is never
/// touched: nothing of it can be withheld that was published.
pub async fn guard_perished_blocks(client: &mut Client, build: &str) -> Result<(), String> {
    // a database before 0063 (the tests' older schemas) has no prints to lose by this rule
    let known: bool = client.query_one("SELECT to_regprocedure('core.prints_perished_at(date,timestamp with time zone,integer)') IS NOT NULL", &[]).await.map_err(db)?.get(0);
    if !known {
        return Ok(());
    }
    let candidates: Vec<i64> = client
        .query(
            "SELECT DISTINCT j.job_id FROM core.job j JOIN core.session s ON s.session BETWEEN j.next_session AND j.last_session
             WHERE j.kind = 'DAY_BLOCK' AND j.state = 'QUEUED' AND j.engine_build = $1 AND NOT COALESCE(j.scope @> '{\"compare_only\": true}', false)
               AND core.prints_perished_at(s.session, now()) AND core.day_published(s.session, j.profile_id)",
            &[&build],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for job in candidates {
        let tx = client.transaction().await.map_err(db)?;
        tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
        let Some(row) = tx
            .query_opt(
                "SELECT j.profile_id, j.scope::text,
                        ARRAY(SELECT s.session::text FROM core.session s WHERE s.session BETWEEN j.next_session AND j.last_session ORDER BY 1),
                        ARRAY(SELECT s.session::text FROM core.session s WHERE s.session BETWEEN j.next_session AND j.last_session
                                AND core.prints_perished_at(s.session, now()) AND core.day_published(s.session, j.profile_id) ORDER BY 1)
                 FROM core.job j WHERE j.job_id = $1 AND j.state = 'QUEUED' FOR UPDATE SKIP LOCKED",
                &[&job],
            )
            .await
            .map_err(db)?
        else {
            continue;
        };
        let (profile, scope, all, perished): (String, Option<String>, Vec<String>, Vec<String>) = (row.get(0), row.get(1), row.get(2), row.get(3));
        if perished.is_empty() {
            continue;
        }
        // the days it did: a finished job of their own, as a hand-over keeps them
        tx.execute(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, state, attempts, max_attempts, finished_at, priority)
             SELECT kind, profile_id, engine_build, first_session,
                    (SELECT max(session) FROM core.session WHERE session < j.next_session), NULL, scope, 'DONE', attempts, max_attempts, clock_timestamp(), priority
             FROM core.job j WHERE job_id = $1 AND next_session > first_session",
            &[&job],
        )
        .await
        .map_err(db)?;
        tx.execute("UPDATE core.job SET first_session = next_session, state = 'CANCELLED', finished_at = clock_timestamp() WHERE job_id = $1", &[&job])
            .await
            .map_err(db)?;
        // the other days, in runs of consecutive sessions, queued again as they were
        let mut run: Vec<&String> = Vec::new();
        let mut runs: Vec<(String, String)> = Vec::new();
        for d in &all {
            if perished.contains(d) {
                if let (Some(a), Some(b)) = (run.first(), run.last()) {
                    runs.push(((*a).clone(), (*b).clone()));
                }
                run.clear();
            } else {
                run.push(d);
            }
        }
        if let (Some(a), Some(b)) = (run.first(), run.last()) {
            runs.push(((*a).clone(), (*b).clone()));
        }
        for (a, b) in &runs {
            tx.execute(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, attempts, max_attempts, priority)
                 SELECT kind, profile_id, engine_build, $2::text::date, $3::text::date, $2::text::date, scope, attempts, max_attempts, priority FROM core.job WHERE job_id = $1",
                &[&job, a, b],
            )
            .await
            .map_err(db)?;
        }
        let scope: Value = scope.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let names: Vec<String> = scope["measurements"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_owned)).collect();
        notice::log(
            &tx,
            &format!(
                "Day block job {job} of build {build} cut at its claim: {} published, past the prints' retention ({}); a full recompute would withhold the prints' sensors there. Its other days queued again.",
                perished.len(),
                perished.join(" ")
            ),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        // each such day: scoped, when the block says what it recomputes and that scope reads no prints
        let reason = scope["reason"].as_str().unwrap_or("a full recompute past the prints' retention").to_owned();
        let scoped = if names.is_empty() {
            Err("the block names no measurements it recomputes".to_owned())
        } else {
            match crate::republish::plans(&*client, build).await.and_then(|plans| crate::republish::compose(&plans, &[], &names)) {
                Ok(w) if w.categories.iter().any(|c| c == "exq01") => Err(format!("its scope [{}] reads the prints itself: no faithful recompute", names.join(","))),
                Ok(w) => crate::republish::schedule_tagged(client, &profile, build, &w, &perished, &reason, scope.get("recompute_plan")).await.map(|_| ()),
                Err(e) => Err(e),
            }
        };
        if let Err(why) = scoped {
            notice::alert(
                &*client,
                &format!(
                    "🔴 Needs you: a full recompute of {} was not run: the provider no longer serves those days' option prints, so it would publish their sensors withheld; {why}. Published numbers stay as they are. Decide whether to recompute them scoped, or leave them.",
                    perished.join(" ")
                ),
            )
            .await?;
        }
    }
    Ok(())
}

/// C-077: how long a release lets a running day block of the old build finish the day it is computing
/// (`build::hand_over`). The measured worst first day of a block on the cluster, 4 h 55 min (2024-07-16
/// on 5f637983, MILESTONES: about 3 h of download and 2 h of compute; a later day is shorter), its
/// archive (15 min) and a typical reference check, rounded up to 6 h. Past it the lease is stopped as a
/// withdrawal stops one, and the day is carried to the successor.
pub const DRAIN_MINUTES: f64 = 360.0;

/// C-077: stops the draining leases past their bound and settles every drain whose lease ended: its day
/// done (nothing to do), or not (its job, queued again or failed under the withdrawn build, is carried
/// to the build the hand-overs from it lead to, as a hand-over carries a job). Each in its own
/// transaction; an error names the leases it could not settle (the others are done).
pub async fn settle_drains(client: &mut Client) -> Result<u64, String> {
    let over: Vec<i64> = client
        .query(
            "SELECT d.lease_id FROM core.drain d JOIN core.lease l USING (lease_id) WHERE l.ended_at IS NULL AND d.until < now() ORDER BY 1",
            &[],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut errors = Vec::new();
    for lease in over {
        if let Err(e) = stop_drain(client, lease).await {
            errors.push(format!("drain of lease {lease}: {e}"));
        }
    }
    let ended: Vec<i64> = client
        .query(
            "SELECT d.lease_id FROM core.drain d JOIN core.lease l USING (lease_id) WHERE d.settled_at IS NULL AND l.ended_at IS NOT NULL ORDER BY 1",
            &[],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut settled = 0;
    for lease in ended {
        match settle_drain(client, lease).await {
            Ok(true) => settled += 1,
            Ok(false) => {}
            Err(e) => errors.push(format!("drain of lease {lease}: {e}")),
        }
    }
    if errors.is_empty() { Ok(settled) } else { Err(errors.join("; ")) }
}

/// A draining lease past its bound: ended as a withdrawal ends one (its attempt not counted), its job
/// DONE when its day was done, else queued again under its build, for `settle_drain` to carry.
async fn stop_drain(client: &mut Client, lease: i64) -> Result<(), String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    let Some(row) = tx
        .query_opt(
            "UPDATE core.lease l SET ended_at = clock_timestamp(), outcome = 'CANCELLED', failure = 'its drain''s bound passed (C-077)'
             FROM core.drain d WHERE d.lease_id = l.lease_id AND l.lease_id = $1 AND l.ended_at IS NULL AND d.until < clock_timestamp()
             RETURNING l.job_id, d.session::text",
            &[&lease],
        )
        .await
        .map_err(db)?
    else {
        return Ok(());
    };
    let (job, day): (i64, String) = (row.get(0), row.get(1));
    let done = tx
        .query_one(
            "UPDATE core.job SET state = CASE WHEN next_session IS NULL THEN 'DONE' ELSE 'QUEUED' END,
                                 attempts = GREATEST(attempts - 1, 0),
                                 finished_at = CASE WHEN next_session IS NULL THEN clock_timestamp() END
             WHERE job_id = $1 AND state = 'LEASED' RETURNING state = 'DONE'",
            &[&job],
        )
        .await
        .map_err(db)?
        .get::<_, bool>(0);
    let what = if done { format!("{day} was done; its reference check, if it was still running, is re-run by the watch") } else { format!("{day} goes to the successor") };
    notice::log(&tx, &format!("Draining day block job {job} stopped at its bound ({} h after its hand-over): {what}.", DRAIN_MINUTES / 60.0)).await?;
    tx.commit().await.map_err(db)
}

/// A drain whose lease ended: settled once. False when there was nothing to settle (yet).
async fn settle_drain(client: &mut Client, lease: i64) -> Result<bool, String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    let Some(row) = tx
        .query_opt(
            "SELECT d.job_id, d.session::text, j.state, j.next_session::text, j.engine_build, h.to_build, l.outcome, COALESCE(l.failure, '')
             FROM core.drain d JOIN core.job j USING (job_id) JOIN core.hand_over h USING (hand_over_id) JOIN core.lease l USING (lease_id)
             WHERE d.lease_id = $1 AND d.settled_at IS NULL AND l.ended_at IS NOT NULL FOR UPDATE OF d, j",
            &[&lease],
        )
        .await
        .map_err(db)?
    else {
        return Ok(false);
    };
    let (job, day, state, next, from, to, outcome, failure): (i64, String, String, Option<String>, String, String, String, String) =
        (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4), row.get(5), row.get(6), row.get(7));
    let carried = match (state.as_str(), next) {
        ("QUEUED" | "FAILED", Some(next)) => {
            // where the hand-overs from the old build lead (a later release may have handed its
            // successor's work on): the newest successor of each, from the drain's own
            let mut successor = to.clone();
            let mut seen = vec![from.clone(), to.clone()];
            while let Some(next) = tx
                .query_opt("SELECT to_build FROM core.hand_over WHERE from_build = $1 ORDER BY handed_at DESC, hand_over_id DESC LIMIT 1", &[&successor])
                .await
                .map_err(db)?
                .map(|r| r.get::<_, String>(0))
                .filter(|n| !seen.contains(n))
            {
                seen.push(next.clone());
                successor = next;
            }
            // (release-3 audit, Codex 16, Gemini 16) where that build no longer runs (withdrawn with nothing to
            // hand over, or a rollback), the newest running build that reads the day takes it; a successor
            // not admitted yet (its hand-over's admission to come) keeps it
            let (runs, never_admitted): (bool, bool) = {
                let r = tx
                    .query_one("SELECT core.build_runs($1), admitted_at IS NULL FROM core.engine_build WHERE engine_build = $1", &[&successor])
                    .await
                    .map_err(db)?;
                (r.get(0), r.get(1))
            };
            if !runs && !never_admitted {
                match tx
                    .query_opt(
                        "SELECT b.engine_build FROM core.engine_build b WHERE core.build_runs(b.engine_build) AND core.reads_days_of(b.engine_build, $1)
                         ORDER BY b.admitted_at DESC, b.engine_build LIMIT 1",
                        &[&from],
                    )
                    .await
                    .map_err(db)?
                {
                    Some(r) => successor = r.get(0),
                    // no build to take it yet: tried again at the next sweep
                    None => return Ok(false),
                }
            }
            // a base published without its day-local revision is superseded, as a day set aside's is
            // (release-3 audit, Gemini 3: C-063, migration 0039), so no reader sees half a day
            let profile: String = tx.query_one("SELECT profile_id FROM core.job WHERE job_id = $1", &[&job]).await.map_err(db)?.get(0);
            let dropped = dropped_base_clause(crate::publish::drop_base_without_day_local(&tx, job, &next, &profile).await?, &next);
            // the days it did: a finished job of their own, as a hand-over keeps them
            tx.execute(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, state, attempts, max_attempts, finished_at, priority)
                 SELECT kind, profile_id, engine_build, first_session,
                        (SELECT max(session) FROM core.session WHERE session < j.next_session), NULL, scope, 'DONE', attempts, max_attempts, clock_timestamp(), priority
                 FROM core.job j WHERE job_id = $1 AND next_session > first_session",
                &[&job],
            )
            .await
            .map_err(db)?;
            tx.execute("UPDATE core.job SET first_session = next_session, state = 'CANCELLED', finished_at = clock_timestamp() WHERE job_id = $1", &[&job])
                .await
                .map_err(db)?;
            let new: i64 = tx
                .query_one(
                    "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, max_attempts, priority)
                     SELECT kind, profile_id, $2, next_session, last_session, next_session, scope, max_attempts, priority FROM core.job WHERE job_id = $1
                     RETURNING job_id",
                    &[&job, &successor],
                )
                .await
                .map_err(db)?
                .get(0);
            let why = if failure.is_empty() { outcome.to_lowercase() } else { crate::watch::one_line(&failure, 160) };
            notice::log(
                &tx,
                &format!("Draining day block job {job} of build {from} ended without {next} ({why}): the day is build {successor}'s, job {new}{dropped}."),
            )
            .await?;
            Some(new)
        }
        ("DONE", _) => {
            notice::log(&tx, &format!("Draining day block job {job} of build {from} finished {day} and stopped (C-077).")).await?;
            None
        }
        // cancelled by hand, or (impossible: its build is withdrawn) leased again: nothing to carry
        ("CANCELLED", _) => None,
        _ => return Ok(false),
    };
    tx.execute("UPDATE core.drain SET settled_at = clock_timestamp(), carried_job = $2 WHERE lease_id = $1", &[&lease, &carried]).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}

/// C-077 (live, 2026-11-25 03:35 MT: job 21823 of 1ae946f7, an engine before D-465, sent 2024-09-06 after its
/// drained 2024-09-05 and the refusal paused the whole queue): a bundle refused from a draining lease whose
/// job has no day left is the drain's overrun (a worker that does not know its block was cut goes on to its
/// claim's next day), not a bundle to look at: its lease ends (the job DONE, its later days the successor's
/// already), one line says so, and nothing pauses. True when `lease` was such a lease; the caller answers
/// the worker that its lease is gone. Every other refusal pauses as before.
pub async fn drain_overrun(tx: &impl GenericClient, lease: &Held, refusal: &str) -> Result<bool, String> {
    if lease.kind != "DAY_BLOCK" {
        return Ok(false);
    }
    let Some(row) = tx
        .query_opt(
            "SELECT d.session::text, j.engine_build, h.to_build FROM core.drain d JOIN core.lease l USING (lease_id) JOIN core.job j ON j.job_id = d.job_id
             JOIN core.hand_over h USING (hand_over_id)
             WHERE d.lease_id = $1 AND l.ended_at IS NULL AND j.next_session IS NULL FOR UPDATE OF l, j",
            &[&lease.lease_id],
        )
        .await
        .map_err(db)?
    else {
        return Ok(false);
    };
    let (day, from, to): (String, String, String) = (row.get(0), row.get(1), row.get(2));
    tx.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE lease_id = $1 AND ended_at IS NULL", &[&lease.lease_id]).await.map_err(db)?;
    if let Some(job) = tx
        .query_opt(&format!("UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1 AND state = 'LEASED' RETURNING {FINISHED}"), &[&lease.job_id])
        .await
        .map_err(db)?
    {
        finished(tx, &job, &lease.worker).await?;
    }
    let sent = refusal
        .split_once("the bundle is ")
        .and_then(|(_, r)| r.get(..10))
        .filter(|d| core_contract::is_date(d))
        .map_or_else(|| "a later day".to_owned(), str::to_owned);
    notice::log(
        tx,
        &format!(
            "Draining job {} of build {from} sent {sent} after its drained day {day} (a worker that does not stop at core's last day): refused, its lease ended, nothing paused; later days are build {to}'s.",
            lease.job_id
        ),
    )
    .await?;
    Ok(true)
}

/// C-077: ends a draining lease once its day is done and that day's reference check is with core (the
/// last thing a worker sends of a day, after its archive): the job DONE, the lease's outcome DONE. A
/// worker of an engine before C-077 does not know its block was cut and goes on to the next day; its
/// next call is answered that the lease is gone. False when `lease` is not such a lease.
pub(crate) async fn end_drained(tx: &impl GenericClient, lease: &Held, session: &str) -> Result<bool, String> {
    if lease.kind != "DAY_BLOCK" || lease.next_session.is_some() {
        return Ok(false);
    }
    let draining = tx.query_opt("SELECT FROM core.drain WHERE lease_id = $1 AND session = $2::text::date", &[&lease.lease_id, &session]).await.map_err(db)?.is_some();
    if !draining {
        return Ok(false);
    }
    tx.execute("UPDATE core.lease SET ended_at = now(), outcome = 'DONE' WHERE lease_id = $1 AND ended_at IS NULL", &[&lease.lease_id]).await.map_err(db)?;
    if let Some(job) = tx
        .query_opt(&format!("UPDATE core.job SET state = 'DONE', finished_at = now() WHERE job_id = $1 AND state = 'LEASED' RETURNING {FINISHED}"), &[&lease.job_id])
        .await
        .map_err(db)?
    {
        finished(tx, &job, &lease.worker).await?;
    }
    Ok(true)
}

/// Ends one expired lease and moves its job, in one transaction. False when it was renewed or
/// ended meanwhile.
async fn expire(client: &mut Client, lease: i64) -> Result<bool, String> {
    let tx = client.transaction().await.map_err(db)?;
    // a day set aside may take down its base and what read it (`requeue`): the publish lock first,
    // before any row lock, as a publish takes it
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    let Some(row) = tx
        .query_opt(
            "UPDATE core.lease l SET ended_at = now(), outcome = 'EXPIRED', failure = 'the lease was not renewed',
                                     stopped_on = (SELECT j.next_session FROM core.job j WHERE j.job_id = l.job_id)
             WHERE lease_id = $1 AND ended_at IS NULL AND expires_at < now() RETURNING job_id, stopped_on::text",
            &[&lease],
        )
        .await
        .map_err(db)?
    else {
        return Ok(false);
    };
    let (job, day): (i64, Option<String>) = (row.get(0), row.get(1));
    if let Some(day) = day {
        lost(&tx, job, lease, &day).await?;
    }
    requeue(&tx, job, "its lease expired").await?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}

/// How many leases a session of a job may lose (expired: its node lost, its pod killed or out of
/// memory, its Job's deadline) before a lost lease is charged as an attempt (C-035 amended, C-068).
pub const LOST_LEASES_FREE: i64 = 2;

/// Audit 2026-11-22 opsurface (ops.md) 13, C-035 amended: a lease that expired without leaving its
/// evidence (no stop the worker could explain: the node went, the pod was killed, the Job's deadline
/// stopped it) is not charged as an attempt while the session it stopped on (`day`, the job's next
/// session then) has lost at most `LOST_LEASES_FREE` leases: the attempt is given back before the
/// job is moved (`requeue`), so a day is never set aside on its first lost node. A session that keeps
/// killing its node is charged from its third lost lease on: it is set aside (or the pass, or the tape,
/// stands FAILED) after at most `LOST_LEASES_FREE` + `max_attempts` lost leases. A stop with evidence
/// is charged as before.
async fn lost(tx: &impl GenericClient, job: i64, lease: i64, day: &str) -> Result<(), String> {
    let row = tx
        .query_one(
            "SELECT EXISTS (SELECT FROM core.job_evidence WHERE lease_id = $2),
                    (SELECT count(*) FROM core.lease WHERE job_id = $1 AND outcome = 'EXPIRED' AND stopped_on = $3::text::date)",
            &[&job, &lease, &day],
        )
        .await
        .map_err(db)?;
    let (explained, lost): (bool, i64) = (row.get(0), row.get(1));
    if explained || lost > LOST_LEASES_FREE {
        return Ok(());
    }
    let n = tx.execute("UPDATE core.job SET attempts = GREATEST(attempts - 1, 0) WHERE job_id = $1 AND state = 'LEASED'", &[&job]).await.map_err(db)?;
    if n == 1 {
        notice::log(tx, &format!("Job {job} lost its lease on {day} without a stop ({lost} of {LOST_LEASES_FREE} such losses free on that day): the attempt is not counted.")).await?;
    }
    Ok(())
}

/// From this many day jobs standing FAILED the log says the count with each further one. It was the count
/// that paused the whole queue, the nightly included (C-035); since C-094 no count of failed days pauses it.
pub const FAILED_DAYS_NOTED: i64 = 3;

/// A job whose attempt ended without finishing: DONE, queued again, or FAILED. A job whose every
/// session is done is DONE, whatever became of its worker's `finish` (lost, never sent, or sent
/// after a failure): there is nothing left to claim. A day block out of attempts keeps only the
/// day it stopped on: the days before it are done, the days after it become a new queued block,
/// and no count of failed days pauses the queue (C-094). An ordered pass out of attempts
/// stays FAILED without a pause; a tape job pauses the queue at once (the day blocks wait on it).
/// Returns true when the job was queued again.
async fn requeue(db_client: &impl GenericClient, job: i64, why: &str) -> Result<bool, String> {
    // C-074: a republish's base waits STAGED for its day-local revision; an attempt that ends without
    // it leaves the day as it was, and its staged revisions are rejected
    reject_staged(db_client, &[job], why).await?;
    // C-074 (release-2 audit, Codex 4, Gemini 2): a republish whose day-local revision is published (its
    // worker ended before saying the day done) has replaced its day: done, never leased again for it
    db_client
        .execute(
            "UPDATE core.job j SET next_session = NULL WHERE j.job_id = $1 AND j.kind = 'REPUBLISH' AND j.state = 'LEASED'
               AND EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = j.first_session AND d.day_local AND d.state = 'PUBLISHED'
                             AND d.republish_scope->>'job' = j.job_id::text)",
            &[&job],
        )
        .await
        .map_err(db)?;
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
    // audit 2026-11-22 changes-core 10: a day block whose attempt stopped on a day it had already
    // said was done (a prune after `day_done`, say) never tried the day it moved on to. That attempt is
    // not counted against the block: queued again, never FAILED on a day it did not try, nor that day
    // given the earlier day's cause. Round 2 (core-r2 4, 10): core establishes it itself. The day is
    // the one the stop message names (C-049's evidence, as the engine writes it: `stop: <date> ...`, or
    // `stop: prune after <date>: ...`), never the evidence's session alone, which the worker sets to its
    // claim's first day when the message names none (`vyreon-worker` `stopped_on`); and it must be a
    // day this very lease did (from where it began, `start_session`, which the database sets when the
    // lease is granted, to before the block's next session). Each refund then moved the block on by a day of its own lease: bounded by the block.
    if let Some(day) = db_client
        .query_opt(
            "SELECT s.day FROM core.job j
             JOIN LATERAL (SELECT l.lease_id, l.start_session FROM core.lease l WHERE l.job_id = j.job_id ORDER BY l.lease_id DESC LIMIT 1) l ON true
             JOIN core.job_evidence e ON e.lease_id = l.lease_id
             CROSS JOIN LATERAL (SELECT COALESCE((regexp_match(e.message, '^stop: ([0-9]{4}-[0-9]{2}-[0-9]{2}) '))[1],
                                                 (regexp_match(e.message, '^stop: prune after ([0-9]{4}-[0-9]{2}-[0-9]{2}):'))[1]) AS day) s
             WHERE j.job_id = $1 AND j.state = 'LEASED' AND j.kind = 'DAY_BLOCK'
               AND s.day >= l.start_session::text AND s.day < j.next_session::text",
            &[&job],
        )
        .await
        .map_err(db)?
        .map(|r| r.get::<_, String>(0))
    {
        let next: String = db_client
            .query_one(
                "UPDATE core.job SET state = 'QUEUED', attempts = GREATEST(attempts - 1, 0), finished_at = NULL WHERE job_id = $1 RETURNING next_session::text",
                &[&job],
            )
            .await
            .map_err(db)?
            .get(0);
        notice::log(
            db_client,
            &format!("Day block job {job} stopped on {day}, a day it had done ({why}); {next} was not tried: queued again, the attempt not counted."),
        )
        .await?;
        return Ok(true);
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
    if kind == "REPUBLISH" {
        // C-074: the day keeps the revisions it had (a republish replaces a day whole or not at all);
        // nothing waits on it but the passes after it, which it no longer holds (FAILED); JOB_FAILED says so
        let day: String = db_client.query_one("SELECT first_session::text FROM core.job WHERE job_id = $1", &[&job]).await.map_err(db)?.get(0);
        notice::log(db_client, &format!("Republish of {day} failed for the last time (job {job}): {why}. The day keeps its revisions.")).await?;
        return Ok(false);
    }
    if kind == "REFCHECK" {
        // C-073: a re-run holds nothing back; the day's check stands not run (REFCHECK_NOT_RUN)
        let day: String = db_client.query_one("SELECT first_session::text FROM core.job WHERE job_id = $1", &[&job]).await.map_err(db)?.get(0);
        notice::log(db_client, &format!("Reference-check re-run of {day} failed for the last time (job {job}): {why}. Nothing waits on it.")).await?;
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
    let row = db_client
        .query_one("UPDATE core.job SET first_session = next_session, last_session = next_session WHERE job_id = $1 RETURNING next_session::text, profile_id", &[&job])
        .await
        .map_err(db)?;
    let (day, profile): (String, String) = (row.get(0), row.get(1));
    // (release-3 audit, Gemini 13) a draining block of a withdrawn build is carried to the successor by the
    // sweep: its failed day is no day of the running builds' to pause for
    let failed: i64 = db_client
        .query_one(
            "SELECT count(*) FROM core.job j WHERE j.kind = 'DAY_BLOCK' AND j.state = 'FAILED' AND NOT EXISTS (SELECT FROM core.drain d WHERE d.job_id = j.job_id)",
            &[],
        )
        .await
        .map_err(db)?
        .get(0);
    // audit 2026-11-22 round 2 core-r2 1: a base its day-local revision never joined is superseded, so
    // no later day's history reads the day this block leaves missing
    let local = dropped_base_clause(crate::publish::drop_base_without_day_local(db_client, job, &day, &profile).await?, &day);
    // C-094 (owner, 2026-11-26: "the nightly should never get paused"; the run records every fault and goes on):
    // failed days no longer pause the queue, at any count. Each is set aside and alerted (JOB_FAILED); systematic
    // breakage is the stop rule's to say (C-090), and stopping the backfill is `backfill stop`, which never
    // touches the nightly.
    let after = if rest == 1 { "; the rest of its block is queued again" } else { "" };
    let standing = if failed >= FAILED_DAYS_NOTED { format!(" {failed} days stand failed.") } else { String::new() };
    notice::log(db_client, &format!("Day {day} failed for the last time and is set aside (job {job}): {why}{after}{local}. The build goes on.{standing}")).await?;
    Ok(false)
}

/// C-074: rejects the STAGED revisions the leases of republish `jobs` left (a base waiting for its
/// day-local revision) when their attempt ends without it: a failure, a lost lease, a cancellation, a
/// withdrawal or a retirement (release-2 audit, Codex 15, Gemini 14). The day keeps what it had.
pub(crate) async fn reject_staged(db_client: &impl GenericClient, jobs: &[i64], why: &str) -> Result<u64, String> {
    db_client
        .execute(
            "UPDATE vyreon.day_revision d SET state = 'REJECTED', rejection = 'its republish attempt ended before the day was published: ' || left($2, 500)
             FROM core.lease_revision lr JOIN core.lease l USING (lease_id) JOIN core.job j USING (job_id)
             WHERE d.revision_id = lr.revision_id AND d.state = 'STAGED' AND l.job_id = ANY($1) AND j.kind = 'REPUBLISH'",
            &[&jobs, &why],
        )
        .await
        .map_err(db)
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

/// The clause a notice gives a base `drop_base_without_day_local` superseded; else empty.
fn dropped_base_clause(base: Option<i64>, day: &str) -> String {
    base.map_or_else(String::new, |b| {
        format!(
            "; its base revision {b} was published but no day-local derived revision on it: the base is superseded, so no later day's \
             history reads {day} (a missing date everywhere; `vyreon-core queue retry-failed` computes the day again)"
        )
    })
}

/// C-086: a backfill day block of a build starts at most once in this many minutes (`core.backfill_block_room`,
/// migration 0064, which states the number): the blocks ramp up instead of sharing the request budget from
/// one minute (2026-11-26: seven at once, their first days' downloads twice as long).
pub const RAMP_MINUTES: i64 = 20;

/// Taken by every publish before any row lock, and by the claim of an ordered pass: a pass is then
/// claimed either before a publish (which stops it if it reads that day) or after it (and reads
/// what it published), never in between (`publish`).
pub const PUBLISH_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended('vyreon publish', 0))";

/// How many times one claim looks for a claimable row before it answers that there is none while
/// some no other claim holds is still claimable (C-065): each look after the first is a fresh
/// snapshot.
pub const CLAIM_TRIES: i32 = 3;

/// The longest worker name a claim takes: a Kubernetes pod's host name (an RFC 1123 label is at
/// most 63 characters).
pub const WORKER_NAME_MAX: usize = 63;

/// Whether `worker` is a name a claim takes (audit 2026-11-22 prebuild round 2, core-watch.md, 5):
/// 1 to `WORKER_NAME_MAX` letters, digits, '.', '_' or '-', beginning with a letter or a digit, as
/// a pod's host name is (the engine's worker names itself `VYREON_WORKER_NAME` or its host name).
/// The name is stored with the lease and posted in notices and progress lines, so one that could
/// carry a credential (a URL, `key=value`, a header, anything with a space, ':', '/', '=' or '@')
/// is refused before it is stored. The refusal never repeats the name.
pub fn worker_name(worker: &str) -> Result<(), String> {
    let mut chars = worker.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if first_ok && worker.len() <= WORKER_NAME_MAX && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        Ok(())
    } else {
        Err(format!(
            "a claim names its worker with 1 to {WORKER_NAME_MAX} letters, digits, '.', '_' or '-', beginning with a letter or a digit \
             (a pod's host name); this one is not such a name (not repeated here)"
        ))
    }
}

/// Claims the earliest claimable job of `kind` for the worker's own build, if there is one; a daily
/// run's job (priority 1, C-051) before any other.
pub async fn claim(client: &mut Client, kind: Kind, build: &str, worker: &str, lease_seconds: f64) -> Result<Option<Claim>, String> {
    claim_as(client, kind, build, worker, lease_seconds, false).await
}

/// `claim`; with `daily_only`, only a daily run's job (priority 1): the daily CronJob's worker
/// never takes a backfill block (C-051 amended, audit round x, 13).
pub async fn claim_as(client: &mut Client, kind: Kind, build: &str, worker: &str, lease_seconds: f64, daily_only: bool) -> Result<Option<Claim>, String> {
    worker_name(worker)?;
    // expired leases end first, each in its own transaction; one that cannot be ended is reported
    // by the service's sweeper every half minute and does not stop this claim
    let _ = sweep(client).await;
    // C-074 (release-2 audit, Codex 2), in its own statements (a claim that leases nothing rolls back): a republish that reads EXQ01's prints is claimed only while the
    // provider still serves its day's (`core.prints_perished_at`: C-081, engine D-512, a session counts once it has opened), whenever it was
    // queued, retried or carried: one past them is FAILED with that cause, never run to publish withheld
    if kind == Kind::Republish {
        let t = client.transaction().await.map_err(db)?;
        let gone = t
            .query(
                "UPDATE core.job j SET state = 'FAILED', finished_at = now()
                 WHERE j.kind = 'REPUBLISH' AND j.state = 'QUEUED' AND j.engine_build = $1 AND COALESCE(j.scope->'categories' ? 'exq01', false)
                   AND core.prints_perished_at(j.first_session, now())
                 RETURNING j.job_id, j.first_session::text",
                &[&build],
            )
            .await
            .map_err(db)?;
        for r in &gone {
            let (job, day): (i64, String) = (r.get(0), r.get(1));
            notice::log(&t, &format!("Republish of {day} (job {job}) not run: the scope reads {day}'s option prints, which the provider no longer serves. The day keeps its revisions.")).await?;
        }
        t.commit().await.map_err(db)?;
    }
    // C-079 (release-3 audit, Codex 1 and 15): a day block over a published day whose option prints the
    // provider no longer serves would publish the prints' sensors withheld where the day holds readings;
    // such days are taken out of the block first, whoever queued it and however long it waited
    if kind == Kind::DayBlock {
        guard_perished_blocks(client, build).await?;
    }
    let tx = client.transaction().await.map_err(db)?;
    if kind == Kind::OrderedPass {
        tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    }
    // the queue's control row, shared (audit round 2 of core-watch, 3): a pause (which updates it)
    // waits for this claim to commit, or this claim waits for the pause and reads the queue
    // paused; the claimable view reads `paused` from each statement's snapshot, so without the
    // lock a pause committed during the claim still let it lease, stamped after the pause (C-025)
    let paused: bool = tx.query_one("SELECT paused FROM core.queue_control FOR SHARE", &[]).await.map_err(db)?.get(0);
    if paused {
        return Ok(None);
    }
    // the build's row, shared: a withdrawal (which updates it, then ends the build's leases) waits
    // for this claim, or this claim reads the build withdrawn (`build::withdraw`)
    tx.execute("SELECT FROM core.engine_build WHERE engine_build = $1 FOR SHARE", &[&build]).await.map_err(db)?;
    // C-065 (the claim race of 2026-11-22): the row is locked by the inner SELECT, whose own
    // condition says `j.state = 'QUEUED'`. Under READ COMMITTED a row another claim leased and
    // committed after this statement's snapshot is re-read at its newest version when it is locked
    // (PostgreSQL's EvalPlanQual recheck of the locking scan's own conditions): with the state among
    // them it fails the recheck and the scan goes on to the next claimable row, instead of handing
    // the row to the outer `AND state = 'QUEUED'`, which dropped it and returned nothing. The view's
    // own `state = 'QUEUED'` is read from the snapshot and is not rechecked.
    let mut tries = 0;
    // a claimable job no other claim holds, found by the last look and held KEY SHARE since
    let mut free: Option<i64> = None;
    let job = loop {
        tries += 1;
        let job = tx
            .query_opt(
                "UPDATE core.job SET state = 'LEASED', attempts = attempts + 1
                 WHERE job_id = (SELECT j.job_id FROM core.job j
                                 WHERE j.state = 'QUEUED'
                                   AND j.job_id IN (SELECT job_id FROM core.claimable_job WHERE kind = $1 AND engine_build = $2 AND next_session IS NOT NULL)
                                   AND (NOT $3 OR j.priority = 1)
                                 ORDER BY j.priority DESC, j.first_session, j.job_id LIMIT 1 FOR UPDATE SKIP LOCKED)
                   AND state = 'QUEUED'
                 RETURNING job_id, profile_id, scope::text, attempts, next_session::text, last_session::text",
                &[&kind.name(), &build, &daily_only],
            )
            .await
            .map_err(db)?;
        if job.is_some() {
            break job;
        }
        // nothing leased (audit round 2 of core-watch, 6 and 8): is a claimable job there that no
        // other claim holds? A claim holds its row FOR UPDATE, which conflicts with this look's
        // KEY SHARE, so the look skips exactly the jobs claims in flight are taking (healthy
        // concurrency: nothing for this claim, and no miss), and finds a job queued after the last
        // statement's snapshot or released by a claim that rolled back, which the next statement
        // then takes: the answer "nothing" is true as of the last look. Held KEY SHARE until this
        // claim ends, the job found is skipped by other claims while this one tries it again.
        let look = tx
            .query_one(
                "WITH free AS (SELECT j.job_id FROM core.job j
                               WHERE j.state = 'QUEUED'
                                 AND j.job_id IN (SELECT job_id FROM core.claimable_job WHERE kind = $1 AND engine_build = $2 AND next_session IS NOT NULL)
                                 AND (NOT $3 OR j.priority = 1)
                               ORDER BY j.priority DESC, j.first_session, j.job_id LIMIT 1 FOR KEY SHARE SKIP LOCKED)
                 SELECT (SELECT job_id FROM free),
                        (SELECT count(*) FROM core.claimable_job WHERE kind = $1 AND engine_build = $2 AND next_session IS NOT NULL AND (NOT $3 OR priority = 1))",
                &[&kind.name(), &build, &daily_only],
            )
            .await
            .map_err(db)?;
        let (found, claimable): (Option<i64>, i64) = (look.get(0), look.get(1));
        let Some(found) = found else {
            break None;
        };
        // a job no other claim held was there before this try too, and still none was leased after
        // `CLAIM_TRIES` tries: a miss, kept for the watch (CLAIMS_EMPTY) with the job it could not take
        if free.is_some() && tries >= CLAIM_TRIES {
            tx.execute(
                "INSERT INTO core.claim_miss (kind, engine_build, worker, daily, claimable, job_id) VALUES ($1, $2, $3, $4, $5, $6)",
                &[&kind.name(), &build, &worker, &daily_only, &claimable, &found],
            )
            .await
            .map_err(db)?;
            tx.commit().await.map_err(db)?;
            return Ok(None);
        }
        free = Some(found);
    };
    let Some(job) = job else {
        return Ok(None);
    };
    let (job_id, profile_id, scope, attempt, next, last): (i64, String, Option<String>, i32, Option<String>, String) =
        (job.get(0), job.get(1), job.get(2), job.get(3), job.get(4), job.get(5));
    let next = next.ok_or_else(|| format!("job {job_id} has no session left to do"))?;
    // C-064 amended (audit round 2 of core-watch, 1): granted and expiring on one clock, read when
    // the lease is written, so a claim that waited (an ordered pass for the publish lock) never
    // hands out a lease that expired before it was granted (`now()` is the transaction's start)
    let lease = tx
        .query_one(
            "WITH t AS (SELECT replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '') AS token)
             INSERT INTO core.lease (job_id, worker, token_sha256, granted_at, expires_at)
             SELECT $1, $2, encode(sha256(convert_to(token, 'UTF8')), 'hex'), g.at, g.at + make_interval(secs => $3) FROM t, (SELECT clock_timestamp() AS at) g
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
/// the day it is on, then should stop). The new expiry is counted from when it is written, after
/// any wait for the lease's lock (a publish under way holds it), as a claim's is.
pub async fn heartbeat(client: &mut Client, token: &str, lease_seconds: f64) -> Result<(String, bool), LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    let row = tx
        .query_one(
            "UPDATE core.lease SET expires_at = clock_timestamp() + make_interval(secs => $2) WHERE lease_id = $1
             RETURNING to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), (SELECT paused FROM core.queue_control)",
            &[&lease.lease_id, &lease_seconds],
        )
        .await
        .map_err(failed)?;
    tx.commit().await.map_err(failed)?;
    Ok((row.get(0), row.get(1)))
}

/// C-073: the longest a declared wait may reach ahead of now.
pub const WAIT_MAX_HOURS: f64 = 24.0;

/// C-073: a worker's report of the step it is on and of any deliberate wait (`waiting_until`, an
/// instant, with its `reason`; both or neither), kept per lease (`core.lease_step`). A step or a wait
/// that differs from the last report starts its clock (`step_since`): the worker's own progress mark
/// for the watch's NO_PROGRESS; the same report again only says the worker is alive. Refused: an empty
/// or long text, one a credential pattern matches (never stored), a wait that is not an RFC 3339
/// instant or is more than `WAIT_MAX_HOURS` ahead (one already over is kept).
pub async fn report_step(client: &mut Client, token: &str, step: &str, waiting_until: Option<&str>, reason: Option<&str>) -> Result<(), LeaseError> {
    report_step_with(client, token, step, waiting_until, reason, None).await
}

/// `report_step` with the step's `progress` (C-073 hotfix: "chains 41 of 64", "312 MB of 1.9 GB"; 1 to
/// 200 characters): a report whose step, wait, reason or progress differs from the last is progress for
/// NO_PROGRESS (`step_since`), so a long step that reports how far it is never alerts; the same report
/// again only says the worker is alive.
pub async fn report_step_with(client: &mut Client, token: &str, step: &str, waiting_until: Option<&str>, reason: Option<&str>, progress: Option<&str>) -> Result<(), LeaseError> {
    let (step, reason) = (step.trim(), reason.map(str::trim).filter(|r| !r.is_empty()));
    let progress = progress.map(str::trim).filter(|p| !p.is_empty());
    if progress.is_some_and(|p| p.chars().count() > 200) {
        return Err(LeaseError::Refused("a step's progress is at most 200 characters".into()));
    }
    if step.is_empty() || step.chars().count() > 200 {
        return Err(LeaseError::Refused("a step is named in 1 to 200 characters".into()));
    }
    if reason.is_some_and(|r| r.chars().count() > 300) {
        return Err(LeaseError::Refused("a wait's reason is at most 300 characters".into()));
    }
    if waiting_until.is_some() != reason.is_some() {
        return Err(LeaseError::Refused("a wait states both its end (waiting_until) and its reason".into()));
    }
    for text in std::iter::once(step).chain(reason).chain(progress) {
        if let Some(found) = crate::evidence::unredacted(text) {
            return Err(LeaseError::Refused(format!("{found}: a step report carries no credential; nothing was stored")));
        }
    }
    // release audit (Gemini 12): an RFC 3339 instant, parsed strictly here (never echoed when it is not
    // one); a wait already over is kept as reported (the watch's deadline is then the usual one), so a
    // second of clock skew never loses the step; only one more than `WAIT_MAX_HOURS` ahead is refused
    let until: Option<String> = match waiting_until {
        None => None,
        Some(u) => Some(
            u.trim()
                .parse::<jiff::Timestamp>()
                .map_err(|_| LeaseError::Refused("waiting_until is not an RFC 3339 instant (not repeated here)".into()))?
                .to_string(),
        ),
    };
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    if let Some(until) = &until {
        let ok: bool = tx
            .query_one("SELECT $1::text::timestamptz <= clock_timestamp() + make_interval(secs => $2::float8 * 3600)", &[until, &WAIT_MAX_HOURS])
            .await
            .map_err(failed)?
            .get(0);
        if !ok {
            return Err(LeaseError::Refused(format!("waiting_until {until} is more than {WAIT_MAX_HOURS} h ahead")));
        }
    }
    tx.execute(
        "INSERT INTO core.lease_step (lease_id, step, waiting_until, reason, progress, step_since, reported_at)
         VALUES ($1, $2, $3::text::timestamptz, $4, $5, clock_timestamp(), clock_timestamp())
         ON CONFLICT (lease_id) DO UPDATE SET
             step_since = CASE WHEN core.lease_step.step = EXCLUDED.step AND core.lease_step.waiting_until IS NOT DISTINCT FROM EXCLUDED.waiting_until
                                    AND core.lease_step.reason IS NOT DISTINCT FROM EXCLUDED.reason AND core.lease_step.progress IS NOT DISTINCT FROM EXCLUDED.progress
                               THEN core.lease_step.step_since ELSE EXCLUDED.step_since END,
             step = EXCLUDED.step, waiting_until = EXCLUDED.waiting_until, reason = EXCLUDED.reason, progress = EXCLUDED.progress,
             reported_at = EXCLUDED.reported_at",
        &[&lease.lease_id, &step, &until, &reason, &progress],
    )
    .await
    .map_err(failed)?;
    tx.commit().await.map_err(failed)
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
    // audit 2026-11-22 opsurface round 2 (round2-core.md) 9, C-035 amended: a day block's attempts are
    // its next day's. The lease that moves it on is that day's first attempt, so the failures charged on
    // earlier days never set a later day aside on its first failure
    db_client
        .execute(
            "UPDATE core.job SET next_session = (SELECT min(session) FROM core.session WHERE session > $2::text::date AND session <= last_session),
                                 attempts = CASE WHEN kind = 'DAY_BLOCK' THEN 1 ELSE attempts END
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
///
/// Returns true when the job has no session left after it (C-077: the block's last day, or the day a
/// draining block finishes): a worker with more days in its claim then stops after this day's archive
/// and reference check.
pub async fn day_done(client: &mut Client, token: &str, session: &str) -> Result<bool, LeaseError> {
    let tx = client.transaction().await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    if lease.kind == "REPUBLISH" {
        // C-074: done once this lease published the day's day-local revision (with its staged base,
        // promoted in the same transaction, when the scope held base sensors)
        let published: bool = tx
            .query_one(
                "SELECT EXISTS (SELECT FROM core.lease_revision lr JOIN vyreon.day_revision d USING (revision_id)
                                WHERE lr.lease_id = $1 AND d.represented_date = $2::text::date AND d.day_local AND d.state = 'PUBLISHED')",
                &[&lease.lease_id, &session],
            )
            .await
            .map_err(failed)?
            .get(0);
        if !published {
            return Err(LeaseError::Refused(format!("republish job {} has not published {session}'s day-local revision", lease.job_id)));
        }
        session_done(&tx, &lease, session).await?;
        return done_left(tx, lease.job_id).await;
    }
    if lease.kind != "DAY_BLOCK" {
        return Err(LeaseError::Refused(format!("job {} is not a day block", lease.job_id)));
    }
    // C-074 (release-2 audit, Codex 13): a compare-only day block's day is done with both revisions staged
    if crate::publish::compare_only(&tx, &lease).await.map_err(|e| LeaseError::Failed(e.to_string()))? {
        let staged: bool = tx
            .query_one(
                "SELECT EXISTS (SELECT FROM core.lease_revision lr JOIN vyreon.day_revision d USING (revision_id)
                                WHERE lr.lease_id = $1 AND d.represented_date = $2::text::date AND d.day_local AND d.state = 'STAGED')",
                &[&lease.lease_id, &session],
            )
            .await
            .map_err(failed)?
            .get(0);
        if !staged {
            return Err(LeaseError::Refused(format!("compare-only job {} has not staged {session}'s day-local revision", lease.job_id)));
        }
        session_done(&tx, &lease, session).await?;
        return done_left(tx, lease.job_id).await;
    }
    let published = tx
        .query_opt(
            "SELECT b.revision_id, EXISTS (SELECT FROM vyreon.day_revision l
                                           WHERE l.base_revision = b.revision_id AND l.represented_date = b.represented_date
                                             AND l.state = 'PUBLISHED' AND l.day_local)
             FROM vyreon.day_revision b
             WHERE b.represented_date = $1::text::date AND b.profile_id = $2 AND b.engine_build = $3 AND b.state = 'PUBLISHED' AND b.base_revision IS NULL",
            &[&session, &lease.profile_id, &lease.engine_build],
        )
        .await
        .map_err(failed)?;
    let Some(published) = published else {
        return Err(LeaseError::Refused(format!("job {} has not published {session}", lease.job_id)));
    };
    if !published.get::<_, bool>(1) {
        return Err(LeaseError::Refused(format!(
            "job {} published {session}'s base (revision {}) but no day-local revision on it: a day is done with both (C-031, C-070)",
            lease.job_id,
            published.get::<_, i64>(0)
        )));
    }
    session_done(&tx, &lease, session).await?;
    done_left(tx, lease.job_id).await
}

/// `day_done`'s commit and its answer: whether the job has no session left.
async fn done_left(tx: tokio_postgres::Transaction<'_>, job: i64) -> Result<bool, LeaseError> {
    let last: bool = tx.query_one("SELECT next_session IS NULL FROM core.job WHERE job_id = $1", &[&job]).await.map_err(failed)?.get(0);
    tx.commit().await.map_err(failed)?;
    Ok(last)
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
    (SELECT count(*) FROM core.job o WHERE o.kind = core.job.kind AND o.job_id <> core.job.job_id AND o.state IN ('QUEUED', 'LEASED', 'FAILED')),
    (SELECT extract(epoch FROM clock_timestamp() - min(l.granted_at))::float8 / 60 FROM core.lease l WHERE l.job_id = core.job.job_id)";

/// The log line of a finished job (C-050: logged, not posted): one per day block; for the many
/// small jobs, only when the last one is in.
async fn finished(db_client: &impl GenericClient, job: &tokio_postgres::Row, by: &str) -> Result<(), String> {
    let (kind, first, last, remaining, minutes): (String, String, String, i64, Option<f64>) = (job.get(0), job.get(1), job.get(2), job.get(3), job.get(4));
    if kind == "DAY_BLOCK" {
        // C-065: with the block's minutes, from its first lease (every attempt's time included)
        let minutes = minutes.map_or(String::new(), |m| format!(" in {m:.0} min"));
        notice::log(db_client, &format!("Day block {first} to {last} done{minutes} by {by} ({remaining} blocks to go).")).await
    } else if remaining == 0 {
        // C-073 (release audit, Codex 8, Gemini 4): each kind its own words (a re-run is no pass)
        notice::log(db_client, &format!("All scheduled {} are done (the last: {last}).", crate::watch::kind_words(&kind))).await
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
    // before the lease's row, as a publish takes it: a day set aside may take down its base (`requeue`)
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(failed)?;
    let lease = held(&tx, token).await?;
    tx.execute(
        "UPDATE core.lease SET ended_at = now(), outcome = 'FAILED', failed_stage = $2, failure = $3, failure_log = $4 WHERE lease_id = $1",
        &[&lease.lease_id, &stage, &message, &log],
    )
    .await
    .map_err(failed)?;
    // C-075: a day the engine's input plan refused is held (or retried once), never retried to the end
    if matches!(lease.kind.as_str(), "DAY_BLOCK" | "REPUBLISH") {
        refused(&tx, &lease, message).await.map_err(LeaseError::Failed)?;
    }
    let again = requeue(&tx, lease.job_id, &format!("{stage}: {message}")).await.map_err(LeaseError::Failed)?;
    tx.commit().await.map_err(failed)?;
    Ok(again)
}

/// C-075: how the engine says a day was refused by its input plan (`stop: <date> not published: <date>
/// refused by the input plan (plan.json)`, engine `day.rs`).
pub const PLAN_REFUSED: &str = "refused by the input plan";

/// C-075 (coordinator, 2026-11-24; an observability gap): a failed attempt that stopped on a day the
/// engine's input plan refused (the worker's stop message, `PLAN_REFUSED`) is recorded
/// in `core.day_refusal` with the refused inputs (the evidence's `refusals`, the plan's refusing entries).
/// The same refusal comes back on every attempt, hours of node time each, so the job is held at once
/// (its attempts spent: `requeue` makes it FAILED, the day set aside as any day out of attempts, the
/// build going on), unless every refused input is a capture that failed here retryably
/// (`refusal_inputs`), which may be transient: retried once, and held if it is refused again. The
/// watch's DAY_REFUSED says so until the day is published; a new build's hand-over or `queue
/// retry-failed` tries it again.
async fn refused(c: &impl GenericClient, lease: &Held, message: &str) -> Result<(), String> {
    // the worker's own stop message, as it reports the failure (its evidence, stored just before, has the
    // plan's refusing entries)
    if !message.contains(PLAN_REFUSED) {
        return Ok(());
    }
    let evidence = c.query_opt("SELECT refusals::text, session::text FROM core.job_evidence WHERE lease_id = $1", &[&lease.lease_id]).await.map_err(db)?;
    let (refusals, session): (Option<String>, Option<String>) = evidence.map_or((None, None), |r| (r.get(0), r.get(1)));
    // the day the stop message names, else the evidence's, else the job's next
    let named = |m: &str| m.strip_prefix("stop: ").and_then(|r| r.get(..10)).filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-').map(str::to_owned);
    let Some(day) = named(message).or(session).or_else(|| lease.next_session.clone()) else {
        return Ok(());
    };
    let refusals: Value = refusals.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let (inputs, transient) = refusal_inputs(&refusals);
    let earlier: i64 = c
        .query_one("SELECT count(*) FROM core.day_refusal WHERE job_id = $1 AND session = $2::text::date", &[&lease.job_id, &day])
        .await
        .map_err(db)?
        .get(0);
    let hold = !transient || earlier > 0;
    c.execute(
        "INSERT INTO core.day_refusal (job_id, lease_id, profile_id, engine_build, session, inputs, transient, held)
         VALUES ($1, $2, $3, $4, $5::text::date, $6, $7, $8) ON CONFLICT (lease_id) DO NOTHING",
        &[&lease.job_id, &lease.lease_id, &lease.profile_id, &lease.engine_build, &day, &inputs, &transient, &hold],
    )
    .await
    .map_err(db)?;
    if hold {
        c.execute("UPDATE core.job SET attempts = GREATEST(attempts, max_attempts) WHERE job_id = $1", &[&lease.job_id]).await.map_err(db)?;
    }
    let what = if hold { "held: nothing more is tried until a new build or `vyreon-core queue retry-failed`" } else { "every refused input a capture that failed here: tried once more" };
    notice::log(c, &format!("Day {day} refused by the engine's input plan (job {}): {inputs}; {what}.", lease.job_id)).await
}

/// C-075: the refused inputs of a plan's refusing entries (the evidence's `refusals`: each with its
/// `input`, `role`, `session`, `status`, `reason` and failing `assets`), readable on one line, and
/// whether every one is a capture that failed here retryably or was not attempted (the engine's
/// `ENGINE_FAULT: capture failed here (<category> retryable|not_attempted)` of an asset, or a day-wide
/// capture's `capture: "<category> retryable"`, engine D-464: a fill that may succeed again, the
/// provider's throttling or outage among them), never a provider's absence, a stale input or an engine
/// rule.
pub fn refusal_inputs(refusals: &Value) -> (String, bool) {
    let Some(entries) = refusals.as_array().filter(|a| !a.is_empty()) else {
        return ("the input plan's refusals were not reported (its plan.json is in the evidence)".into(), false);
    };
    let retryable = |why: &str| {
        why.split("capture failed here (").nth(1).and_then(|r| r.split(')').next()).is_some_and(|how| how.ends_with(" retryable") || how.ends_with(" not_attempted"))
    };
    let mut transient = true;
    let mut parts = Vec::new();
    for e in entries {
        let text = |k: &str| e[k].as_str().unwrap_or("").to_owned();
        let assets: Vec<(String, String)> = e["assets"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned())).collect()).unwrap_or_default();
        // C-076 (engine D-464): a day-wide capture (the carry inputs) says how the fill left it, `capture`:
        // `carry retryable` is a capture a throttled or failing provider outlasted (2024-07-23, job 21818)
        let capture = e["capture"].as_str().unwrap_or("");
        let day_wide = assets.is_empty() && (capture.ends_with(" retryable") || capture.ends_with(" not_attempted"));
        transient &= day_wide || (!assets.is_empty() && assets.iter().all(|(_, why)| retryable(why)));
        let head = match capture {
            "" => format!("{} ({} {}, {})", text("input"), text("role"), text("session"), text("status")),
            c => format!("{} ({} {}, {}; the fill: {c})", text("input"), text("role"), text("session"), text("status")),
        };
        let named: Vec<String> = assets.iter().take(3).map(|(k, why)| format!("{k}: {}", crate::watch::one_line(why, 80))).collect();
        let more = if assets.len() > 3 { format!(" and {} more", assets.len() - 3) } else { String::new() };
        parts.push(match (named.is_empty(), text("reason")) {
            (false, _) => format!("{head} {}{more}", named.join("; ")),
            (true, r) if !r.is_empty() => format!("{head} {}", crate::watch::one_line(&r, 120)),
            _ => head,
        });
    }
    (crate::watch::one_line(&parts.join(" | "), 400), transient)
}

/// A worker's text as core keeps it: whole, or withheld whole when a credential pattern is in it
/// (`evidence::unredacted`, the patterns C-049's evidence is refused on).
pub fn scrubbed(text: &str) -> String {
    match crate::evidence::unredacted(text) {
        Some(found) => format!("(not kept: it carried a credential pattern, {found})"),
        None => text.to_owned(),
    }
}

/// Stops claims until `resume`. The first reason is kept: it is what stopped the queue. A claim
/// under way holds the control row shared, so the pause waits for it and is stamped after it
/// (`changed_at` on the clock, once the row is locked); every lease granted is then either before
/// the pause or refused by it. Call it in
/// a transaction: the pause and its notice are one event (C-027), and a restart between two
/// autocommitted statements would leave the queue paused with no notice (a second `pause` then
/// changes nothing, so would not repair it).
pub async fn pause(db_client: &impl GenericClient, reason: &str) -> Result<(), String> {
    let reason = &scrubbed(reason);
    // the row first: an UPDATE computes its new row (and its clock) before it waits for a locker
    db_client.execute("SELECT FROM core.queue_control FOR UPDATE", &[]).await.map_err(db)?;
    let paused = db_client
        .execute("UPDATE core.queue_control SET paused = true, reason = $1, changed_at = clock_timestamp() WHERE NOT paused", &[&reason])
        .await
        .map_err(db)?;
    if paused == 1 {
        notice::log(db_client, &format!("Queue paused: {reason}. No work is handed out until `vyreon-core queue resume`.")).await?;
        return Ok(());
    }
    // C-073 (audit c072 r4, Gemini 2; pre-existing): a cause that comes while the queue is already
    // paused (a bundle refused, a job failed for the last time, during the operator's pause) was
    // dropped: no log, no alert, and a resume before anyone looked lost it. It is added to the pause's
    // reason (`also:`, the first reason kept first, at most `PAUSE_CAUSES_KEPT` added, the oldest
    // dropped first) and alerted at once, in this transaction (an outbox row, as the log's): the
    // watch's QUEUE_PAUSED problem is already open and only restates. The same cause again (a
    // worker's retried call) adds nothing and posts nothing.
    let was: Option<String> = db_client.query_one("SELECT reason FROM core.queue_control WHERE paused", &[]).await.map_err(db)?.get(0);
    let Some(was) = was else { return Ok(()) };
    let mut causes: Vec<&str> = was.split(ALSO).collect();
    if causes.iter().any(|c| *c == reason.as_str()) {
        return Ok(());
    }
    causes.push(reason);
    while causes.len() > PAUSE_CAUSES_KEPT + 1 {
        causes.remove(1);
    }
    db_client.execute("UPDATE core.queue_control SET reason = $1 WHERE paused", &[&causes.join(ALSO)]).await.map_err(db)?;
    notice::log(db_client, &format!("Queue already paused; also: {reason}.")).await?;
    notice::alert(db_client, &crate::watch::alert_for("QUEUE_PAUSED", &format!("Work queue already paused; also: {}", crate::watch::one_line(reason, 300)), "vyreon-core queue")).await?;
    Ok(())
}

/// C-073: how a pause's later causes follow its first in `queue_control.reason`.
pub const ALSO: &str = "\nalso: ";
/// C-073: the later causes a pause's reason keeps (the first cause is always kept).
pub const PAUSE_CAUSES_KEPT: usize = 5;

/// Lets work be claimed again. Returns the reason the queue was paused for, if it was. Call it in
/// a transaction, as `pause`.
pub async fn resume(db_client: &impl GenericClient) -> Result<Option<String>, String> {
    let was: Option<String> = db_client.query_one("SELECT reason FROM core.queue_control", &[]).await.map_err(db)?.get(0);
    let resumed = db_client.execute("UPDATE core.queue_control SET paused = false, reason = NULL, changed_at = clock_timestamp() WHERE paused", &[]).await.map_err(db)?;
    if resumed == 1 {
        notice::log(db_client, "Queue resumed.").await?;
    }
    Ok(was)
}

/// C-077 (release-4 audit, Codex 1): resumes only a pause whose reason is exactly `expected` (a release's
/// own), in one statement: a cause added meanwhile (`also: …`, a refused bundle) or another pause is left as
/// it is. Returns whether it resumed; Err names what the pause holds when it is not that.
pub async fn resume_if(db_client: &impl GenericClient, expected: &str) -> Result<bool, String> {
    let expected = scrubbed(expected);
    let resumed = db_client
        .execute("UPDATE core.queue_control SET paused = false, reason = NULL, changed_at = clock_timestamp() WHERE paused AND reason = $1", &[&expected])
        .await
        .map_err(db)?;
    if resumed == 1 {
        notice::log(db_client, "Queue resumed.").await?;
        return Ok(true);
    }
    let now: (bool, Option<String>) = {
        let r = db_client.query_one("SELECT paused, reason FROM core.queue_control", &[]).await.map_err(db)?;
        (r.get(0), r.get(1))
    };
    match now {
        (false, _) => Ok(false),
        (true, reason) => Err(format!(
            "the pause is not only this one: {} -- left paused",
            crate::watch::one_line(&reason.unwrap_or_default().replace(ALSO, "; also: "), 300)
        )),
    }
}

/// Puts FAILED jobs back in the queue with their attempts reset (after the cause is fixed). One
/// whose every session was done is DONE instead: there is nothing left to claim. Returns how many
/// were queued again.
pub async fn retry_failed(db_client: &impl GenericClient) -> Result<u64, String> {
    let rows = db_client
        .query(
            "UPDATE core.job SET state = CASE WHEN next_session IS NULL THEN 'DONE' ELSE 'QUEUED' END,
                                 attempts = CASE WHEN next_session IS NULL THEN attempts ELSE 0 END,
                                 finished_at = CASE WHEN next_session IS NULL THEN finished_at END,
                                 retried_at = CASE WHEN next_session IS NULL THEN retried_at ELSE clock_timestamp() END
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
    // a cancelled day block may take down its day's base (`cancel_in`): the publish lock first
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
    let cancelled = cancel_in(&tx, jobs).await?;
    tx.commit().await.map_err(db)?;
    Ok(cancelled.len() as u64)
}

/// `cancel` inside the caller's transaction. Returns the jobs it cancelled: those of `jobs` still
/// unfinished when it ran (one that finished meanwhile is left as it is, and not named).
pub(crate) async fn cancel_in(tx: &impl GenericClient, jobs: &[i64]) -> Result<Vec<i64>, String> {
    // the leases, then the jobs: the order a worker's call (`held`) and `build::withdraw` take them in
    // (audit 2026-11-22 round 2, core-r2 8: the jobs first, under `retire`, could deadlock with a
    // withdrawal). A live lease is only ever on a LEASED job, which is cancelled below.
    tx.execute(
        "UPDATE core.lease SET ended_at = now(), outcome = 'CANCELLED', failure = 'the job was cancelled' WHERE job_id = ANY($1) AND ended_at IS NULL",
        &[&jobs],
    )
    .await
    .map_err(db)?;
    let rows = tx
        .query(
            "UPDATE core.job SET state = 'CANCELLED', finished_at = now() WHERE job_id = ANY($1) AND state IN ('QUEUED', 'LEASED', 'FAILED')
             RETURNING job_id, kind, next_session::text, profile_id",
            &[&jobs],
        )
        .await
        .map_err(db)?;
    let cancelled: Vec<i64> = rows.iter().map(|r| r.get(0)).collect();
    reject_staged(tx, &cancelled, "the job was cancelled").await?;
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
    // a day block cancelled on a day whose base it published without the day-local revision: the base
    // is superseded, as when a block is set aside (audit 2026-11-22 round 2 core-r2 1)
    for r in rows.iter().filter(|r| r.get::<_, String>(1) == "DAY_BLOCK") {
        let (job, day, profile): (i64, Option<String>, String) = (r.get(0), r.get(2), r.get(3));
        if let Some(day) = day {
            let local = dropped_base_clause(crate::publish::drop_base_without_day_local(tx, job, &day, &profile).await?, &day);
            if !local.is_empty() {
                notice::log(tx, &format!("Day block job {job} cancelled on {day}{local}.")).await?;
            }
        }
    }
    Ok(cancelled)
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
    // one publish at a time, then the scheduler's lock (as `retire`): a base published by hand
    // between the check below and the commit would leave a published day omitted (audit
    // 2026-11-22 changes-core 12)
    tx.execute(PUBLISH_LOCK, &[]).await.map_err(db)?;
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

/// The queue at a glance: jobs by kind, state and build (C-068 round 2, 8: which build owns what), what
/// is claimable, the pause and the backfill's stop (C-087).
pub async fn status(db_client: &impl GenericClient) -> Result<Value, String> {
    let row = db_client
        .query_one(
            "SELECT jsonb_build_object(
                 'paused', (SELECT paused FROM core.queue_control),
                 'pause_reason', (SELECT reason FROM core.queue_control),
                 'backfill_stopped', (SELECT backfill_stopped FROM core.queue_control),
                 'backfill_reason', (SELECT backfill_reason FROM core.queue_control),
                 'jobs', COALESCE((SELECT jsonb_agg(jsonb_build_object('kind', kind, 'state', state, 'build', engine_build, 'jobs', n, 'first', first, 'last', last)
                                                    ORDER BY kind, state, engine_build)
                                   FROM (SELECT kind, state, engine_build, count(*) AS n, min(first_session) AS first, max(last_session) AS last
                                         FROM core.job GROUP BY kind, state, engine_build) j), '[]'::jsonb),
                 'claimable', COALESCE((SELECT jsonb_object_agg(kind, n) FROM (SELECT kind, count(*) AS n FROM core.claimable_job GROUP BY kind) c), '{}'::jsonb),
                 'live_leases', (SELECT count(*) FROM core.lease WHERE ended_at IS NULL))::text",
            &[],
        )
        .await
        .map_err(db)?;
    serde_json::from_str(row.get(0)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit 2026-11-22 prebuild round 2 (core-watch.md) 5: the names real workers give (pods of
    /// the test block, 2026-11-02 and 10-28; the tests' own) are taken; a name that could carry a
    /// credential is refused, and the refusal does not repeat it.
    #[test]
    fn a_worker_name_is_a_host_name() {
        for name in ["v-block-1102-0-wqlv9", "v-block-1028-0-jxmxt", "day-7f2c", "w1", "laptop.local", "DESKTOP_7"] {
            assert_eq!(worker_name(name), Ok(()), "{name}");
        }
        let long = "a".repeat(WORKER_NAME_MAX + 1);
        for name in ["", " ", "-pod", "https://discord.com/api/webhooks/1/a-token", "password=a-token", "Bearer a-token", "pod a-token", "pod@a-token", &long] {
            let refused = worker_name(name).unwrap_err();
            assert!(name.trim().is_empty() || !refused.contains(name.trim()), "{refused}");
            assert!(!refused.contains("a-token"), "{refused}");
        }
        assert_eq!(worker_name(&"a".repeat(WORKER_NAME_MAX)), Ok(()));
    }
}
