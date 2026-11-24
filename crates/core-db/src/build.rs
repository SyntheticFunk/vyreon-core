//! Registering an engine build with its slot inventory (migration 0003). The inventory is what
//! publish checks every day of the build against, so it is written once: registering a build
//! again is accepted only with exactly the same inventory.
//!
//! The inventory document: `{"engine_build": "...", "slots": [{"profile_revision", "slot_ordinal",
//! "measurement_id", "object_id", "layer"}, ...]}`.
use serde_json::Value;
use tokio_postgres::Client;

#[derive(Debug, PartialEq, Eq)]
pub struct Registered {
    pub engine_build: String,
    pub slots: i64,
    /// False when the build was already registered with this same inventory.
    pub new: bool,
}

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The longest build name `register` takes.
pub const BUILD_NAME_MAX: usize = 64;

/// C-073 (audit c072 r4: a build's name is put into progress and alert lines as it is, so one that
/// could carry a credential reached the progress channel): a build is named as the engine names it,
/// `<commit>+<inventory digest>`, 1 to `BUILD_NAME_MAX` letters, digits, '+', '.', '_' or '-',
/// beginning with a letter or digit; anything else is refused before it is stored, without being
/// repeated.
pub fn build_name(build: &str) -> Result<(), String> {
    let mut chars = build.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if first_ok && build.len() <= BUILD_NAME_MAX && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '_' | '-')) {
        Ok(())
    } else {
        Err(format!(
            "inventory: an engine build is named with 1 to {BUILD_NAME_MAX} letters, digits, '+', '.', '_' or '-' (the engine's <commit>+<inventory>); \
             this one is not such a name (not repeated here)"
        ))
    }
}

/// C-073: an inventory's `profiles` (profile revision -> base | day-local | history), checked: every
/// profile revision of its slots declared, no other, at most one base and one day-local profile. None
/// when the inventory declares none (an engine before the declaration).
pub fn declared_profiles(inventory: &Value) -> Result<Option<Vec<(String, String)>>, String> {
    let Some(map) = inventory.get("profiles") else { return Ok(None) };
    let map = map.as_object().ok_or("inventory: profiles is an object of profile revision: layer")?;
    let mut out = Vec::new();
    for (profile, layer) in map {
        let layer = layer.as_str().filter(|l| ["base", "day-local", "history"].contains(l)).ok_or_else(|| format!("inventory: profile {profile}'s layer is base, day-local or history"))?;
        out.push((profile.clone(), layer.to_owned()));
    }
    let in_slots: std::collections::BTreeSet<&str> =
        inventory["slots"].as_array().into_iter().flatten().filter_map(|s| s["profile_revision"].as_str()).collect();
    let declared: std::collections::BTreeSet<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
    if in_slots != declared {
        return Err("inventory: profiles declares exactly the profile revisions of its slots".into());
    }
    for layer in ["base", "day-local"] {
        if out.iter().filter(|(_, l)| l == layer).count() > 1 {
            return Err(format!("inventory: a build has one {layer} profile"));
        }
    }
    Ok(Some(out))
}

