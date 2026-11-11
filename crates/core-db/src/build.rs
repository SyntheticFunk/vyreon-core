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
