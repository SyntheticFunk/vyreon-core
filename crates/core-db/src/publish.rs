//! Publishing a day: the only write path into the measurement store (C-019). A bundle
//! (`core-contract`) is checked, written, read back and promoted in one transaction, so a day is
//! either published whole or leaves nothing behind.
//!
//! Checked before anything is promoted (started from the engine's `vyreon-store` stage, verify
//! and `publish_revision`):
//!   * the day is a session of the calendar core holds;
//!   * the members the bundle states as listed on the day are the ones core's universe lists
//!     (`core.members_on`, a ticker that day): the worker and core agree on membership (C-036);
//!   * the build is registered and the bundle's slots are exactly its inventory for the profile,
//!     narrowed to the field object and the members listed that day: none missing, none extra,
//!     none twice (the inventory and the universe are core's, never the bundle's);
//!   * every body and receipt hashes to the digest the bundle states; a body ends with the
//!     newline the engine writes and is stored without it;
//!   * a body that commits to an event stream has it, and only such a body: the root hashes to
//!     the commitment, the events hash to the root's digest with its length and line count;
//!   * every reader scalar belongs to a slot of the day and is a finite number;
//!   * a revision computed from a base is bound to the day's published base;
//!   * read back from the database: every stored body re-hashes to its digest and equals its
//!     JSONB, every stream decompresses to its digest, and the slot count is what the inventory
//!     expects of the listed members recorded with the revision.
//!
//! Then the day's previous published revision of the profile is superseded and the new one
//! published, with what read what it replaces (`promote`): the day's own revisions computed from a
//! replaced base or reading replaced day-local values, and every later day's revisions that read
//! history. The ordered pass derives those again: its jobs are queued again in the same
//! transaction, and a pass running on an affected day is stopped.
use core_contract::{FIELD_OBJECT, Header, Item, LISTED_MEMBERS, Scalar, Scalars, SlotFiles, StreamRoot, read_bundle, sha256_hex};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use futures_util::TryStreamExt;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use tokio::sync::mpsc::{Receiver, channel};
use tokio_postgres::types::ToSql;
use crate::work::{self, LeaseError};
use tokio_postgres::{Client, GenericClient, Transaction};

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
    /// True when a worker sent again a day its job had already published (the answer to the
    /// first sending was lost): nothing was written, and the counts other than `slots` are zero.
    pub already_published: bool,
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

/// A database error: data the database will not take (classes 22 and 23) is the bundle's fault,
/// except a constraint of core's own tables (the queue's, the notices'): the bundle writes only the
/// store, so such a violation is core's, never a refused bundle that pauses the queue (audit
/// 2026-11-22 changes-core 1: a pass queued meanwhile hit `job_one_unfinished_pass`).
fn db(e: tokio_postgres::Error) -> Error {
    let message = e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned());
    let core_table = e.as_db_error().is_some_and(|d| d.schema() == Some("core"));
    match e.code().map(|c| &c.code()[..2]) {
        Some("23") if core_table => Error::Failed(message),
        Some("22" | "23") => Error::Refused(message),
        _ => Error::Failed(message),
    }
}

/// A body commits to an event stream when it carries a nonzero product_stream_root_sha256.
fn stream_reference(body: &Value) -> Option<&str> {
    let root = body["product_stream_root_sha256"].as_str()?;
    (root.len() == 64 && root.bytes().all(|b| b.is_ascii_hexdigit()) && root.bytes().any(|b| b != b'0')).then_some(root)
}

/// Who sends a bundle: a worker under its lease, or a hand without one.
#[derive(Clone, Copy)]
enum By<'a> {
    Lease(&'a str),
    Hand { vouched: bool },
}

/// Publishes the bundle file at `path` by hand, without a lease. With no lease core cannot tell
/// what a revision computed from a base read (C-042): one that reads history is refused (a day it
/// read may have been replaced since; the ordered pass publishes those), and one that reads its
/// day only must name the base revision it was computed from.
pub async fn publish_file(client: &mut Client, path: &Path) -> Result<Published, Error> {
    publish(client, path, By::Hand { vouched: false }).await
}

/// The same, with the hand's word that the bundle was computed from what is published now: its
/// day's base and, if it reads history, every earlier day (C-042). Core does not check it.
pub async fn publish_file_vouched(client: &mut Client, path: &Path) -> Result<Published, Error> {
    publish(client, path, By::Hand { vouched: true }).await
}

/// Publishes the bundle a worker sends under its lease: the day must be its job's next session,
/// for the job's profile and build. The day is published and the job moved on in one transaction.
///
/// A refusal is a failed check of live work (the service pauses the queue on it) only while the lease
/// is live: some refusals come before the lease is checked (a bundle that ends before its scalars, or
/// cannot be read to its end), so the lease is checked again first, and a refusal from a lease that has
/// ended meanwhile (withdrawn, cancelled, expired) is answered as the lease gone (audit 2026-11-22
/// round 2, core-r2 6: a withdrawn worker's truncated upload paused the replacement build's queue).
pub async fn publish_for_lease(client: &mut Client, path: &Path, token: &str) -> Result<Published, LeaseError> {
    match publish(client, path, By::Lease(token)).await {
        Ok(published) => Ok(published),
        Err(Error::Refused(m)) if m == GONE => Err(LeaseError::Gone),
        Err(Error::Refused(m)) => match work::identify(&*client, token).await {
            Err(LeaseError::Gone) => Err(LeaseError::Gone),
            _ => Err(LeaseError::Refused(m)),
        },
        Err(Error::Failed(m)) => Err(LeaseError::Failed(m)),
    }
}

const GONE: &str = "no live lease has this token";

async fn publish(client: &mut Client, path: &Path, by: By<'_>) -> Result<Published, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Failed(format!("{}: {e}", path.display())))?;
    // the bundle is read on a blocking thread, one slot ahead of the database; its end is sent as
    // `None` once the whole bundle has been read, so a channel closed without it (the reader
    // failed, perhaps after the last slot) is never taken for a complete bundle
    let (sender, mut items) = channel::<Option<Item>>(1);
    let reader = tokio::task::spawn_blocking(move || {
        let stopped = |_| std::io::Error::other("publish stopped");
        read_bundle(std::io::BufReader::new(file), &mut |item| sender.blocking_send(Some(item)).map_err(stopped))?;
        sender.blocking_send(None).map_err(stopped)
    });
    let result = publish_items(client, &mut items, by).await;
    drop(items);
    match reader.await {
        // a bundle that cannot be read is refused, whatever the publish made of its beginning
        Ok(Err(e)) if e.to_string() != "publish stopped" => refuse(format!("unreadable bundle: {e}")),
        Err(e) => Err(Error::Failed(format!("bundle reader: {e}"))),
        _ => result,
    }
}