pub async fn register(client: &mut Client, inventory: &Value) -> Result<Registered, String> {
    let build = inventory["engine_build"].as_str().filter(|b| !b.is_empty()).ok_or("inventory: no engine_build")?.to_owned();
    build_name(&build)?;
    if inventory["slots"].as_array().is_none_or(Vec::is_empty) {
        return Err("inventory: no slots".into());
    }
    const SLOTS: &str = "jsonb_to_recordset($1::text::jsonb->'slots')
        AS s(profile_revision text, slot_ordinal integer, measurement_id text, object_id text, layer text)";
    let text = inventory.to_string();
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('engine_build ' || $1, 0))", &[&build]).await.map_err(db)?;
    let known = tx.query_opt("SELECT FROM core.engine_build WHERE engine_build = $1", &[&build]).await.map_err(db)?.is_some();
    if known {
        // the same rows on both sides, or it is refused
        let differing: i64 = tx
            .query_one(
                &format!(
                    "SELECT count(*) FROM (
                         (SELECT profile_revision, slot_ordinal, measurement_id, object_id, layer FROM core.expected_slot WHERE engine_build = $2
                          EXCEPT SELECT profile_revision, slot_ordinal, measurement_id, object_id, layer FROM {SLOTS})
                         UNION ALL
                         (SELECT profile_revision, slot_ordinal, measurement_id, object_id, layer FROM {SLOTS}
                          EXCEPT SELECT profile_revision, slot_ordinal, measurement_id, object_id, layer FROM core.expected_slot WHERE engine_build = $2)) d"
                ),
                &[&text, &build],
            )
            .await
            .map_err(db)?
            .get(0);
        if differing != 0 {
            return Err(format!("engine build {build} is already registered with a different inventory ({differing} rows differ)"));
        }
    } else {
        tx.execute("INSERT INTO core.engine_build (engine_build) VALUES ($1)", &[&build]).await.map_err(db)?;
        tx.execute(
            &format!(
                "INSERT INTO core.expected_slot (engine_build, profile_revision, slot_ordinal, measurement_id, object_id, layer)
                 SELECT $2, profile_revision, slot_ordinal, measurement_id, object_id, layer FROM {SLOTS}"
            ),
            &[&text, &build],
        )
        .await
        .map_err(db)?;
    }
    // C-073: the layer of each profile revision, as the inventory declares it (`profiles`)
    if let Some(profiles) = declared_profiles(inventory)? {
        for (profile, layer) in &profiles {
            let stored: Option<String> = tx
                .query_opt("SELECT layer FROM core.build_profile WHERE engine_build = $1 AND profile_revision = $2", &[&build, profile])
                .await
                .map_err(db)?
                .map(|r| r.get(0));
            match stored {
                Some(l) if &l != layer => return Err(format!("engine build {build}'s profile {profile} is registered as its {l} profile, not {layer}")),
                Some(_) => {}
                None => {
                    tx.execute("INSERT INTO core.build_profile (engine_build, profile_revision, layer) VALUES ($1, $2, $3)", &[&build, profile, layer])
                        .await
                        .map_err(db)?;
                }
            }
        }
    }
    // C-074: the engine's republish plans (engine D-462), kept as the build declared them; a build
    // registered again keeps its first (its inventory is the same: checked above)
    if let Some(plans) = inventory.get("republish").filter(|p| !p.is_null()) {
        if !plans["seeds"].is_object() {
            return Err("inventory: republish has no seeds".into());
        }
        tx.execute(
            "INSERT INTO core.build_republish (engine_build, plans) VALUES ($1, $2::text::jsonb) ON CONFLICT (engine_build) DO NOTHING",
            &[&build, &plans.to_string()],
        )
        .await
        .map_err(db)?;
        // (release-2 audit, Gemini 16b) the same plans on both sides, as the slots, or it is refused
        let same: bool = tx
            .query_one("SELECT plans = $2::text::jsonb FROM core.build_republish WHERE engine_build = $1", &[&build, &plans.to_string()])
            .await
            .map_err(db)?
            .get(0);
        if !same {
            return Err(format!("engine build {build} is already registered with other republish plans"));
        }
    }
    let slots: i64 = tx.query_one("SELECT count(*) FROM core.expected_slot WHERE engine_build = $1", &[&build]).await.map_err(db)?.get(0);
    tx.commit().await.map_err(db)?;
    Ok(Registered { engine_build: build, slots, new: !known })
}

/// Admits a registered build to the queue, with the evidence it was admitted on (C-014b): the
/// checks, their results and who ran them. For the first build this is a checklist by hand.
pub async fn admit(client: &Client, build: &str, evidence: &Value) -> Result<(), String> {
    if !evidence.is_object() || evidence.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err("admission needs its evidence: a JSON object of the checks and their results".into());
    }
    let n = client
        .execute(
            "UPDATE core.engine_build SET admitted_at = now(), admission = $2::text::jsonb, withdrawn_at = NULL, withdrawal = NULL WHERE engine_build = $1",
            &[&build, &evidence.to_string()],
        )
        .await
        .map_err(db)?;
    if n == 0 {
        return Err(format!("engine build {build} is not registered"));
    }
    crate::notice::log(client, &format!("Engine build {build} admitted: its workers may claim work.")).await
}

