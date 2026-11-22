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

pub async fn register(client: &mut Client, inventory: &Value) -> Result<Registered, String> {
    let build = inventory["engine_build"].as_str().filter(|b| !b.is_empty()).ok_or("inventory: no engine_build")?.to_owned();
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
    let mut sorted = stopped.clone();
    sorted.sort_unstable();
    let leases = match sorted.len() {
        0 => String::new(),
        n => format!(" {n} running jobs stopped, their leases ended: {}.", sorted.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")),
    };
    crate::notice::log(&tx, &format!("Engine build {build} withdrawn: {reason}.{leases}")).await?;
    tx.commit().await.map_err(db)?;
    Ok(sorted)
}