async fn publish_items(client: &mut Client, items: &mut Receiver<Option<Item>>, by: By<'_>) -> Result<Published, Error> {
    let Some(Some(Item::Header(header))) = items.recv().await else { return refuse("the bundle has no header") };
    let Some(Some(Item::Scalars(scalars))) = items.recv().await else { return refuse("the bundle has no scalars") };
    let tx = client.transaction().await.map_err(db)?;
    // one publish at a time, taken before any row lock: a publish supersedes and queues again what
    // read its day, and an ordered pass is claimed under the same lock (`work::PUBLISH_LOCK`)
    tx.execute(work::PUBLISH_LOCK, &[]).await.map_err(db)?;
    let date = header.represented_date.as_str();
    let lease = match by {
        // no lease says what the bundle read, so the checks a lease allows cannot be made (C-042)
        By::Hand { vouched } => {
            if header.base_profile_revision.is_some() && !vouched {
                if !header.day_local {
                    return refuse(
                        "a revision that reads history is published by the ordered pass: by hand, core cannot tell whether a day it read \
                         has been replaced since (`vyreon-core publish --vouch` states that none has)",
                    );
                }
                if header.base_revision.is_none() {
                    return refuse(
                        "published by hand, a revision computed from a base names the base revision it read (base_revision), \
                         or `vyreon-core publish --vouch` states that it is the day's published one",
                    );
                }
            }
            None
        }
        By::Lease(token) => {
            let lease = match work::held(&tx, token).await {
                Ok(lease) => lease,
                Err(LeaseError::Gone) => return refuse(GONE),
                Err(LeaseError::Refused(m)) => return refuse(m),
                Err(LeaseError::Failed(m)) => return Err(Error::Failed(m)),
            };
            if lease.kind == "TAPE" {
                return refuse("a tape job publishes no days");
            }
            // C-073: a re-run judges what is published; it never publishes
            if lease.kind == "REFCHECK" {
                return refuse("a reference-check re-run publishes nothing");
            }
            // a day block's current day stays its next until the worker says it is done, so a
            // bundle of it sent again is answered like one of an earlier day
            if lease.engine_build == header.engine_build
                && lease.profile_id == header.profile_id
                && (lease.kind == "DAY_BLOCK" || lease.next_session.as_deref() != Some(date))
                && let Some(revision_id) = published_before(&tx, &header, &lease).await?
            {
                // C-073 (audit c072 r2, pre-existing): acknowledged only once the bundle is what was
                // published, every file and number of it; one that differs is refused, never answered
                // `already_published` while the reader keeps serving the first
                same_as_published(&tx, revision_id, &header, &scalars, items).await?;
                return Ok(Published {
                    revision_id,
                    represented_date: header.represented_date.clone(),
                    profile_revision: header.profile_revision.clone(),
                    slots: header.slots.len() as u64,
                    streams: 0,
                    stream_bytes: 0,
                    reader_scalars: 0,
                    unusual_scores: 0,
                    superseded: 0,
                    already_published: true,
                });
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
            // a day block publishes a day's base and what reads that day alone; the ordered pass
            // publishes what reads history
            let based = header.base_profile_revision.is_some();
            if lease.kind == "DAY_BLOCK" && based && !header.day_local {
                return refuse("a day block publishes no revision that reads history: that is the ordered pass's");
            }
            if lease.kind == "ORDERED_PASS" && (!based || header.day_local) {
                return refuse("the ordered pass publishes only a revision computed from the day's base that reads history");
            }
            if lease.kind == "ORDERED_PASS" {
                // the pass read the days up to its own after its lease was granted: a day of them
                // published since may have changed under it (a publish normally stops such a pass
                // first, ending its lease; this is the check that it did)
                let changed: Option<(String, i64)> = tx
                    .query_opt(
                        "SELECT d.represented_date::text, d.revision_id FROM vyreon.day_revision d, core.lease l
                         WHERE l.lease_id = $1 AND d.represented_date <= $2::text::date AND d.published_at > l.granted_at
                         ORDER BY d.published_at DESC LIMIT 1",
                        &[&lease.lease_id, &date],
                    )
                    .await
                    .map_err(db)?
                    .map(|r| (r.get(0), r.get(1)));
                if let Some((day, revision)) = changed {
                    return refuse(format!(
                        "revision {revision} of {day} was published after job {}'s lease was granted: the pass may have read what it replaced",
                        lease.job_id
                    ));
                }
            }
            Some(lease)
        }
    };
    let listed = check_header(&tx, &header).await?;
    check_layer(&tx, &header, lease.as_ref()).await?;
    if header.report.as_ref().is_some_and(|r| !r.is_object()) {
        return refuse("the report is not a JSON object");
    }
    let report = header.report.as_ref().map(Value::to_string);
    let base = bind_base(&tx, &header).await?;
    let revision: i64 = tx
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, base_revision, report, day_local,
                 listed_members)
             VALUES ($1::text::date, $2, $3, $4, $5::text::timestamptz, $6, $7::text::jsonb, $8, $9) RETURNING revision_id",
            &[&date, &header.profile_id, &header.profile_revision, &header.engine_build, &header.snapshot_cutoff, &base, &report, &header.day_local, &listed],
        )
        .await
        .map_err(db)?
        .get(0);
    // the lease it is published under, if any (audit round 2 of core-watch, 11: what published a
    // day is recorded, never inferred from overlapping dates and times)
    if let Some(lease) = &lease {
        tx.execute("INSERT INTO core.lease_revision (revision_id, lease_id) VALUES ($1, $2)", &[&revision, &lease.lease_id]).await.map_err(db)?;
    }

    let insert_slot = tx
        .prepare(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version,
                 method_version, body_sha256, body_text, body, receipt, coordinates)
             VALUES ($1, $2, $3, $4::text::date, $5, $6, $7, $8, $9, $9::text::jsonb, $10::text::jsonb, $11::text::jsonb)",
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
        let Some(Some(Item::Slot(files))) = items.recv().await else { return refuse(format!("{name}: the bundle ends before this slot")) };
        let staged = check_slot(slot, &files).map_err(|e| Error::Refused(format!("{name}: {e}")))?;
        tx.execute(
            &insert_slot,
            &[&revision, &slot.measurement_id, &slot.object_id, &date, &slot.slot_state, &slot.science_version, &slot.method_version,
              &slot.body_sha256, &staged.body_text, &staged.receipt_text, &staged.coordinates_text],
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
    // nothing is written unless the reader says the bundle ended where its header says (C-024)
    match items.recv().await {
        Some(None) => {}
        Some(Some(_)) => return refuse("the bundle holds more than its header lists"),
        None => return refuse("the bundle could not be read to its end"),
    }
    let (reader_scalars, unusual_scores) = insert_scalars(&tx, revision, date, &scalars).await?;
    let (streams, stream_bytes) = read_back(&tx, revision, date).await?;
    let superseded = promote(&tx, revision, &header, base, lease.as_ref()).await?;
    // an ordered pass's day is its one bundle; a day block's is every bundle of the day (the base,
    // then what was computed on it: C-031), so its worker says when the day is done (`work::day_done`)
    if let Some(lease) = lease.as_ref().filter(|l| l.kind != "DAY_BLOCK") {
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
        already_published: false,
    })
}