/// Withdraws an admitted build: its queued jobs are no longer claimable, and its workers stop (audit
/// 2026-11-22 changes-core 8 and the triage's note: a live lease of a withdrawn build could still
/// publish, so a later pass, no longer waiting for it, published before it). Every live lease of the
/// build's jobs ends (CANCELLED) and its job goes back to the queue under the build, its attempt not
/// counted (DONE when every session was done): the worker is refused at its next call, a heartbeat
/// or a publish. A publish already under way holds its lease's row: the withdrawal waits for it to
/// commit (that day is published, before the withdrawal) or the publish finds the lease ended and
/// writes nothing; never half. A claim takes the build's row first (`work::claim`), so none is
/// granted under a build being withdrawn. Returns the jobs whose lease it ended.
///
/// It takes the publish lock first, as a publish, a claim of an ordered pass, `retire`, `omit`, a
/// schedule of ordered passes, `cancel`, a lease's failure and its expiry do (audit 2026-11-22 round 2,
/// core-r2 7 and 8): a publish's re-derive then reads one state of the builds in all its statements
/// (a withdrawal between them left an old build's queued pass in the way of the new build's), and the
/// leases-then-jobs order every holder of the lock takes rows in leaves no cycle with `retire`.
pub async fn withdraw(client: &mut Client, build: &str, reason: &str) -> Result<Vec<i64>, String> {
    let tx = client.transaction().await.map_err(db)?;
    let sorted = withdraw_in(&tx, build, reason).await?;
    tx.commit().await.map_err(db)?;
    Ok(sorted)
}

