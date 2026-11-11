//! Publishing a day: the only write path into the measurement store (C-019). A bundle
//! (`core-contract`) is checked, written, read back and promoted in one transaction, so a day is
//! either published whole or leaves nothing behind.
//!
//! Checked before anything is promoted (started from the engine's `vyreon-store` stage, verify
//! and `publish_revision`):
//!   * the build is registered and the bundle's slots are exactly its inventory for the profile:
//!     none missing, none extra, none twice (the inventory is core's, never the bundle's);
//!   * every body and receipt hashes to the digest the bundle states; a body ends with the
//!     newline the engine writes and is stored without it;
//!   * a body that commits to an event stream has it, and only such a body: the root hashes to
//!     the commitment, the events hash to the root's digest with its length and line count;
//!   * every reader scalar belongs to a slot of the day and is a finite number;
//!   * a revision computed from a base is bound to the day's published base;
//!   * read back from the database: every stored body re-hashes to its digest and equals its
//!     JSONB, every stream decompresses to its digest, and the slot count is the inventory's.
//!
//! Then the day's previous published revision of the profile is superseded and the new one
//! published. Publishing a base revision also supersedes the later days' revisions that were
//! computed from a base (their history windows read this day); the ordered pass derives them again.
use core_contract::{Header, Item, Scalar, Scalars, SlotFiles, StreamRoot, read_bundle, sha256_hex};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use futures_util::TryStreamExt;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use tokio::sync::mpsc::{Receiver, channel};
use tokio_postgres::types::ToSql;
use crate::work::{self, LeaseError};
use tokio_postgres::{Client, Transaction};

/// What a publish stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub revision_id: i64,
    pub represented_date: String,
    pub profile_revision: String,
    pub slots: u64,
    pub streams: u64,
    /// Uncompressed bytes of the event streams read back.
    pub stream_bytes: u64,
    pub reader_scalars: u64,
    pub unusual_scores: u64,
    /// Published revisions this one superseded (the day's previous one, and later computed days).
    pub superseded: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bundle is wrong; sending it again will not help. Nothing was written.
    Refused(String),
    /// Core or the database failed; nothing was written.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Refused(e) => write!(f, "bundle refused: {e}"),
            Error::Failed(e) => write!(f, "publish failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}

fn refuse<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error::Refused(message.into()))
}

/// A database error: data the database will not take (classes 22 and 23) is the bundle's fault.
fn db(e: tokio_postgres::Error) -> Error {
    let message = e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned());
    match e.code().map(|c| &c.code()[..2]) {
        Some("22" | "23") => Error::Refused(message),
        _ => Error::Failed(message),
    }
}

/// A body commits to an event stream when it carries a nonzero product_stream_root_sha256.
fn stream_reference(body: &Value) -> Option<&str> {
    let root = body["product_stream_root_sha256"].as_str()?;
    (root.len() == 64 && root.bytes().all(|b| b.is_ascii_hexdigit()) && root.bytes().any(|b| b != b'0')).then_some(root)
}

/// Publishes the bundle file at `path`.
pub async fn publish_file(client: &mut Client, path: &Path) -> Result<Published, Error> {
    publish(client, path, None).await
}

/// Publishes the bundle a worker sends under its lease: the day must be its job's next session,
/// for the job's profile and build. The day is published and the job moved on in one transaction.
pub async fn publish_for_lease(client: &mut Client, path: &Path, token: &str) -> Result<Published, LeaseError> {
    publish(client, path, Some(token)).await.map_err(|e| match e {
        Error::Refused(m) if m == GONE => LeaseError::Gone,
        Error::Refused(m) => LeaseError::Refused(m),
        Error::Failed(m) => LeaseError::Failed(m),
    })
}

const GONE: &str = "no live lease has this token";

async fn publish(client: &mut Client, path: &Path, token: Option<&str>) -> Result<Published, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Failed(format!("{}: {e}", path.display())))?;
    // the bundle is read on a blocking thread, one slot ahead of the database
    let (sender, mut items) = channel::<Item>(1);
    let reader = tokio::task::spawn_blocking(move || {
        read_bundle(std::io::BufReader::new(file), &mut |item| sender.blocking_send(item).map_err(|_| std::io::Error::other("publish stopped")))
    });
    let result = publish_items(client, &mut items, token).await;
    drop(items);
    match reader.await {
        // a bundle that cannot be read is refused, whatever the publish made of its beginning
        Ok(Err(e)) if e.to_string() != "publish stopped" => refuse(format!("unreadable bundle: {e}")),
        Err(e) => Err(Error::Failed(format!("bundle reader: {e}"))),
        _ => result,
    }
}