/// The published revision of the bundle's day, when the lease's job has already published that
/// day with this very bundle: an earlier session of the job, never one before its first (C-070:
/// another job's day was answered as this one's), or a day block's current one; the
/// same profile and build, the same listed members, each slot once, and every slot's body digest
/// equal. A worker sends a day again when the answer to its first sending was lost; that is
/// answered, not refused. Anything else goes on to the full checks.
async fn published_before(tx: &Transaction<'_>, header: &Header, lease: &work::Held) -> Result<Option<i64>, Error> {
    let date = header.represented_date.as_str();
    let current = lease.kind == "DAY_BLOCK" && lease.next_session.as_deref() == Some(date);
    if !core_contract::is_date(date)
        || date < lease.first_session.as_str()
        || date > lease.last_session.as_str()
        || (!current && lease.next_session.as_deref().is_some_and(|next| date >= next))
    {
        return Ok(None);
    }
    // a slot sent twice would count twice below (audit 2026-11-14, core.md 15)
    let distinct: HashSet<(&str, &str)> = header.slots.iter().map(|s| (s.measurement_id.as_str(), s.object_id.as_str())).collect();
    let Some(listed) = &header.listed_members else { return Ok(None) };
    if distinct.len() != header.slots.len() {
        return Ok(None);
    }
    let slots: Vec<Value> = header.slots.iter().map(|s| serde_json::json!({"measurement_id": s.measurement_id, "object_id": s.object_id, "body_sha256": s.body_sha256})).collect();
    let row = tx
        .query_opt(
            "SELECT d.revision_id,
                    (SELECT count(*) FROM vyreon.measurement m WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date),
                    (SELECT count(*) FROM vyreon.measurement m
                         JOIN jsonb_to_recordset($4::text::jsonb) AS s(measurement_id text, object_id text, body_sha256 text)
                         USING (measurement_id, object_id, body_sha256)
                     WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date)
             FROM vyreon.day_revision d
             WHERE d.represented_date = $1::text::date AND d.profile_revision = $2 AND d.engine_build = $3 AND d.state = 'PUBLISHED'
               AND d.listed_members = $5::text[]",
            &[&date, &header.profile_revision, &header.engine_build, &Value::Array(slots).to_string(), listed],
        )
        .await
        .map_err(db)?;
    let n = header.slots.len() as i64;
    Ok(row.filter(|r| r.get::<_, i64>(1) == n && r.get::<_, i64>(2) == n).map(|r| r.get(0)))
}

/// C-073 (audit c072 r2, pre-existing): a bundle sent again is acknowledged as published only when it
/// is that revision: every slot's files as its header states them (`check_slot`) and its body digests
/// the stored ones (`published_before`), the bundle ending where its header says, and its reader
/// scalars and unusual scores exactly the stored ones. Anything else is refused, naming what differs.
async fn same_as_published(tx: &Transaction<'_>, revision: i64, header: &Header, scalars: &Scalars, items: &mut Receiver<Option<Item>>) -> Result<(), Error> {
    // release audit (Codex 12, Gemini 13): every field of each slot is the stored one, not its body alone
    let mut sent = Vec::with_capacity(header.slots.len());
    for slot in &header.slots {
        let name = format!("{}/{}", slot.measurement_id, slot.object_id);
        let Some(Some(Item::Slot(files))) = items.recv().await else { return refuse(format!("{name}: the bundle ends before this slot")) };
        let staged = check_slot(slot, &files).map_err(|e| Error::Refused(format!("{name}: {e}")))?;
        sent.push(serde_json::json!({
            "measurement_id": slot.measurement_id, "object_id": slot.object_id, "slot_state": slot.slot_state,
            "science_version": slot.science_version, "method_version": slot.method_version, "receipt": staged.receipt_text,
            "coordinates": staged.coordinates_text, "root_sha256": staged.stream.as_ref().map(|x| x.root_sha256.clone()),
            "events_sha256": staged.stream.as_ref().map(|x| x.events_sha256.clone()),
        }));
    }
    let differs: Option<String> = tx
        .query_opt(
            "SELECT s.measurement_id || '/' || s.object_id FROM jsonb_to_recordset($2::text::jsonb)
                 AS s(measurement_id text, object_id text, slot_state text, science_version text, method_version text, receipt text, coordinates text,
                      root_sha256 text, events_sha256 text)
             LEFT JOIN vyreon.measurement m ON m.revision_id = $1 AND m.measurement_id = s.measurement_id AND m.object_id = s.object_id
             LEFT JOIN vyreon.measurement_stream t ON t.revision_id = $1 AND t.measurement_id = s.measurement_id AND t.object_id = s.object_id
             WHERE m.revision_id IS NULL OR m.slot_state <> s.slot_state OR m.science_version <> s.science_version OR m.method_version <> s.method_version
                OR m.receipt <> s.receipt::jsonb OR m.coordinates IS DISTINCT FROM s.coordinates::jsonb
                OR t.root_sha256 IS DISTINCT FROM s.root_sha256 OR t.events_sha256 IS DISTINCT FROM s.events_sha256
             LIMIT 1",
            &[&revision, &Value::Array(sent).to_string()],
        )
        .await
        .map_err(db)?
        .map(|r| r.get(0));
    if let Some(slot) = differs {
        return refuse(format!(
            "the bundle sent again for {} is not published revision {revision}: slot {slot} differs (its state, versions, receipt, coordinates or stream)",
            header.represented_date
        ));
    }
    let head: (bool, bool) = tx
        .query_one(
            "SELECT snapshot_cutoff = $2::text::timestamptz, day_local = $3 AND ($4::bigint IS NULL OR base_revision = $4) FROM vyreon.day_revision WHERE revision_id = $1",
            &[&revision, &header.snapshot_cutoff, &header.day_local, &header.base_revision],
        )
        .await
        .map(|r| (r.get(0), r.get(1)))
        .map_err(db)?;
    if head != (true, true) {
        return refuse(format!(
            "the bundle sent again for {} is not published revision {revision}: its snapshot cutoff, layer or base differs",
            header.represented_date
        ));
    }
    match items.recv().await {
        Some(None) => {}
        Some(Some(_)) => return refuse("the bundle holds more than its header lists"),
        None => return refuse("the bundle could not be read to its end"),
    }
    for (table, column, sent, what) in [("reader_scalar", "value", &scalars.reader_scalar, "reader scalars"), ("unusual_score", "z", &scalars.unusual_score, "unusual scores")] {
        let stored: HashMap<(String, String, String), u64> = tx
            .query(&format!("SELECT object_id, measurement_id, role_id, {column} FROM vyreon.{table} WHERE revision_id = $1"), &[&revision])
            .await
            .map_err(db)?
            .iter()
            .map(|r| ((r.get(0), r.get(1), r.get(2)), canonical(r.get::<_, f64>(3))))
            .collect();
        // release audit (Gemini 11): the store keeps numbers through jsonb, where -0 is 0; the
        // comparison is of the values, every other bit exact
        let sent_set: HashMap<(String, String, String), u64> =
            sent.iter().map(|x| ((x.object_id.clone(), x.measurement_id.clone(), x.role_id.clone()), canonical(x.value))).collect();
        if sent_set.len() != sent.len() || sent_set != stored {
            let differs = sent_set
                .iter()
                .find(|(k, v)| stored.get(*k) != Some(*v))
                .map(|(k, _)| k)
                .or_else(|| stored.keys().find(|k| !sent_set.contains_key(*k)))
                .map_or_else(|| "a value sent twice".to_owned(), |(o, m, r)| format!("{m}/{o}/{r}"));
            return refuse(format!(
                "the bundle sent again for {} is not published revision {revision}: its {what} differ ({differs}); a day is published once with one set of numbers",
                header.represented_date
            ));
        }
    }
    Ok(())
}

/// A value's bits with -0 as 0 (the store's jsonb path keeps no negative zero).
fn canonical(v: f64) -> u64 {
    if v == 0.0 { 0 } else { v.to_bits() }
}

/// A revision's layer: the base, the day-local revision on it, or the history revision (C-031,
/// C-070).
fn layer(header: &Header) -> &'static str {
    match (header.base_profile_revision.is_some(), header.day_local) {
        (false, _) => "base",
        (true, true) => "day-local",
        (true, false) => "history",
    }
}

