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
    crate::notice::post(client, &format!("Engine build {build} admitted: its workers may claim work.")).await
}

/// Withdraws an admitted build: its queued jobs are no longer claimable.
pub async fn withdraw(client: &Client, build: &str, reason: &str) -> Result<(), String> {
    let n = client
        .execute("UPDATE core.engine_build SET withdrawn_at = now(), withdrawal = $2 WHERE engine_build = $1 AND admitted_at IS NOT NULL", &[&build, &reason])
        .await
        .map_err(db)?;
    if n == 0 {
        return Err(format!("engine build {build} is not admitted"));
    }
    crate::notice::post(client, &format!("Engine build {build} withdrawn: {reason}.")).await
}