async fn publish_items(client: &mut Client, items: &mut Receiver<Item>, token: Option<&str>) -> Result<Published, Error> {
    let Some(Item::Header(header)) = items.recv().await else { return refuse("the bundle has no header") };
    let Some(Item::Scalars(scalars)) = items.recv().await else { return refuse("the bundle has no scalars") };
    let tx = client.transaction().await.map_err(db)?;
    let date = header.represented_date.as_str();
    let lease = match token {
        None => None,
        Some(token) => {
            let lease = match work::held(&tx, token).await {
                Ok(lease) => lease,
                Err(LeaseError::Gone) => return refuse(GONE),
                Err(LeaseError::Refused(m)) => return refuse(m),
                Err(LeaseError::Failed(m)) => return Err(Error::Failed(m)),
            };
            if lease.kind == "TAPE" {
                return refuse("a tape job publishes no days");
            }
            if lease.next_session.as_deref() != Some(date) || lease.engine_build != header.engine_build || lease.profile_id != header.profile_id {
                return refuse(format!(
                    "job {} expects {} of {} from build {}, the bundle is {date} of {} from build {}",
                    lease.job_id,
                    lease.next_session.as_deref().unwrap_or("no further day"),
                    lease.profile_id,
                    lease.engine_build,
                    header.profile_id,
                    header.engine_build
                ));
            }
            Some(lease)
        }
    };
    // one publish at a time per day and profile
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended($1 || ' ' || $2, 0))", &[&date, &header.profile_revision]).await.map_err(db)?;
    let inventory = check_header(&tx, &header).await?;
    let base = bind_base(&tx, &header).await?;
    let revision: i64 = tx
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, base_revision)
             VALUES ($1::text::date, $2, $3, $4, $5::text::timestamptz, $6) RETURNING revision_id",
            &[&date, &header.profile_id, &header.profile_revision, &header.engine_build, &header.snapshot_cutoff, &base],
        )
        .await
        .map_err(db)?
        .get(0);

    let insert_slot = tx
        .prepare(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version,
                 method_version, body_sha256, body_text, body, receipt)
             VALUES ($1, $2, $3, $4::text::date, $5, $6, $7, $8, $9, $9::text::jsonb, $10::text::jsonb)",
        )
        .await
        .map_err(db)?;
    let insert_stream = tx
        .prepare(
            "INSERT INTO vyreon.measurement_stream (revision_id, measurement_id, object_id, represented_date, root_sha256, root_bytes,
                 events_sha256, event_count, byte_length, events_gzip)
             VALUES ($1, $2, $3, $4::text::date, $5, $6, $7, $8, $9, $10)",
        )
        .await
        .map_err(db)?;
    for slot in &header.slots {
        let name = format!("{}/{}", slot.measurement_id, slot.object_id);
        let Some(Item::Slot(files)) = items.recv().await else { return refuse(format!("{name}: the bundle ends before this slot")) };
        let staged = check_slot(slot, &files).map_err(|e| Error::Refused(format!("{name}: {e}")))?;
        tx.execute(
            &insert_slot,
            &[&revision, &slot.measurement_id, &slot.object_id, &date, &slot.slot_state, &slot.science_version, &slot.method_version,
              &slot.body_sha256, &staged.body_text, &staged.receipt_text],
        )
        .await
        .map_err(|e| prefixed(&name, db(e)))?;
        if let Some(stream) = &staged.stream {
            tx.execute(
                &insert_stream,
                &[&revision, &slot.measurement_id, &slot.object_id, &date, &stream.root_sha256, &stream.root_bytes, &stream.events_sha256,
                  &stream.event_count, &stream.byte_length, &stream.events_gzip],
            )
            .await
            .map_err(|e| prefixed(&name, db(e)))?;
        }
    }
    if items.recv().await.is_some() {
        return refuse("the bundle holds more than its header lists");
    }
    let (reader_scalars, unusual_scores) = insert_scalars(&tx, revision, date, &scalars).await?;
    let (streams, stream_bytes) = read_back(&tx, revision, date, inventory.len()).await?;
    let superseded = promote(&tx, revision, &header, base).await?;
    if let Some(lease) = &lease {
        work::session_done(&tx, lease, date).await.map_err(|e| Error::Failed(e.to_string()))?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Published {
        revision_id: revision,
        represented_date: header.represented_date.clone(),
        profile_revision: header.profile_revision.clone(),
        slots: header.slots.len() as u64,
        streams,
        stream_bytes,
        reader_scalars,
        unusual_scores,
        superseded,
    })
}