/// C-073 (audit c072 r2, pre-existing): a bundle's layer is what its header says (a day-local bundle
/// that omitted its base fields and `day_local` read as a base, and its promotion superseded the
/// real base). Two checks the header cannot escape: a profile revision of a build has one layer (any
/// revision of it stored before, in any state, fixes it), and a day block's day has one base (a
/// second base of another profile revision on the same lease and day is refused).
///
/// Under a lease only: by hand an operator may publish an engine's exported bundles of a profile
/// that predates the split into layers (C-031; `publish --vouch`), and says what it is.
async fn check_layer(tx: &Transaction<'_>, header: &Header, lease: Option<&work::Held>) -> Result<(), Error> {
    let Some(lease) = lease else { return Ok(()) };
    let this = layer(header);
    let before: Option<String> = tx
        .query_opt(
            "SELECT CASE WHEN base_revision IS NULL THEN 'base' WHEN day_local THEN 'day-local' ELSE 'history' END FROM vyreon.day_revision
             WHERE engine_build = $1 AND profile_revision = $2 AND state <> 'REJECTED' ORDER BY revision_id LIMIT 1",
            &[&header.engine_build, &header.profile_revision],
        )
        .await
        .map_err(db)?
        .map(|r| r.get(0));
    if let Some(before) = before.filter(|b| b != this) {
        return refuse(format!(
            "profile {} of build {} is published as a {before} revision; this bundle is presented as a {this} revision (its base fields and day_local say what it is)",
            header.profile_revision, header.engine_build
        ));
    }
    // release audit (Codex 11): a lease publishes a day's revision of one profile once; one it already
    // published that `published_before` did not match is a different bundle, refused (it would
    // supersede the first, and what read it)
    let mine: Option<i64> = tx
        .query_opt(
            "SELECT d.revision_id FROM core.lease_revision lr JOIN vyreon.day_revision d USING (revision_id)
             WHERE lr.lease_id = $1 AND d.represented_date = $2::text::date AND d.profile_revision = $3 AND d.state = 'PUBLISHED' LIMIT 1",
            &[&lease.lease_id, &header.represented_date, &header.profile_revision],
        )
        .await
        .map_err(db)?
        .map(|r| r.get(0));
    if let Some(first) = mine {
        return refuse(format!(
            "job {} published {}'s {} as revision {first}; this bundle differs from it: a day is published once with one set of numbers",
            lease.job_id, header.represented_date, header.profile_revision
        ));
    }
    if lease.kind == "DAY_BLOCK" && this == "base" {
        // release audit (Codex 13, Gemini 13): the day's published base of this build, whichever lease
        // published it
        let other: Option<String> = tx
            .query_opt(
                "SELECT d.profile_revision FROM vyreon.day_revision d
                 WHERE d.represented_date = $2::text::date AND d.engine_build = $1 AND d.base_revision IS NULL AND d.state = 'PUBLISHED'
                   AND d.profile_revision <> $3 LIMIT 1",
                &[&header.engine_build, &header.represented_date, &header.profile_revision],
            )
            .await
            .map_err(db)?
            .map(|r| r.get(0));
        if let Some(other) = other {
            return refuse(format!(
                "{}'s base of build {} is published in profile {other}; a second base in profile {} is a day-local or history revision without its base fields",
                header.represented_date, header.engine_build, header.profile_revision
            ));
        }
    }
    Ok(())
}

/// How many slots a stored revision is expected to hold: its build's inventory rows for the field
/// object and for the members recorded as listed with it ($1 the revision, $2 the field object).
/// Read from the revision, never from today's universe, so a stored day is never judged again
/// when the universe is reloaded.
pub const EXPECTED_COUNT: &str = "SELECT count(*) FROM vyreon.day_revision d
     JOIN core.expected_slot e ON e.engine_build = d.engine_build AND e.profile_revision = d.profile_revision
     WHERE d.revision_id = $1 AND (e.object_id = $2 OR e.object_id = ANY(d.listed_members))";

fn prefixed(name: &str, e: Error) -> Error {
    match e {
        Error::Refused(m) => Error::Refused(format!("{name}: {m}")),
        Error::Failed(m) => Error::Failed(format!("{name}: {m}")),
    }
}

/// Asset keys a and not b, sorted, or "none".
fn only_in(a: &HashSet<&str>, b: &HashSet<&str>) -> String {
    let mut keys: Vec<&str> = a.difference(b).copied().collect();
    keys.sort_unstable();
    if keys.is_empty() { "none".to_owned() } else { keys.join(" ") }
}