/// `withdraw` inside the caller's transaction (C-068: `hand_over` withdraws in its own).
async fn withdraw_in(tx: &tokio_postgres::Transaction<'_>, build: &str, reason: &str) -> Result<Vec<i64>, String> {
    tx.execute(crate::work::PUBLISH_LOCK, &[]).await.map_err(db)?;
    // the clock, not the transaction's start, as a publish and a claim stamp theirs (core-r2 11): taken
    // after the locks, a lease's end is never before the publish or the grant the withdrawal waited for
    let n = tx
        .execute("UPDATE core.engine_build SET withdrawn_at = clock_timestamp(), withdrawal = $2 WHERE engine_build = $1 AND admitted_at IS NOT NULL", &[&build, &reason])
        .await
        .map_err(db)?;
    if n == 0 {
        return Err(format!("engine build {build} is not admitted"));
    }
    // the leases, then their jobs, in two statements: the order of every holder of the publish lock
    let ended: Vec<i64> = tx
        .query(
            "UPDATE core.lease l SET ended_at = clock_timestamp(), outcome = 'CANCELLED', failure = 'its engine build was withdrawn'
             FROM core.job j
             WHERE l.job_id = j.job_id AND j.engine_build = $1 AND l.ended_at IS NULL
             RETURNING l.job_id",
            &[&build],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let stopped: Vec<i64> = tx
        .query(
            "UPDATE core.job SET state = CASE WHEN next_session IS NULL THEN 'DONE' ELSE 'QUEUED' END,
                                 attempts = GREATEST(attempts - 1, 0),
                                 finished_at = CASE WHEN next_session IS NULL THEN clock_timestamp() END
             WHERE job_id = ANY($1) AND state = 'LEASED'
             RETURNING job_id",
            &[&ended],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // C-074 (release-2 audit, Codex 15, Gemini 14): a stopped republish's staged base is rejected
    crate::work::reject_staged(tx, &ended, "its engine build was withdrawn").await?;
    let mut sorted = stopped.clone();
    sorted.sort_unstable();
    let leases = match sorted.len() {
        0 => String::new(),
        n => format!(" {n} running jobs stopped, their leases ended: {}.", sorted.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")),
    };
    crate::notice::log(tx, &format!("Engine build {build} withdrawn: {reason}.{leases}")).await?;
    Ok(sorted)
}

/// One job a hand-over carries: the withdrawn build's job and what took its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    pub old_job: i64,
    pub kind: String,
    /// QUEUED or FAILED when it was carried.
    pub state: String,
    /// The sessions carried: from the old job's next session to its last.
    pub first: String,
    pub last: String,
    /// The successor's new job; None in a preview.
    pub new_job: Option<i64>,
}

/// What `hand_over` did, or, previewed, would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandOver {
    /// None when previewed: nothing was changed.
    pub hand_over_id: Option<i64>,
    /// `from` still ran and the hand-over withdrew it (its live leases ended, their jobs carried).
    pub withdrew: bool,
    pub carried: Vec<Carried>,
    /// `from`'s unfinished tape verify jobs, cancelled and not carried (C-068 round 2, 11: a sample of
    /// the old build's caches, which held a schedule of the successor's tapes refused as an overlap).
    pub verify_cancelled: Vec<i64>,
    /// What moves, as a token: the old jobs with their states and next sessions, and the two builds.
    /// The hand-over itself is refused unless it is given the same.
    pub token: String,
}

/// Audit 2026-11-22 opsurface (ops.md) 1 and 4, C-068: hands the unfinished work of a build to its
/// successor, once. The successor must not be admitted yet (round 2, 1: so none of its workers runs a
/// later pass before the moved ones; it is admitted after the hand-over); the old build, if it still
/// runs, is withdrawn in the same transaction (`withdraw`: its live leases end, their jobs carried). Every unfinished job of
/// `from` (QUEUED or FAILED; a tape verify job, a sample that holds nothing back, is left) is
/// cancelled and replaced by the same work under `to`, its attempts fresh: a tape and an ordered
/// pass for their session, a day block for its sessions from its next one (the days it did become a
/// DONE job of `from`, as when a block is set aside), with the old job's priority and scope. A job
/// of another build unfinished on the same sessions refuses the whole hand-over (none can be: a
/// schedule refuses an overlap, and a pass is one per session). Without it, the withdrawn build's queued tapes and day blocks held the
/// successor's blocks and passes for ever (the claim's gates count them, and must: a block may not
/// run before every tape it reads, nor a pass before the block that computes its day), and its
/// queued passes were claimed by no worker (C-066 counts the successor's jobs only).
///
/// With `preview`, everything is done inside a transaction rolled back and the report says exactly
/// what it would do, with its `token`; without it, `expect` must be that token, so nothing the
/// operator did not see is moved (C-063's rule for `days retire`). Run once: afterwards `from` holds
/// no unfinished work and a second hand-over is refused. Takes the publish lock, then the scheduler's,
/// as `retire` and a schedule of passes do.
pub async fn hand_over(client: &mut Client, from: &str, to: &str, reason: &str, preview: bool, expect: Option<&str>) -> Result<HandOver, String> {
    let reason = crate::work::scrubbed(reason.trim());
    if reason.is_empty() {
        return Err("a hand-over needs a reason".into());
    }
    if from == to {
        return Err("a build hands its work to another build".into());
    }
    if !preview && expect.is_none() {
        return Err("a hand-over moves only what its preview showed: run it with --preview first and give the token it prints (--expect); nothing was moved".into());
    }
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(crate::work::PUBLISH_LOCK, &[]).await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    for build in [from, to] {
        if tx.query_opt("SELECT FROM core.engine_build WHERE engine_build = $1", &[&build]).await.map_err(db)?.is_none() {
            return Err(format!("engine build {build} is not registered"));
        }
    }
    let (from_runs, to_runs): (bool, bool) = {
        let r = tx.query_one("SELECT core.build_runs($1), core.build_runs($2)", &[&from, &to]).await.map_err(db)?;
        (r.get(0), r.get(1))
    };
    // audit round 2 (round2-core.md) 1: the successor must not run before the hand-over. A worker of
    // an admitted successor could claim a later day's pass while an earlier day's pass still belonged
    // to the withdrawn build, and publish that later day's history out of date order (one prior
    // reading short) before the earlier pass was moved and run. Not admitted, its workers claim
    // nothing until every moved job is its own; then it is admitted.
    if to_runs {
        return Err(format!(
            "build {to} is admitted: hand over before admitting it (its workers could run a later pass before the moved ones); nothing was moved"
        ));
    }
    let to_leases: i64 = tx
        .query_one("SELECT count(*) FROM core.lease l JOIN core.job j USING (job_id) WHERE j.engine_build = $1 AND l.ended_at IS NULL", &[&to])
        .await
        .map_err(db)?
        .get(0);
    if to_leases > 0 {
        return Err(format!("build {to} has {to_leases} live leases: let them end first; nothing was moved"));
    }
    // the old build is withdrawn here when it still runs, in this transaction: nothing of it runs
    // between its withdrawal and the hand-over
    // the jobs its withdrawal stopped (their leases ended here): the hand-over's "restarted" (C-073,
    // audit c072 r4: counted from any earlier withdrawal's leases before)
    let stopped = if from_runs { withdraw_in(&tx, from, &format!("handed over to build {to}: {reason}")).await? } else { Vec::new() };
    let verify_cancelled: Vec<i64> = tx
        .query(
            "UPDATE core.job SET state = 'CANCELLED', finished_at = clock_timestamp()
             WHERE engine_build = $1 AND kind = 'TAPE' AND state IN ('QUEUED', 'FAILED') AND COALESCE(scope @> $2::text::jsonb, false)
             RETURNING job_id",
            &[&from, &crate::tape::VERIFY_SCOPE],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let rows = tx
        .query(
            "SELECT job_id, kind, state, profile_id, next_session::text, last_session::text
             FROM core.job
             WHERE engine_build = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED') AND next_session IS NOT NULL
               AND NOT (kind = 'TAPE' AND COALESCE(scope @> $2::text::jsonb, false))
             ORDER BY CASE kind WHEN 'TAPE' THEN 0 WHEN 'DAY_BLOCK' THEN 1 ELSE 2 END, first_session, job_id FOR UPDATE",
            &[&from, &crate::tape::VERIFY_SCOPE],
        )
        .await
        .map_err(db)?;
    if let Some(r) = rows.iter().find(|r| r.get::<_, String>(2) == "LEASED") {
        return Err(format!("job {} of build {from} is under a live lease: let it end or cancel it first; nothing was moved", r.get::<_, i64>(0)));
    }
    if rows.is_empty() && verify_cancelled.is_empty() {
        return Err(format!("build {from} has no unfinished work to hand over"));
    }
    // audit round 2 (round2-core.md) 6, C-064: a pass is carried only where the successor can run it.
    // A day published (by any build) whose base or day-local revision is not of a profile revision the
    // successor reads (`core.day_published` for it) is a day it cannot read: with no day block to
    // compute it again, its pass would never be claimed and would hold every later one. Such days are
    // recomputed under the successor first: schedule its day blocks over them, then hand over.
    // C-074 (release-2 audit, Gemini 3): a republish copies forward only within its day's profile
    // revisions; one whose day the successor does not read in its own cannot move
    let stale: Vec<(i64, String)> = tx
        .query(
            "SELECT j.job_id, j.first_session::text FROM core.job j
             WHERE j.engine_build = $1 AND j.kind = 'REPUBLISH' AND j.state IN ('QUEUED', 'FAILED')
               AND NOT core.day_published(j.first_session, j.profile_id, $2) ORDER BY 2",
            &[&from, &to],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    if let Some((job, day)) = stale.first() {
        return Err(format!(
            "republish job {job} ({day}) and {} more: their days are published in profile revisions build {to} does not publish, so they \
             cannot be copied forward under it; cancel them and recompute those days whole; nothing was moved",
            stale.len() - 1
        ));
    }
    let unreadable: Vec<String> = tx
        .query(
            "SELECT DISTINCT j.first_session::text FROM core.job j
             WHERE j.engine_build = $1 AND j.kind = 'ORDERED_PASS' AND j.state IN ('QUEUED', 'FAILED')
               AND NOT core.day_published(j.first_session, j.profile_id, $2)
               AND EXISTS (SELECT FROM vyreon.day_revision b WHERE b.represented_date = j.first_session AND b.profile_id = j.profile_id
                             AND b.state = 'PUBLISHED' AND b.base_revision IS NULL)
               AND NOT EXISTS (SELECT FROM core.job d WHERE d.kind = 'DAY_BLOCK' AND d.profile_id = j.profile_id AND d.engine_build IN ($1, $2)
                                 AND d.state IN ('QUEUED', 'FAILED') AND j.first_session BETWEEN d.next_session AND d.last_session)
             ORDER BY 1",
            &[&from, &to],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if !unreadable.is_empty() {
        let more = if unreadable.len() > 10 { format!(" and {} more", unreadable.len() - 10) } else { String::new() };
        return Err(format!(
            "{} days are published in profile revisions build {to} does not read, and no day block computes them again: {}{more}. \
             Schedule build {to}'s day blocks over them first (C-064: they are recomputed), then hand over; nothing was moved",
            unreadable.len(),
            unreadable.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let mut carried = Vec::new();
    for r in &rows {
        let (job, kind, state, profile, next, last): (i64, String, String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        // what else is unfinished on these sessions: none can be, since a schedule refuses an overlap
        // and a pass is one per session (0032), but a job queued by hand in SQL would make two
        let other: Option<(i64, String)> = tx
            .query_opt(
                "SELECT job_id, engine_build FROM core.job
                 WHERE (kind = $1 OR (kind IN ('DAY_BLOCK', 'REPUBLISH') AND $1 IN ('DAY_BLOCK', 'REPUBLISH')))
                   AND profile_id = $2 AND engine_build <> $3 AND state IN ('QUEUED', 'LEASED', 'FAILED')
                   AND NOT (kind = 'TAPE' AND COALESCE(scope @> $6::text::jsonb, false))
                   -- C-073 (release audit, Gemini 5): re-runs of one session's different revisions may stand
                   -- side by side (one per revision pair, migration 0055's index); they never overlap
                   AND kind <> 'REFCHECK'
                   AND first_session <= $5::text::date AND last_session >= $4::text::date
                 ORDER BY job_id LIMIT 1",
                &[&kind, &profile, &from, &next, &last, &crate::tape::VERIFY_SCOPE],
            )
            .await
            .map_err(db)?
            .map(|o| (o.get(0), o.get(1)));
        if let Some((other_job, of)) = other {
            return Err(format!(
                "{kind} job {job} ({next} to {last}) of build {from} overlaps unfinished job {other_job} of build {of}: cancel one first; nothing was moved"
            ));
        }
        carried.push(Carried { old_job: job, kind, state, first: next, last, new_job: None });
    }
    let token = {
        let jobs: Vec<String> = carried.iter().map(|c| format!("{}:{}:{}", c.old_job, c.state, c.first)).collect();
        let verify: Vec<String> = verify_cancelled.iter().map(i64::to_string).collect();
        core_contract::sha256_hex(format!("from {from} (running {from_runs}); to {to}; jobs {}; verify {}", jobs.join(","), verify.join(",")).as_bytes())[..16]
            .to_owned()
    };
    if let Some(expected) = expect.filter(|e| !preview && *e != token) {
        return Err(format!(
            "what build {from} holds changed since the preview (its token {expected}, now {token}): a job was scheduled, claimed, finished or cancelled; preview again; nothing was moved"
        ));
    }
    let hand_over_id: i64 = tx
        .query_one("INSERT INTO core.hand_over (from_build, to_build, reason) VALUES ($1, $2, $3) RETURNING hand_over_id", &[&from, &to, &reason])
        .await
        .map_err(db)?
        .get(0);
    for job in &verify_cancelled {
        tx.execute("INSERT INTO core.hand_over_job (hand_over_id, old_job, new_job, restarted) VALUES ($1, $2, NULL, false)", &[&hand_over_id, job]).await.map_err(db)?;
    }
    for c in &mut carried {
        if c.kind == "DAY_BLOCK" {
            // the days it did: a finished job of their own, as `work::requeue` keeps them
            tx.execute(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, state, attempts, max_attempts, finished_at, priority)
                 SELECT kind, profile_id, engine_build, first_session,
                        (SELECT max(session) FROM core.session WHERE session < j.next_session), NULL, scope, 'DONE', attempts, max_attempts, clock_timestamp(), priority
                 FROM core.job j WHERE job_id = $1 AND next_session > first_session",
                &[&c.old_job],
            )
            .await
            .map_err(db)?;
            tx.execute("UPDATE core.job SET first_session = next_session WHERE job_id = $1", &[&c.old_job]).await.map_err(db)?;
        }
        // cancelled before its successor is queued: one unfinished pass per session (0032)
        tx.execute("UPDATE core.job SET state = 'CANCELLED', finished_at = clock_timestamp() WHERE job_id = $1", &[&c.old_job]).await.map_err(db)?;
        let new: i64 = tx
            .query_one(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope, max_attempts, priority)
                 SELECT kind, profile_id, $2, next_session, last_session, next_session, scope, max_attempts, priority FROM core.job WHERE job_id = $1
                 RETURNING job_id",
                &[&c.old_job, &to],
            )
            .await
            .map_err(db)?
            .get(0);
        tx.execute(
            "INSERT INTO core.hand_over_job (hand_over_id, old_job, new_job, restarted) VALUES ($1, $2, $3, $4)",
            &[&hand_over_id, &c.old_job, &new, &stopped.contains(&c.old_job)],
        )
        .await
        .map_err(db)?;
        if c.kind == "REPUBLISH" {
            // C-074: the recompute set is the successor's graph's (its worker checks it against its own)
            let old: serde_json::Value = tx
                .query_one("SELECT scope::text FROM core.job WHERE job_id = $1", &[&c.old_job])
                .await
                .map_err(db)
                .map(|r| serde_json::from_str(r.get::<_, &str>(0)).unwrap_or_default())?;
            let names = |k: &str| old[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>();
            let plans = crate::republish::plans(&tx, to).await.map_err(|e| format!("republish job {} cannot move to build {to}: {e}; cancel it first; nothing was moved", c.old_job))?;
            let work = crate::republish::compose(&plans, &names("families"), &names("measurements"))
                .map_err(|e| format!("republish job {} cannot move to build {to}: {e}; cancel it first; nothing was moved", c.old_job))?;
            let scope = work.scope(old["reason"].as_str().unwrap_or_default()).to_string();
            tx.execute("UPDATE core.job SET scope = $2::text::jsonb WHERE job_id = $1", &[&new, &scope]).await.map_err(db)?;
        }
        c.new_job = Some(new);
    }
    let counts: Vec<String> = ["TAPE", "DAY_BLOCK", "ORDERED_PASS", "REFCHECK", "REPUBLISH"]
        .iter()
        .filter_map(|k| {
            let n = carried.iter().filter(|c| c.kind == *k).count();
            (n > 0).then(|| format!("{n} {}", crate::watch::kind_words(k)))
        })
        .collect();
    crate::notice::log(
        &tx,
        &format!(
            "Hand-over {hand_over_id}: the unfinished work of build {from} ({}) is build {to}'s, the old jobs cancelled{}: {reason}. Admit build {to} now.",
            if counts.is_empty() { "nothing to carry".to_owned() } else { counts.join(", ") },
            if verify_cancelled.is_empty() { String::new() } else { format!(", {} tape verify jobs cancelled", verify_cancelled.len()) }
        ),
    )
    .await?;
    if preview {
        tx.rollback().await.map_err(db)?;
        let carried = carried.into_iter().map(|c| Carried { new_job: None, ..c }).collect();
        return Ok(HandOver { hand_over_id: None, withdrew: from_runs, carried, verify_cancelled, token });
    }
    tx.commit().await.map_err(db)?;
    Ok(HandOver { hand_over_id: Some(hand_over_id), withdrew: from_runs, carried, verify_cancelled, token })
}