fn prefixed(name: &str, e: Error) -> Error {
    match e {
        Error::Refused(m) => Error::Refused(format!("{name}: {m}")),
        Error::Failed(m) => Error::Failed(format!("{name}: {m}")),
    }
}

/// The build, its inventory for the profile against the bundle's slots, and the month's partitions.
async fn check_header(tx: &Transaction<'_>, header: &Header) -> Result<HashSet<(String, String)>, Error> {
    let (build, profile) = (&header.engine_build, &header.profile_revision);
    if !core_contract::is_date(&header.represented_date) {
        return refuse(format!("represented_date {} is not a date written YYYY-MM-DD", header.represented_date));
    }
    if tx.query_opt("SELECT FROM core.engine_build WHERE engine_build = $1", &[build]).await.map_err(db)?.is_none() {
        return refuse(format!("engine build {build} is not registered"));
    }
    let inventory: HashSet<(String, String)> = tx
        .query("SELECT measurement_id, object_id FROM core.expected_slot WHERE engine_build = $1 AND profile_revision = $2", &[build, profile])
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    if inventory.is_empty() {
        return refuse(format!("engine build {build} has no slot inventory for profile {profile}"));
    }
    let mut sent = HashSet::new();
    for slot in &header.slots {
        if !sent.insert((slot.measurement_id.clone(), slot.object_id.clone())) {
            return refuse(format!("{}/{} is listed twice", slot.measurement_id, slot.object_id));
        }
    }
    let examples = |set: Vec<&(String, String)>| {
        let mut names: Vec<String> = set.iter().map(|(m, o)| format!("{m}/{o}")).collect();
        names.sort();
        names.truncate(5);
        names.join(", ")
    };
    let (missing, extra): (Vec<_>, Vec<_>) = (inventory.difference(&sent).collect(), sent.difference(&inventory).collect());
    if !missing.is_empty() || !extra.is_empty() {
        let mut parts = Vec::new();
        if !missing.is_empty() {
            parts.push(format!("{} missing ({})", missing.len(), examples(missing)));
        }
        if !extra.is_empty() {
            parts.push(format!("{} not in the inventory ({})", extra.len(), examples(extra)));
        }
        return refuse(format!("the slots are not the inventory of {build} for {profile}: {}", parts.join("; ")));
    }
    let month: Option<bool> = tx
        .query_one("SELECT to_regclass('vyreon.measurement' || to_char($1::text::date, '\"_y\"YYYY\"m\"MM')) IS NOT NULL", &[&header.represented_date])
        .await
        .map_err(db)?
        .get(0);
    if month != Some(true) {
        return Err(Error::Failed(format!("no partitions for the month of {}: vyreon.ensure_months creates them when work is scheduled", header.represented_date)));
    }
    Ok(inventory)
}

/// The day's published base revision, for a bundle computed from one. The share lock holds it
/// published until this transaction ends.
async fn bind_base(tx: &Transaction<'_>, header: &Header) -> Result<Option<i64>, Error> {
    let Some(base_profile) = &header.base_profile_revision else {
        return match header.base_revision {
            None => Ok(None),
            Some(_) => refuse("base_revision without base_profile_revision"),
        };
    };
    if *base_profile == header.profile_revision {
        return refuse("a revision cannot be its own profile's base");
    }
    let published: Option<i64> = tx
        .query_opt(
            "SELECT revision_id FROM vyreon.day_revision
             WHERE represented_date = $1::text::date AND profile_revision = $2 AND state = 'PUBLISHED' FOR SHARE",
            &[&header.represented_date, base_profile],
        )
        .await
        .map_err(db)?
        .map(|r| r.get(0));
    match (published, header.base_revision) {
        (None, _) => refuse(format!("{} has no published {base_profile} revision to build on", header.represented_date)),
        (Some(p), Some(named)) if p != named => refuse(format!("computed from base revision {named}, but the day's published base is now {p}")),
        (Some(p), _) => Ok(Some(p)),
    }
}

struct StagedStream {
    root_sha256: String,
    root_bytes: Vec<u8>,
    events_sha256: String,
    event_count: i64,
    byte_length: i64,
    events_gzip: Vec<u8>,
}

struct Staged {
    body_text: String,
    receipt_text: String,
    stream: Option<StagedStream>,
}

