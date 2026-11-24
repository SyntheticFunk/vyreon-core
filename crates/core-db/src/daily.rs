//! Daily runs (C-051): each trading session from a start the owner sets is queued by core itself,
//! once the provider has published what its day reads, for the admitted build the settings name.
//! The cluster's CronJob (`deploy/k8s/40-daily.yaml`) then starts a worker that claims it.
//!
//! A session T is due at `READY_AT` in New York on the calendar day after T (a Friday's on
//! Saturday). Evidence (C-051): T's closing option snapshot was not yet listed at 21:00 ET on T
//! (the droplet's 02:00 UTC captures of 2026-11-10, 11-11, 11-12 got T−1's), and every input of T
//! was captured by the next 07:00 ET runs (12:00 UTC, 2026-11-10 to 11-12); Intrinio pushes open
//! interest from 04:00 ET; the engine takes T's daily bars as settled only when captured on a UTC
//! day after T. A session the calendar lacks (a weekend, a holiday) is never due.
//!
//! For each due session one TAPE, one DAY_BLOCK and one ORDERED_PASS job are queued at priority 1
//! (claimed before the backfill's work of the same kind), each only if no job of its kind (not
//! cancelled) already covers the session, and none at all if the session's base revision is
//! already published. `core.daily_session` records that the session was handled, in the same
//! transaction: a session is never queued twice.
use crate::notice;
use crate::work::Kind;
use tokio_postgres::{Client, GenericClient};

/// New York local time, on the calendar day after the session, from which its inputs are ready.
pub const READY_AT: &str = "07:30";
pub const READY_ZONE: &str = "America/New_York";
/// What a daily session gets, in the order the worker runs them.
pub const KINDS: [Kind; 3] = [Kind::Tape, Kind::DayBlock, Kind::OrderedPass];

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The scheduler's settings: `VYREON_CORE_DAILY_FROM` and `VYREON_CORE_DAILY_BUILD`, for the
/// watch's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daily {
    pub profile: String,
    pub build: String,
    /// The first session queued by core itself; earlier ones are the backfill's, scheduled by hand.
    pub from: String,
}

/// One session handled: the jobs queued for it (none when it was already queued or published).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub session: String,
    pub jobs: Vec<i64>,
}

/// Why the build cannot be given work, or None when it can.
pub async fn build_stopped(c: &impl GenericClient, d: &Daily) -> Result<Option<String>, String> {
    let row = c
        .query_opt("SELECT admitted_at IS NOT NULL, withdrawn_at IS NOT NULL FROM core.engine_build WHERE engine_build = $1", &[&d.build])
        .await
        .map_err(db)?;
    Ok(match row.map(|r| (r.get::<_, bool>(0), r.get::<_, bool>(1))) {
        None => Some("is not registered".into()),
        Some((_, true)) => Some("was withdrawn".into()),
        Some((false, _)) => Some("is not admitted".into()),
        Some((true, false)) => None,
    })
}

/// C-072: the hand-over that explains why the build cannot be given work, a planned step: the
/// build's current withdrawal is that hand-over's own (made in its transaction, with its reason),
/// or it is a hand-over's successor never admitted yet. A withdrawal of its own (a failure, or one
/// after a re-admission) is no hand-over's (audit c072, 3). (hand-over id, its other build, when
/// it was made); None otherwise.
pub async fn handed_over(c: &impl GenericClient, d: &Daily) -> Result<Option<(i64, String, String)>, String> {
    let row = c
        .query_opt(
            "SELECT h.hand_over_id, CASE WHEN h.from_build = $1 THEN h.to_build ELSE h.from_build END, h.handed_at::text
             FROM core.hand_over h JOIN core.engine_build b ON b.engine_build = $1
             WHERE (h.from_build = $1 AND b.withdrawal = 'handed over to build ' || h.to_build || ': ' || h.reason
                    AND b.withdrawn_at BETWEEN h.handed_at - interval '1 hour' AND h.handed_at)
                OR (h.to_build = $1 AND b.admitted_at IS NULL)
             ORDER BY h.handed_at DESC, h.hand_over_id DESC LIMIT 1",
            &[&d.build],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|r| (r.get(0), r.get(1), r.get(2))))
}