/// The build; the members listed on the day, core's and the bundle's; the inventory, narrowed to
/// the field and those members, against the bundle's slots; and the month's partitions. Returns
/// the listed members in profile order, which the revision records.
async fn check_header(tx: &Transaction<'_>, header: &Header) -> Result<Vec<String>, Error> {
    let (build, profile, date) = (&header.engine_build, &header.profile_revision, header.represented_date.as_str());
    if !core_contract::is_date(date) {
        return refuse(format!("represented_date {date} is not a date written YYYY-MM-DD"));
    }
    // a day is a session of the calendar: a day published on any other date would be stored where
    // no reader looks (the history lists sessions only)
    let row = tx
        .query_one("SELECT EXISTS (SELECT FROM core.session WHERE session = $1::text::date), EXISTS (SELECT FROM core.session)", &[&date])
        .await
        .map_err(db)?;
    match (row.get::<_, bool>(0), row.get::<_, bool>(1)) {
        (true, _) => {}
        (false, true) => return refuse(format!("{date} is not a trading session of the calendar")),
        (false, false) => return Err(Error::Failed("core holds no trading calendar: `vyreon-core calendar load` first".into())),
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

    // who is listed on the day: core's universe decides, and the worker must have computed with
    // the same members
    let members = tx
        .query("SELECT asset_key, ticker IS NOT NULL FROM core.members_on($1, $2::text::date)", &[&header.profile_id, &date])
        .await
        .map_err(db)?;
    if members.is_empty() {
        return Err(Error::Failed(format!("core holds no members of profile {}: its universe is not loaded", header.profile_id)));
    }
    let listed: Vec<String> = members.iter().filter(|r| r.get::<_, bool>(1)).map(|r| r.get(0)).collect();
    let Some(stated) = &header.listed_members else {
        return refuse(format!("the bundle does not state the members listed on {date} ({LISTED_MEMBERS})"));
    };
    let mut stated_set = HashSet::new();
    if let Some(twice) = stated.iter().find(|key| !stated_set.insert(key.as_str())) {
        return refuse(format!("{LISTED_MEMBERS} names {twice} twice"));
    }
    let listed_set: HashSet<&str> = listed.iter().map(String::as_str).collect();
    if stated_set != listed_set {
        return refuse(format!(
            "the worker and core disagree about the members listed on {date}: listed only by the bundle: {}; only by core: {}",
            only_in(&stated_set, &listed_set),
            only_in(&listed_set, &stated_set)
        ));
    }
    let expected: HashSet<(String, String)> =
        inventory.iter().filter(|(_, object)| object == FIELD_OBJECT || listed_set.contains(object.as_str())).cloned().collect();

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
    let missing: Vec<_> = expected.difference(&sent).collect();
    let (unlisted, extra): (Vec<_>, Vec<_>) = sent.difference(&expected).partition(|slot| inventory.contains(*slot));
    if !missing.is_empty() || !unlisted.is_empty() || !extra.is_empty() {
        let mut parts = Vec::new();
        if !missing.is_empty() {
            parts.push(format!("{} missing ({})", missing.len(), examples(missing)));
        }
        if !unlisted.is_empty() {
            parts.push(format!("{} for objects not listed on {date} ({})", unlisted.len(), examples(unlisted)));
        }
        if !extra.is_empty() {
            parts.push(format!("{} not in the inventory ({})", extra.len(), examples(extra)));
        }
        return refuse(format!("the slots are not the inventory of {build} for {profile} on {date}: {}", parts.join("; ")));
    }
    let month: Option<bool> = tx
        .query_one("SELECT to_regclass('vyreon.measurement' || to_char($1::text::date, '\"_y\"YYYY\"m\"MM')) IS NOT NULL", &[&date])
        .await
        .map_err(db)?
        .get(0);
    if month != Some(true) {
        return Err(Error::Failed(format!("no partitions for the month of {date}: vyreon.ensure_months creates them when work is scheduled")));
    }
    Ok(listed)
}

/// The day's published base revision, for a bundle computed from one. The share lock holds it
/// published until this transaction ends.
async fn bind_base(tx: &Transaction<'_>, header: &Header) -> Result<Option<i64>, Error> {
    let Some(base_profile) = &header.base_profile_revision else {
        return match header.base_revision {
            None if header.day_local => refuse("day_local without base_profile_revision"),
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
    coordinates_text: Option<String>,
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
    let coordinates_text = match (&slot.coordinates_sha256, &files.coordinates) {
        (None, None) => None,
        (Some(listed), Some(bytes)) => {
            if sha256_hex(bytes) != *listed {
                return Err("coordinates digest mismatch".into());
            }
            let text = std::str::from_utf8(bytes).map_err(|e| format!("coordinates: {e}"))?;
            if !serde_json::from_str::<Value>(text).map_err(|e| format!("coordinates: {e}"))?.is_object() {
                return Err("coordinates are not a JSON object".into());
            }
            Some(text.to_owned())
        }
        _ => return Err("the header and the bundle disagree on the slot's coordinates".into()),
    };
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
    Ok(Staged { body_text: body_text.to_owned(), receipt_text: receipt_text.to_owned(), coordinates_text, stream })
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

/// Reads the staged revision back from the database and checks it as a reader would find it,
/// against what its recorded listed members make the inventory expect.
/// Returns (streams, their uncompressed bytes).
async fn read_back(tx: &Transaction<'_>, revision: i64, date: &str) -> Result<(u64, u64), Error> {
    let broken = |what: String| Error::Failed(format!("readback: {what}"));
    let params: [&(dyn ToSql + Sync); 2] = [&revision, &date];
    let expected: i64 = tx.query_one(EXPECTED_COUNT, &[&revision, &FIELD_OBJECT]).await.map_err(db)?.get(0);
    let expected = expected as usize;
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
///
/// Replaced: the day's published revision of the same layer of the profile (its base, its
/// day-local revision or its history revision), whatever its profile revision (C-070: a build that
/// bumped a profile revision left the old one PUBLISHED beside the new, and readers served both;
/// migration 0053 holds one per layer), and as before the day's revision of the same profile
/// revision. Then everything that read what this one
/// replaces or adds to (C-031, ARCHITECTURE 5.3), whichever kind of revision this is:
///   * on the day itself, a new base replaces the base every based revision of the day was
///     computed from; a new day-local revision feeds the day's revisions that read history
///     (DLM-UNUSUALNESS reads every reader value of the day); a new history revision feeds no
///     other revision of its day;
///   * on every later day, each revision that reads history read this day (its windows, or its
///     own earlier values).
///
/// What falls is derived again, not left to vanish: `rederive` queues its ordered-pass jobs again
/// and stops a pass running on an affected day, in this transaction.
async fn promote(tx: &Transaction<'_>, revision: i64, header: &Header, base: Option<i64>, lease: Option<&work::Held>) -> Result<u64, Error> {
    let date = header.represented_date.as_str();
    let mut superseded = tx
        .execute(
            "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
             WHERE represented_date = $1::text::date AND state = 'PUBLISHED'
               AND (profile_revision = $2 OR (profile_id = $3 AND (base_revision IS NULL) = $4 AND day_local = $5))",
            &[&date, &header.profile_revision, &header.profile_id, &base.is_none(), &header.day_local],
        )
        .await
        .map_err(db)?;
    let same_day = match (base, header.day_local) {
        (None, _) => "base_revision IS NOT NULL",
        (Some(_), true) => "base_revision IS NOT NULL AND NOT day_local",
        (Some(_), false) => "false",
    };
    let fallen = tx
        .query(
            &format!(
                "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
                 WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND revision_id <> $2
                   AND ((represented_date > $1::text::date AND NOT day_local) OR (represented_date = $1::text::date AND {same_day}))
                 RETURNING represented_date::text, profile_id, engine_build, day_local, profile_revision"
            ),
            &[&date, &revision],
        )
        .await
        .map_err(db)?;
    superseded += fallen.len() as u64;
    let fallen: Vec<Fallen> =
        fallen.iter().map(|r| Fallen { day: r.get(0), profile_id: r.get(1), engine_build: r.get(2), day_local: r.get(3), profile_revision: r.get(4) }).collect();
    let what = format!("{} of {date} published: {} revisions that read it are superseded", header.profile_revision, fallen.len());
    let by = Trigger { date, what, publisher: Some(&header.engine_build), cause: "A republish left" };
    rederive(tx, &by, same_day != "false", &fallen, lease).await?;
    // the clock, not the transaction's start: an ordered pass compares it with when its lease was
    // granted, and both are taken under the publish lock
    tx.execute("UPDATE vyreon.day_revision SET state = 'PUBLISHED', published_at = clock_timestamp() WHERE revision_id = $1", &[&revision])
        .await
        .map_err(db)?;
    Ok(superseded)
}

/// A revision a publish (or a retirement) superseded because it read what that replaced.
struct Fallen {
    day: String,
    profile_id: String,
    engine_build: String,
    day_local: bool,
    profile_revision: String,
}

/// The ordered pass derives again what fell (ARCHITECTURE 5.2): a pass running on an affected day
/// is stopped (its lease ends and the worker is told the lease is gone) and queued again; each day
/// whose history revision fell and that has a finished pass but no unfinished one under a build that
/// runs (one under a build that does not run is cancelled first, C-063) gets a pass
/// queued with fresh attempts, under a build that runs (C-062): the publishing build when it runs
/// and publishes the fallen revision's profile, else the fallen revision's own build when it runs
/// (the finished pass of that build is queued again; a pass under another build is a new job, the
/// finished one stays DONE). With neither, no job is queued: a pass under a build no worker claims
/// would never run. A day-local revision that fell with its day's base is the day block's to
/// publish again on the new base. What no job will derive again is said in a notice, never left
/// silent.
/// What made revisions fall, for `rederive`: the (earliest) day replaced, the notice's first clause,
/// the build that published (none for a retirement) and the NOT_DERIVED problem's opening words.
struct Trigger<'a> {
    date: &'a str,
    what: String,
    publisher: Option<&'a str>,
    cause: &'a str,
}

/// Returns (the days whose pass is queued or stopped and queued, the days no job derives again).
async fn rederive(tx: &impl GenericClient, by: &Trigger<'_>, same_day: bool, fallen: &[Fallen], lease: Option<&work::Held>) -> Result<(Vec<String>, Vec<String>), Error> {
    let date = by.date;
    // whether a job's build runs (`core.build_runs`, written out: a publish also runs on a database
    // brought to an older migration in the tests)
    let runs = |job: &str| format!("EXISTS (SELECT FROM core.engine_build r WHERE r.engine_build = {job}.engine_build AND r.admitted_at IS NOT NULL AND r.withdrawn_at IS NULL)");
    // a stopped pass of a build that does not run (a lease older than `build::withdraw` ending
    // them) is cancelled, not queued where no worker claims: its day is then targeted below
    let stopped: Vec<String> = tx
        .query(
            &format!("WITH stopped AS (
                 UPDATE core.lease l SET ended_at = now(), outcome = 'CANCELLED', failure = 'a day it reads was published again: it is derived again'
                 FROM core.job j
                 WHERE l.job_id = j.job_id AND l.ended_at IS NULL AND j.kind = 'ORDERED_PASS'
                   AND (j.first_session > $1::text::date OR ($2 AND j.first_session = $1::text::date))
                 RETURNING j.job_id)
             UPDATE core.job SET state = CASE WHEN {r} THEN 'QUEUED' ELSE 'CANCELLED' END,
                                 next_session = first_session, attempts = GREATEST(attempts - 1, 0),
                                 finished_at = CASE WHEN {r} THEN NULL ELSE now() END
             WHERE job_id IN (SELECT job_id FROM stopped) AND state = 'LEASED'
             RETURNING first_session::text, state = 'QUEUED'", r = runs("core.job")),
            &[&date, &same_day],
        )
        .await
        .map_err(db)?
        .iter()
        .filter(|r| r.get::<_, bool>(1))
        .map(|r| r.get(0))
        .collect();
    let history: Vec<&Fallen> = fallen.iter().filter(|f| !f.day_local).collect();
    let days: Vec<&str> = history.iter().map(|f| f.day.as_str()).collect();
    let profiles: Vec<&str> = history.iter().map(|f| f.profile_id.as_str()).collect();
    let builds: Vec<&str> = history.iter().map(|f| f.engine_build.as_str()).collect();
    let revisions: Vec<&str> = history.iter().map(|f| f.profile_revision.as_str()).collect();
    const FALLEN: &str =
        "fallen AS (SELECT * FROM unnest($1::text[]::date[], $2::text[], $3::text[], $4::text[]) AS f(day, profile_id, engine_build, profile_revision))";
    // a pass derives a fallen day again only under a build that runs (audit 2026-11-22 changes-core
    // 7): one of a build that does not run neither stands for the day nor keeps the publishing
    // build from it; it is cancelled first, as `schedule` replaces it
    let unfinished = format!(
        "SELECT FROM core.job u WHERE u.kind = 'ORDERED_PASS' AND u.profile_id = f.profile_id AND u.first_session = f.day
           AND u.state IN ('QUEUED', 'LEASED', 'FAILED') AND {}",
        runs("u")
    );
    let replaced: Vec<i64> = tx
        .query(
            &format!(
                "WITH {FALLEN}
                 UPDATE core.job u SET state = 'CANCELLED', finished_at = now()
                 WHERE u.kind = 'ORDERED_PASS' AND u.state IN ('QUEUED', 'FAILED') AND NOT {}
                   AND EXISTS (SELECT FROM fallen f WHERE f.day = u.first_session AND f.profile_id = u.profile_id)
                 RETURNING u.job_id",
                runs("u")
            ),
            &[&days, &profiles, &builds, &revisions],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // one pass per day and profile (a unique index holds it): when two history revisions of one day
    // fell, the pass goes to the publishing build if either names it
    let requeued: Vec<String> = tx
        .query(
            &format!(
                "WITH {FALLEN},
                 target AS (SELECT DISTINCT ON (t.day, t.profile_id) t.day, t.profile_id, t.build
                            FROM (SELECT f.day, f.profile_id,
                                         CASE WHEN EXISTS (SELECT FROM core.engine_build r WHERE r.engine_build = $5 AND r.admitted_at IS NOT NULL AND r.withdrawn_at IS NULL)
                                                   AND EXISTS (SELECT FROM core.expected_slot e WHERE e.engine_build = $5 AND e.profile_revision = f.profile_revision)
                                              THEN $5
                                              WHEN EXISTS (SELECT FROM core.engine_build r WHERE r.engine_build = f.engine_build AND r.admitted_at IS NOT NULL AND r.withdrawn_at IS NULL)
                                              THEN f.engine_build END AS build
                                  FROM fallen f
                                  WHERE NOT EXISTS ({unfinished})
                                    AND EXISTS (SELECT FROM core.job d WHERE d.kind = 'ORDERED_PASS' AND d.state = 'DONE'
                                                  AND d.profile_id = f.profile_id AND d.first_session = f.day)) t
                            WHERE t.build IS NOT NULL
                            ORDER BY t.day, t.profile_id, t.build = $5 DESC, t.build),
                 again AS (UPDATE core.job SET state = 'QUEUED', next_session = first_session, attempts = 0, finished_at = NULL
                           WHERE job_id IN (SELECT (SELECT d.job_id FROM core.job d
                                                    WHERE d.kind = 'ORDERED_PASS' AND d.state = 'DONE' AND d.profile_id = t.profile_id
                                                      AND d.first_session = t.day AND d.engine_build = t.build
                                                    ORDER BY d.job_id DESC LIMIT 1)
                                            FROM target t)
                           RETURNING first_session),
                 made AS (INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, priority)
                          SELECT 'ORDERED_PASS', t.profile_id, t.build, t.day, t.day, t.day,
                                 (SELECT d.priority FROM core.job d WHERE d.kind = 'ORDERED_PASS' AND d.state = 'DONE' AND d.profile_id = t.profile_id
                                    AND d.first_session = t.day ORDER BY d.job_id DESC LIMIT 1)
                          FROM target t
                          WHERE NOT EXISTS (SELECT FROM core.job d WHERE d.kind = 'ORDERED_PASS' AND d.state = 'DONE' AND d.profile_id = t.profile_id
                                              AND d.first_session = t.day AND d.engine_build = t.build)
                          RETURNING first_session)
                 SELECT first_session::text FROM again UNION ALL SELECT first_session::text FROM made"
            ),
            &[&days, &profiles, &builds, &revisions, &by.publisher],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // (day, profile, day-local): what no job derives again
    let mut unowned: Vec<(String, String, bool)> = tx
        .query(
            &format!("WITH {FALLEN} SELECT DISTINCT f.day::text, f.profile_id FROM fallen f WHERE NOT EXISTS ({unfinished}) ORDER BY 1, 2"),
            &[&days, &profiles, &builds, &revisions],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1), false))
        .collect();
    // a day block publishes its day's day-local revision again on the base it is publishing
    if lease.is_none_or(|l| l.kind != "DAY_BLOCK") {
        unowned.extend(fallen.iter().filter(|f| f.day_local).map(|f| (f.day.clone(), f.profile_id.clone(), true)));
    }
    let orphans: Vec<String> = unowned.iter().map(|(day, _, local)| if *local { format!("{day} (day-local)") } else { day.clone() }).collect();
    let mut rerun: Vec<String> = stopped.iter().chain(&requeued).cloned().collect();
    rerun.sort();
    rerun.dedup();
    if rerun.is_empty() && orphans.is_empty() && replaced.is_empty() {
        return Ok((rerun, orphans));
    }
    let mut text = by.what.clone();
    if !replaced.is_empty() {
        let mut jobs = replaced.clone();
        jobs.sort_unstable();
        text.push_str(&format!("; passes under builds that do not run cancelled: jobs {}", jobs.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")));
    }
    if let (Some(first), Some(last)) = (rerun.first(), rerun.last()) {
        text.push_str(&format!("; the ordered pass derives {} days again, {first} to {last}", rerun.len()));
    }
    if !orphans.is_empty() {
        let shown = orphans.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
        let more = if orphans.len() > 10 { format!(" and {} more", orphans.len() - 10) } else { String::new() };
        text.push_str(&format!("; no job derives these again, schedule them: {shown}{more}"));
    }
    crate::notice::log(tx, &text).await.map_err(Error::Failed)?;
    // what no job derives again needs the owner: a problem until each day has its job (C-050)
    crate::watch::not_derived(tx, &unowned, by.cause).await.map_err(Error::Failed)?;
    Ok((rerun, orphans))
}

/// C-063 amended (audit 2026-11-22 prebuild round 2, core-r2 1): when day block `job` ends for good
/// on `day` (set aside FAILED, or cancelled) with the day's base PUBLISHED and no day-local revision
/// published on it, the base is superseded and recorded (`core.dropped_base`), and what read it falls
/// and is derived again (C-037, `rederive`, no publishing build). The engine's ordered pass reads every
/// published base, so a base left so would feed later days' history while core calls its day missing;
/// superseded, the day is missing everywhere. A later attempt publishes the day afresh: its base is a
/// new revision and its day-local revision binds to it. Call it under the publish lock. Returns the
/// base it superseded.
pub(crate) async fn drop_base_without_day_local(tx: &impl GenericClient, job: i64, day: &str, profile: &str) -> Result<Option<i64>, String> {
    let q = |e: tokio_postgres::Error| db(e).to_string();
    let Some(base) = tx.query_one("SELECT core.base_without_day_local($1::text::date, $2)", &[&day, &profile]).await.map_err(q)?.get::<_, Option<i64>>(0) else {
        return Ok(None);
    };
    tx.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = $1 AND state = 'PUBLISHED'", &[&base]).await.map_err(q)?;
    tx.execute(
        "INSERT INTO core.dropped_base (revision_id, job_id, represented_date, profile_id) VALUES ($1, $2, $3::text::date, $4)",
        &[&base, &job, &day, &profile],
    )
    .await
    .map_err(q)?;
    // what read it: a revision of the day computed on a base, and every later day's history revision
    let fallen: Vec<Fallen> = tx
        .query(
            "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
             WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL
               AND ((represented_date > $1::text::date AND NOT day_local) OR represented_date = $1::text::date)
             RETURNING represented_date::text, profile_id, engine_build, day_local, profile_revision",
            &[&day],
        )
        .await
        .map_err(q)?
        .iter()
        .map(|r| Fallen { day: r.get(0), profile_id: r.get(1), engine_build: r.get(2), day_local: r.get(3), profile_revision: r.get(4) })
        .collect();
    let what = format!(
        "Day {day}'s base revision {base} superseded: its day block (job {job}) ended without a day-local revision on it; {} revisions that read it are superseded",
        fallen.len()
    );
    let by = Trigger { date: day, what, publisher: None, cause: "A day set aside without its day-local revision left" };
    rederive(tx, &by, true, &fallen, None).await.map_err(|e| e.to_string())?;
    Ok(Some(base))
}

/// One revision a retirement takes down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredRevision {
    pub date: String,
    pub revision_id: i64,
    pub profile_id: String,
    pub profile_revision: String,
    pub engine_build: String,
    /// "base", "day-local" or "history"
    pub kind: String,
}

/// What `retire` did, or, previewed, would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retired {
    /// None when previewed: nothing was changed.
    pub retirement_id: Option<i64>,
    pub revisions: Vec<RetiredRevision>,
    /// The unfinished jobs of the days' builds it cancelled (as cancelled, not as selected).
    pub cancelled: Vec<i64>,
    /// Later days' history revisions that read a retired day and fall with it (C-037).
    pub fallen: u64,
    /// The days whose ordered pass is queued (or stopped and queued) to derive them again.
    pub rederived: Vec<String>,
    /// The days no job derives again (named in a notice and a NOT_DERIVED problem).
    pub unowned: Vec<String>,
    /// What the retirement takes down, as a token: the revisions, the jobs it cancels and the later
    /// history revisions that fall with it. A preview
    /// prints it; the retirement itself is refused unless it is given the same (audit 2026-11-22
    /// changes-core 13).
    pub token: String,
}

/// The token of a retirement: the first 16 hex digits of the SHA-256 of its revisions, the jobs it
/// cancels and the later history revisions that fall with it (audit 2026-11-22 round 2, core-r2 5),
/// each in order.
fn retirement_token(revisions: &[RetiredRevision], cancelled: &[i64], later: &[i64]) -> String {
    let ids = |v: &mut dyn Iterator<Item = i64>| v.map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let text = format!(
        "revisions {}; jobs {}; later {}",
        ids(&mut revisions.iter().map(|r| r.revision_id)),
        ids(&mut cancelled.iter().copied()),
        ids(&mut later.iter().copied())
    );
    sha256_hex(text.as_bytes())[..16].to_owned()
}

/// C-062: retires the published days of `dates` with the operator's reason. Every PUBLISHED revision
/// of those dates (base, day-local and history, of every profile) becomes SUPERSEDED: every row is
/// kept, none is deleted, and `core.retired_revision` names each with its retirement. The unfinished
/// day blocks and ordered passes of those dates whose build published a revision of that day are
/// cancelled (a job of another build, which will compute the day afresh, stays); a day block that
/// also covers a date not named refuses the whole retirement. What read a retired day falls with it,
/// as after a publish (C-037): every later day's PUBLISHED history revision is superseded and its
/// ordered pass queued again under a build that runs (`rederive`, with no publishing build), or the
/// day is named. The retirement, its reason, the sessions and the jobs cancelled are one row of
/// `core.retirement` and a logged notice; the reader then states each date NOT_PUBLISHED with the
/// cause RETIRED (unless a day job is pending or failed for it). With `preview`, everything is done
/// and reported inside a transaction that is rolled back: the report is exactly what it would do, and
/// its `token` names it. Without `preview`, `expect` must be that token: a retirement whose revisions,
/// cancelled jobs or falling later history revisions are not the ones previewed (a build published one
/// of the dates since, a job was scheduled, a later day's pass published) is refused whole, so nothing
/// the operator did not see is taken down (audit 2026-11-22 changes-core 13, round 2 core-r2 5).
pub async fn retire(client: &mut Client, reason: &str, dates: &[&str], preview: bool, expect: Option<&str>) -> Result<Retired, String> {
    let fail = |e: Error| e.to_string();
    let reason = work::scrubbed(reason.trim());
    if reason.is_empty() {
        return Err("a retirement needs a reason".into());
    }
    if dates.is_empty() {
        return Err("name at least one date to retire".into());
    }
    if let Some(bad) = dates.iter().find(|d| !core_contract::is_date(d)) {
        return Err(format!("{bad} is not a date written YYYY-MM-DD"));
    }
    if !preview && expect.is_none() {
        return Err("a retirement takes down only what its preview showed: run it with --preview first and give the token it prints (--expect); nothing was retired".into());
    }
    let mut dates: Vec<&str> = dates.to_vec();
    dates.sort_unstable();
    dates.dedup();
    let tx = client.transaction().await.map_err(|e| fail(db(e)))?;
    // it changes what is published and cancels jobs: one publish at a time, then the scheduler's lock
    // (the order `omit` and a schedule of ordered passes take them in; a publish takes the first only)
    tx.execute(work::PUBLISH_LOCK, &[]).await.map_err(|e| fail(db(e)))?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(|e| fail(db(e)))?;
    let q = |e: tokio_postgres::Error| fail(db(e));
    for date in &dates {
        if tx.query_opt("SELECT FROM core.session WHERE session = $1::text::date", &[date]).await.map_err(q)?.is_none() {
            return Err(format!("{date} is not a trading session"));
        }
    }
    let revisions: Vec<RetiredRevision> = tx
        .query(
            "SELECT represented_date::text, revision_id, profile_id, profile_revision, engine_build,
                    CASE WHEN base_revision IS NULL THEN 'base' WHEN day_local THEN 'day-local' ELSE 'history' END
             FROM vyreon.day_revision WHERE represented_date = ANY($1::text[]::date[]) AND state = 'PUBLISHED'
             ORDER BY represented_date, base_revision IS NOT NULL, day_local DESC, profile_revision FOR UPDATE",
            &[&dates],
        )
        .await
        .map_err(q)?
        .iter()
        .map(|r| RetiredRevision { date: r.get(0), revision_id: r.get(1), profile_id: r.get(2), profile_revision: r.get(3), engine_build: r.get(4), kind: r.get(5) })
        .collect();
    if let Some(empty) = dates.iter().find(|d| !revisions.iter().any(|r| r.date == **d)) {
        return Err(format!("nothing is published on {empty}: nothing to retire"));
    }
    // the unfinished jobs of the builds that published these days (any revision, published once)
    let jobs = tx
        .query(
            "SELECT j.job_id, j.kind, j.first_session::text, j.last_session::text,
                    EXISTS (SELECT FROM core.session s WHERE s.session BETWEEN j.first_session AND j.last_session
                              AND NOT s.session = ANY($1::text[]::date[]))
             FROM core.job j
             WHERE j.kind IN ('DAY_BLOCK', 'ORDERED_PASS') AND j.state IN ('QUEUED', 'LEASED', 'FAILED')
               AND EXISTS (SELECT FROM vyreon.day_revision d
                           WHERE d.represented_date = ANY($1::text[]::date[]) AND d.represented_date BETWEEN j.first_session AND j.last_session
                             AND d.profile_id = j.profile_id AND d.engine_build = j.engine_build AND d.published_at IS NOT NULL)
             ORDER BY j.job_id",
            &[&dates],
        )
        .await
        .map_err(q)?;
    if let Some(wide) = jobs.iter().find(|r| r.get::<_, bool>(4)) {
        let (job, first, last): (i64, String, String) = (wide.get(0), wide.get(2), wide.get(3));
        return Err(format!("day block {job} ({first} to {last}) also covers dates not named: cancel or finish it first; nothing was retired"));
    }
    let cancelled: Vec<i64> = jobs.iter().map(|r| r.get(0)).collect();
    // the later days' history revisions that read a retired day and fall with it (below): in the token,
    // so a pass that published one after the preview is never taken down unseen (core-r2 5)
    let later: Vec<i64> = tx
        .query(
            "SELECT revision_id FROM vyreon.day_revision
             WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local AND represented_date > $1::text::date
               AND NOT represented_date = ANY($2::text[]::date[])
             ORDER BY revision_id FOR UPDATE",
            &[&dates[0], &dates],
        )
        .await
        .map_err(q)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let token = retirement_token(&revisions, &cancelled, &later);
    if let Some(expected) = expect.filter(|e| !preview && *e != token) {
        return Err(format!(
            "what these dates hold changed since the preview (its token {expected}, now {token}): a revision was published or superseded, \
             a later day's history revision published or superseded, or a job scheduled or ended; preview again; nothing was retired"
        ));
    }
    // the jobs it cancelled are recorded once cancelled (below): a job selected above may finish before
    // then (its worker needs no publish lock), and is then neither cancelled nor named (core-r2 9)
    let retirement_id: i64 = tx
        .query_one(
            "INSERT INTO core.retirement (reason, sessions, cancelled) VALUES ($1, $2::text[]::date[], '{}') RETURNING retirement_id",
            &[&reason, &dates],
        )
        .await
        .map_err(q)?
        .get(0);
    let ids: Vec<i64> = revisions.iter().map(|r| r.revision_id).collect();
    tx.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = ANY($1)", &[&ids]).await.map_err(q)?;
    tx.execute(
        "INSERT INTO core.retired_revision (revision_id, retirement_id, represented_date, profile_id)
         SELECT revision_id, $2, represented_date, profile_id FROM vyreon.day_revision WHERE revision_id = ANY($1)",
        &[&ids, &retirement_id],
    )
    .await
    .map_err(q)?;
    // after the revisions are down, so the cancel finds no published base to leave underived
    let cancelled = work::cancel_in(&tx, &cancelled).await?;
    tx.execute("UPDATE core.retirement SET cancelled = $2 WHERE retirement_id = $1", &[&retirement_id, &cancelled]).await.map_err(q)?;
    // what read a retired day: every later history revision (C-037), derived again
    let first = dates[0];
    let fallen: Vec<Fallen> = tx
        .query(
            "UPDATE vyreon.day_revision SET state = 'SUPERSEDED'
             WHERE state = 'PUBLISHED' AND base_revision IS NOT NULL AND NOT day_local AND represented_date > $1::text::date
             RETURNING represented_date::text, profile_id, engine_build, day_local, profile_revision",
            &[&first],
        )
        .await
        .map_err(q)?
        .iter()
        .map(|r| Fallen { day: r.get(0), profile_id: r.get(1), engine_build: r.get(2), day_local: r.get(3), profile_revision: r.get(4) })
        .collect();
    let named = if dates.len() == 1 { first.to_owned() } else { format!("{} dates, {first} to {}", dates.len(), dates[dates.len() - 1]) };
    let by = Trigger { date: first, what: format!("Retired {named}: {} revisions that read them are superseded", fallen.len()), publisher: None, cause: "A retirement left" };
    let (rederived, unowned) = rederive(&tx, &by, true, &fallen, None).await.map_err(fail)?;
    let jobs_text = if cancelled.is_empty() { "no unfinished job".to_owned() } else { format!("jobs {} cancelled", cancelled.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")) };
    crate::notice::log(&tx, &format!("Retirement {retirement_id}: {} published revisions of {named} superseded (kept), {jobs_text}: {reason}.", revisions.len())).await?;
    let report = Retired { retirement_id: Some(retirement_id), revisions, cancelled, fallen: fallen.len() as u64, rederived, unowned, token };
    if preview {
        tx.rollback().await.map_err(q)?;
        return Ok(Retired { retirement_id: None, ..report });
    }
    tx.commit().await.map_err(q)?;
    Ok(report)
}