/// One slot's files against what the header states about them.
fn check_slot(slot: &core_contract::Slot, files: &SlotFiles) -> Result<Staged, String> {
    if sha256_hex(&files.body) != slot.body_sha256 {
        return Err("body digest mismatch".into());
    }
    if sha256_hex(&files.receipt) != slot.receipt_sha256 {
        return Err("receipt digest mismatch".into());
    }
    let body_file = std::str::from_utf8(&files.body).map_err(|e| format!("body: {e}"))?;
    let body_text = body_file.strip_suffix('\n').ok_or("body does not end with a newline")?;
    let body: Value = serde_json::from_str(body_text).map_err(|e| format!("body: {e}"))?;
    let receipt_text = std::str::from_utf8(&files.receipt).map_err(|e| format!("receipt: {e}"))?;
    serde_json::from_str::<Value>(receipt_text).map_err(|e| format!("receipt: {e}"))?;
    let stream = match (stream_reference(&body), slot.stream_root_sha256.as_deref(), &files.stream) {
        (None, None, None) => None,
        (Some(committed), Some(listed), Some((root_bytes, events))) => {
            if committed != listed || sha256_hex(root_bytes) != committed {
                return Err("stream root does not match the body's commitment".into());
            }
            let root: StreamRoot = serde_json::from_slice(root_bytes).map_err(|e| format!("stream root: {e}"))?;
            let lines = events.iter().filter(|b| **b == b'\n').count() as i64;
            if sha256_hex(events) != root.events_sha256 || events.len() as i64 != root.byte_length || lines != root.event_count {
                return Err("stream events do not match the stream root".into());
            }
            let mut gz = GzEncoder::new(Vec::with_capacity(events.len() / 8), Compression::default());
            gz.write_all(events).map_err(|e| e.to_string())?;
            Some(StagedStream {
                root_sha256: committed.to_owned(),
                root_bytes: root_bytes.clone(),
                events_sha256: root.events_sha256,
                event_count: root.event_count,
                byte_length: root.byte_length,
                events_gzip: gz.finish().map_err(|e| e.to_string())?,
            })
        }
        (Some(_), _, None) => return Err("body commits to an event stream the bundle does not hold".into()),
        (None, _, Some(_)) | (None, Some(_), None) => return Err("event stream without a body commitment".into()),
        (Some(_), None, Some(_)) => return Err("the header does not list the stream the body commits to".into()),
    };
    Ok(Staged { body_text: body_text.to_owned(), receipt_text: receipt_text.to_owned(), stream })
}

async fn insert_scalars(tx: &Transaction<'_>, revision: i64, date: &str, scalars: &Scalars) -> Result<(u64, u64), Error> {
    let finite = |rows: &[Scalar], what: &str| match rows.iter().find(|s| !s.value.is_finite()) {
        Some(s) => refuse(format!("{what} {}/{}/{} is not a finite number", s.measurement_id, s.object_id, s.role_id)),
        None => Ok(()),
    };
    finite(&scalars.reader_scalar, "reader scalar")?;
    finite(&scalars.unusual_score, "unusual score")?;
    let text = |rows: &[Scalar]| serde_json::to_string(rows).map_err(|e| Error::Failed(e.to_string()));
    let reader = tx
        .execute(
            "INSERT INTO vyreon.reader_scalar (revision_id, object_id, measurement_id, role_id, represented_date, value)
             SELECT $1, object_id, measurement_id, role_id, $2::text::date, value
             FROM jsonb_to_recordset($3::text::jsonb) AS s(object_id text, measurement_id text, role_id text, value double precision)",
            &[&revision, &date, &text(&scalars.reader_scalar)?],
        )
        .await
        .map_err(db)?;
    let unusual = tx
        .execute(
            "INSERT INTO vyreon.unusual_score (revision_id, object_id, measurement_id, role_id, represented_date, z)
             SELECT $1, object_id, measurement_id, role_id, $2::text::date, value
             FROM jsonb_to_recordset($3::text::jsonb) AS s(object_id text, measurement_id text, role_id text, value double precision)",
            &[&revision, &date, &text(&scalars.unusual_score)?],
        )
        .await
        .map_err(db)?;
    let stray: Option<(String, String)> = tx
        .query_opt(
            "SELECT s.measurement_id, s.object_id FROM vyreon.reader_scalar s
             WHERE s.revision_id = $1 AND s.represented_date = $2::text::date
               AND NOT EXISTS (SELECT FROM vyreon.measurement m WHERE m.revision_id = s.revision_id AND m.represented_date = s.represented_date
                                 AND m.measurement_id = s.measurement_id AND m.object_id = s.object_id)
             LIMIT 1",
            &[&revision, &date],
        )
        .await
        .map_err(db)?
        .map(|r| (r.get(0), r.get(1)));
    if let Some((m, o)) = stray {
        return refuse(format!("a reader scalar names {m}/{o}, which is not a slot of the day"));
    }
    Ok((reader, unusual))
}