/// The sessions due at `now` and not handled yet, oldest first.
pub async fn due(c: &impl GenericClient, d: &Daily, now: &str) -> Result<Vec<String>, String> {
    let rows = c
        .query(
            "SELECT s.session::text FROM core.session s
             WHERE s.session >= $1::text::date
               AND ((s.session + 1) + $2::text::time) AT TIME ZONE $3 <= $4::text::timestamptz
               AND NOT EXISTS (SELECT FROM core.daily_session q WHERE q.session = s.session AND q.profile_id = $5)
             ORDER BY 1",
            &[&d.from, &READY_AT, &READY_ZONE, &now, &d.profile],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Queues every session due at `now`. Does nothing while none is due; refuses (and queues
/// nothing) while the build cannot be given work.
pub async fn queue_due(client: &mut Client, d: &Daily, now: &str) -> Result<Vec<Queued>, String> {
    let sessions = due(&*client, d, now).await?;
    let (Some(first), Some(last)) = (sessions.first(), sessions.last()) else {
        return Ok(Vec::new());
    };
    if let Some(why) = build_stopped(&*client, d).await? {
        return Err(format!("daily runs: build {} {why}; {} due sessions are not queued", d.build, sessions.len()));
    }
    // the store's partitions first, in their own transaction (C-021), as `work::schedule` does
    client.query_one("SELECT vyreon.ensure_months($1::text::date, $2::text::date)", &[first, last]).await.map_err(db)?;
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    // audit 2026-11-22 opsurface round 2 (round2-core.md) 7, C-068: read again under the scheduler's
    // lock, which a hand-over holds while it withdraws the build and moves its work: a tick that read
    // the build admitted before the hand-over queued its sessions under the withdrawn build after it
    // (and marked them handled, so the successor never queued them)
    if let Some(why) = build_stopped(&tx, d).await? {
        return Err(format!("daily runs: build {} {why}; {} due sessions are not queued", d.build, sessions.len()));
    }
    let mut out = Vec::new();
    for session in &sessions {
        let mine = tx
            .execute(
                "INSERT INTO core.daily_session (session, profile_id, engine_build, queued_at, jobs)
                 VALUES ($1::text::date, $2, $3, $4::text::timestamptz, '{}') ON CONFLICT DO NOTHING",
                &[session, &d.profile, &d.build, &now],
            )
            .await
            .map_err(db)?;
        if mine == 0 {
            continue;
        }
        let published = tx
            .query_opt(
                "SELECT FROM vyreon.day_revision WHERE represented_date = $1::text::date AND profile_id = $2 AND state = 'PUBLISHED' AND base_revision IS NULL LIMIT 1",
                &[session, &d.profile],
            )
            .await
            .map_err(db)?
            .is_some();
        let mut jobs = Vec::new();
        let mut kept = Vec::new();
        for kind in KINDS {
            if published {
                break;
            }
            let covered = tx
                .query_opt(
                    "SELECT job_id FROM core.job WHERE kind = $1 AND profile_id = $2 AND state <> 'CANCELLED'
                       AND $3::text::date BETWEEN first_session AND last_session
                       AND NOT COALESCE(scope @> '{\"verify\": true}', false) LIMIT 1",
                    &[&kind.name(), &d.profile, session],
                )
                .await
                .map_err(db)?;
            if covered.is_some() {
                kept.push(kind.name());
                continue;
            }
            let job: i64 = tx
                .query_one(
                    "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, priority)
                     VALUES ($1, $2, $3, $4::text::date, $4::text::date, $4::text::date, 1) RETURNING job_id",
                    &[&kind.name(), &d.profile, &d.build, session],
                )
                .await
                .map_err(db)?
                .get(0);
            jobs.push(job);
        }
        tx.execute(
            "UPDATE core.daily_session SET jobs = $3 WHERE session = $1::text::date AND profile_id = $2",
            &[session, &d.profile, &jobs],
        )
        .await
        .map_err(db)?;
        let what = if published {
            "already published: nothing queued".to_string()
        } else if kept.is_empty() {
            format!("queued for build {} (jobs {jobs:?})", d.build)
        } else {
            format!("queued for build {} (jobs {jobs:?}); already queued by hand: {}", d.build, kept.join(", "))
        };
        notice::log(&tx, &format!("Daily run: session {session} {what}.")).await?;
        out.push(Queued { session: session.clone(), jobs });
    }
    tx.commit().await.map_err(db)?;
    Ok(out)
}

/// The digest's line: whether daily runs are on, the last session queued and when the next is due
/// (in the owner's zone).
pub async fn digest_line(c: &impl GenericClient, d: Option<&Daily>, now: &str, zone: &str) -> Result<String, String> {
    let Some(d) = d else {
        return Ok("Daily runs: off (no first session or build is set)".into());
    };
    let last: Option<String> = c
        .query_one("SELECT max(session)::text FROM core.daily_session WHERE profile_id = $1", &[&d.profile])
        .await
        .map_err(db)?
        .get(0);
    let next: Option<String> = c
        .query_one(
            "SELECT to_char((((min(s.session) + 1) + $2::text::time) AT TIME ZONE $3) AT TIME ZONE $5, 'Dy HH24:MI') || ' MT'
             FROM core.session s
             WHERE s.session >= $1::text::date AND ((s.session + 1) + $2::text::time) AT TIME ZONE $3 > $4::text::timestamptz",
            &[&d.from, &READY_AT, &READY_ZONE, &now, &zone],
        )
        .await
        .map_err(db)?
        .get(0);
    let last = last.map_or_else(|| format!("none yet (from {})", d.from), |l| format!("last queued {l}"));
    let next = next.map_or_else(|| "no later session in the calendar".to_string(), |n| format!("next queued {n}"));
    Ok(format!("Daily runs: {last} · {next}"))
}
