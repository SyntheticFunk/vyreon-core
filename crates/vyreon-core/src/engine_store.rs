//! Reading a store in the engine's own schema (vyreon-engine `db/schema.sql`, engine-v1): the
//! droplet's. It turns a published day into bundles and its builds into inventory documents, so
//! days the engine published itself can go through core's publish path. Read-only. It goes when
//! the droplet's store does.
use core_contract::{BundleWriter, CONTRACT, Header, Scalar, Scalars, Slot, SlotFiles};
use flate2::read::GzDecoder;
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

async fn scalars(store: &Client, table: &str, column: &str, revision: i64) -> Result<Vec<Scalar>, String> {
    let rows = store
        .query(&format!("SELECT object_id, measurement_id, role_id, {column} FROM vyreon.{table} WHERE revision_id = $1 ORDER BY 1, 2, 3"), &[&revision])
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| Scalar { object_id: r.get(0), measurement_id: r.get(1), role_id: r.get(2), value: r.get(3) }).collect())
}

/// Writes one bundle per published revision of `date` into `dir`, and one inventory document per
/// engine build that published them. Returns the files written, bundles in publish order.
pub async fn export_day(store: &Client, date: &str, dir: &Path) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let revisions = store
        .query(
            "SELECT d.revision_id, d.profile_id, d.profile_revision, d.engine_build,
                    to_char(d.snapshot_cutoff AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    (SELECT b.profile_revision FROM vyreon.day_revision b WHERE b.revision_id = d.base_revision)
             FROM vyreon.day_revision d
             WHERE d.represented_date = $1::text::date AND d.state = 'PUBLISHED'
             ORDER BY d.base_revision NULLS FIRST, d.revision_id",
            &[&date],
        )
        .await
        .map_err(db)?;
    if revisions.is_empty() {
        return Err(format!("the store has no published revision of {date}"));
    }
    let mut written = Vec::new();
    for r in &revisions {
        let (revision, profile_id, profile, build, cutoff, base_profile): (i64, String, String, String, String, Option<String>) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        let slots: Vec<Slot> = store
            .query(
                "SELECT m.measurement_id, m.object_id, m.slot_state, m.science_version, m.method_version, m.body_sha256,
                        encode(sha256(convert_to(m.receipt::text, 'UTF8')), 'hex'), s.root_sha256
                 FROM vyreon.measurement m
                 JOIN vyreon.expected_slot e ON e.profile_revision = $2 AND e.measurement_id = m.measurement_id AND e.object_id = m.object_id
                 LEFT JOIN vyreon.measurement_stream s
                      ON s.revision_id = m.revision_id AND s.measurement_id = m.measurement_id AND s.object_id = m.object_id
                 WHERE m.revision_id = $1 ORDER BY e.slot_ordinal",
                &[&revision, &profile],
            )
            .await
            .map_err(db)?
            .iter()
            .map(|s| Slot {
                measurement_id: s.get(0),
                object_id: s.get(1),
                slot_state: s.get(2),
                science_version: s.get(3),
                method_version: s.get(4),
                body_sha256: s.get(5),
                receipt_sha256: s.get(6),
                stream_root_sha256: s.get(7),
                coordinates_sha256: None,
            })
            .collect();
        let header = Header {
            contract: CONTRACT.to_owned(),
            represented_date: date.to_owned(),
            profile_id,
            profile_revision: profile.clone(),
            engine_build: build.clone(),
            snapshot_cutoff: cutoff,
            base_profile_revision: base_profile,
            base_revision: None,
            report: None,
            slots,
        };
        let all = Scalars {
            reader_scalar: scalars(store, "reader_scalar", "value", revision).await?,
            unusual_score: scalars(store, "unusual_score", "z", revision).await?,
        };
        let name = format!("{date}.{profile}.bundle.tar.gz");
        let path = dir.join(&name);
        let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut bundle = BundleWriter::new(std::io::BufWriter::new(file), &header, &all).map_err(|e| e.to_string())?;
        let params: [&(dyn ToSql + Sync); 2] = [&revision, &profile];
        let rows = store
            .query_raw(
                "SELECT m.body_text, m.receipt::text, s.root_bytes, s.events_gzip
                 FROM vyreon.measurement m
                 JOIN vyreon.expected_slot e ON e.profile_revision = $2 AND e.measurement_id = m.measurement_id AND e.object_id = m.object_id
                 LEFT JOIN vyreon.measurement_stream s
                      ON s.revision_id = m.revision_id AND s.measurement_id = m.measurement_id AND s.object_id = m.object_id
                 WHERE m.revision_id = $1 ORDER BY e.slot_ordinal",
                params,
            )
            .await
            .map_err(db)?;
        let mut rows = std::pin::pin!(rows);
        while let Some(row) = rows.try_next().await.map_err(db)? {
            let (body_text, receipt, root, gz): (String, String, Option<Vec<u8>>, Option<Vec<u8>>) = (row.get(0), row.get(1), row.get(2), row.get(3));
            let stream = match (root, gz) {
                (Some(root), Some(gz)) => {
                    let mut events = Vec::new();
                    GzDecoder::new(gz.as_slice()).read_to_end(&mut events).map_err(|e| e.to_string())?;
                    Some((root, events))
                }
                _ => None,
            };
            // the engine hashes the body file: the stored text and its final newline
            bundle.add(&SlotFiles { body: format!("{body_text}\n").into_bytes(), receipt: receipt.into_bytes(), coordinates: None, stream }).map_err(|e| e.to_string())?;
        }
        bundle.finish().map_err(|e| e.to_string())?;
        written.push(name);

        // the build's inventory: every profile it has published in this store
        let inventory = store
            .query(
                "SELECT e.profile_revision, e.slot_ordinal, e.measurement_id, e.object_id, e.layer
                 FROM vyreon.expected_slot e
                 WHERE e.profile_revision IN (SELECT profile_revision FROM vyreon.day_revision WHERE engine_build = $1 AND state IN ('PUBLISHED', 'SUPERSEDED'))
                 ORDER BY 1, 2",
                &[&build],
            )
            .await
            .map_err(db)?;
        let slots: Vec<Value> = inventory
            .iter()
            .map(|s| {
                json!({"profile_revision": s.get::<_, String>(0), "slot_ordinal": s.get::<_, i32>(1), "measurement_id": s.get::<_, String>(2),
                       "object_id": s.get::<_, String>(3), "layer": s.get::<_, String>(4)})
            })
            .collect();
        let name = format!("build.{build}.inventory.json");
        if !written.contains(&name) {
            let text = json!({"engine_build": build, "slots": slots}).to_string();
            std::fs::write(dir.join(&name), text).map_err(|e| format!("{name}: {e}"))?;
            written.push(name);
        }
    }
    // what to register and publish, in order
    let (inventories, bundles): (Vec<&String>, Vec<&String>) = written.iter().partition(|f| f.ends_with(".inventory.json"));
    let name = format!("{date}.export.json");
    std::fs::write(dir.join(&name), json!({"represented_date": date, "inventories": inventories, "bundles": bundles}).to_string()).map_err(|e| format!("{name}: {e}"))?;
    written.push(name);
    Ok(written)
}