/// Reads the staged revision back from the database and checks it as a reader would find it.
/// Returns (streams, their uncompressed bytes).
async fn read_back(tx: &Transaction<'_>, revision: i64, date: &str, expected: usize) -> Result<(u64, u64), Error> {
    let broken = |what: String| Error::Failed(format!("readback: {what}"));
    let params: [&(dyn ToSql + Sync); 2] = [&revision, &date];
    let mut committed: HashMap<(String, String), String> = HashMap::new();
    let mut slots = 0usize;
    {
        let rows = tx
            .query_raw(
                "SELECT measurement_id, object_id, body_sha256, body_text, body_text::jsonb = body
                 FROM vyreon.measurement WHERE revision_id = $1 AND represented_date = $2::text::date",
                params,
            )
            .await
            .map_err(db)?;
        let mut rows = std::pin::pin!(rows);
        while let Some(row) = rows.try_next().await.map_err(db)? {
            let (m, o, sha, text, same): (String, String, String, String, bool) = (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4));
            if sha256_hex(format!("{text}\n").as_bytes()) != sha {
                return Err(broken(format!("{m}/{o}: the stored body does not hash to its digest")));
            }
            if !same {
                return Err(broken(format!("{m}/{o}: the JSONB does not equal the stored body text")));
            }
            let body: Value = serde_json::from_str(&text).map_err(|e| broken(format!("{m}/{o}: {e}")))?;
            if let Some(root) = stream_reference(&body) {
                committed.insert((m, o), root.to_owned());
            }
            slots += 1;
        }
    }
    if slots != expected {
        return Err(broken(format!("{slots} of {expected} slots readable")));
    }
    let (mut streams, mut stream_bytes) = (0u64, 0u64);
    let rows = tx
        .query_raw(
            "SELECT measurement_id, object_id, root_sha256, root_bytes, events_sha256, byte_length, events_gzip
             FROM vyreon.measurement_stream WHERE revision_id = $1 AND represented_date = $2::text::date",
            params,
        )
        .await
        .map_err(db)?;
    let mut rows = std::pin::pin!(rows);
    while let Some(row) = rows.try_next().await.map_err(db)? {
        let (m, o, root_sha, root_bytes, events_sha, length, gz): (String, String, String, Vec<u8>, String, i64, Vec<u8>) =
            (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4), row.get(5), row.get(6));
        if committed.remove(&(m.clone(), o.clone())).as_deref() != Some(root_sha.as_str()) || sha256_hex(&root_bytes) != root_sha {
            return Err(broken(format!("{m}/{o}: the stored stream root does not match the body's commitment")));
        }
        let mut events = Vec::with_capacity(length.max(0) as usize);
        GzDecoder::new(gz.as_slice()).read_to_end(&mut events).map_err(|e| broken(format!("{m}/{o}: {e}")))?;
        if sha256_hex(&events) != events_sha || events.len() as i64 != length {
            return Err(broken(format!("{m}/{o}: the stored stream does not hash to its digest")));
        }
        streams += 1;
        stream_bytes += events.len() as u64;
    }
    if let Some((m, o)) = committed.keys().next() {
        return Err(broken(format!("{m}/{o}: committed event stream missing")));
    }
    Ok((streams, stream_bytes))
}

/// Supersedes what the new revision replaces and publishes it. Returns how many it superseded.
async fn promote(tx: &Transaction<'_>, revision: i64, header: &Header, base: Option<i64>) -> Result<u64, Error> {
    let date = header.represented_date.as_str();
    let mut superseded = tx
        .execute(
            "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
             WHERE represented_date = $1::text::date AND profile_revision = $2 AND state = 'PUBLISHED'",
            &[&date, &header.profile_revision],
        )
        .await
        .map_err(db)?;
    if base.is_none() {
        // this day is in the history windows of every later computed day
        superseded += tx
            .execute(
                "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
                 WHERE represented_date >= $1::text::date AND state = 'PUBLISHED' AND base_revision IS NOT NULL AND base_revision <> $2",
                &[&date, &revision],
            )
            .await
            .map_err(db)?;
    }
    tx.execute("UPDATE vyreon.day_revision SET state = 'PUBLISHED', published_at = now() WHERE revision_id = $1", &[&revision]).await.map_err(db)?;
    Ok(superseded)
}
