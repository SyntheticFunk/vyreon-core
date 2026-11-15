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
    // the bundle is read on a blocking thread, one slot ahead of the database; its end is sent as
    // `None` once the whole bundle has been read, so a channel closed without it (the reader
    // failed, perhaps after the last slot) is never taken for a complete bundle
    let (sender, mut items) = channel::<Option<Item>>(1);
    let reader = tokio::task::spawn_blocking(move || {
        let stopped = |_| std::io::Error::other("publish stopped");
        read_bundle(std::io::BufReader::new(file), &mut |item| sender.blocking_send(Some(item)).map_err(stopped))?;
        sender.blocking_send(None).map_err(stopped)
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

async fn publish_items(client: &mut Client, items: &mut Receiver<Option<Item>>, token: Option<&str>) -> Result<Published, Error> {
    let Some(Some(Item::Header(header))) = items.recv().await else { return refuse("the bundle has no header") };
    let Some(Some(Item::Scalars(scalars))) = items.recv().await else { return refuse("the bundle has no scalars") };
    let tx = client.transaction().await.map_err(db)?;
    // one publish at a time, taken before any row lock: a publish supersedes and queues again what
    // read its day, and an ordered pass is claimed under the same lock (`work::PUBLISH_LOCK`)
    tx.execute(work::PUBLISH_LOCK, &[]).await.map_err(db)?;
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
            // a day block's current day stays its next until the worker says it is done, so a
            // bundle of it sent again is answered like one of an earlier day
            if lease.engine_build == header.engine_build
                && lease.profile_id == header.profile_id
                && (lease.kind == "DAY_BLOCK" || lease.next_session.as_deref() != Some(date))
                && let Some(revision_id) = published_before(&tx, &header, &lease).await?
            {
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
/// day with this very bundle: an earlier session of the job (or a day block's current one), the
/// same profile and build, the same listed members, each slot once, and every slot's body digest
/// equal. A worker sends a day again when the answer to its first sending was lost; that is
/// answered, not refused. Anything else goes on to the full checks.
async fn published_before(tx: &Transaction<'_>, header: &Header, lease: &work::Held) -> Result<Option<i64>, Error> {
    let date = header.represented_date.as_str();
    let current = lease.kind == "DAY_BLOCK" && lease.next_session.as_deref() == Some(date);
    if !core_contract::is_date(date) || date > lease.last_session.as_str() || (!current && lease.next_session.as_deref().is_some_and(|next| date >= next)) {
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
/// Replaced: the day's previous revision of the profile. Then everything that read what this one
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
             WHERE represented_date = $1::text::date AND profile_revision = $2 AND state = 'PUBLISHED'",
            &[&date, &header.profile_revision],
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
                 RETURNING represented_date::text, profile_id, engine_build, day_local"
            ),
            &[&date, &revision],
        )
        .await
        .map_err(db)?;
    superseded += fallen.len() as u64;
    let fallen: Vec<Fallen> = fallen.iter().map(|r| Fallen { day: r.get(0), profile_id: r.get(1), engine_build: r.get(2), day_local: r.get(3) }).collect();
    rederive(tx, header, same_day != "false", &fallen, lease).await?;
    // the clock, not the transaction's start: an ordered pass compares it with when its lease was
    // granted, and both are taken under the publish lock
    tx.execute("UPDATE vyreon.day_revision SET state = 'PUBLISHED', published_at = clock_timestamp() WHERE revision_id = $1", &[&revision])
        .await
        .map_err(db)?;
    Ok(superseded)
}

/// A revision a publish superseded because it read what the publish replaced.
struct Fallen {
    day: String,
    profile_id: String,
    engine_build: String,
    day_local: bool,
}

/// The ordered pass derives again what fell (ARCHITECTURE 5.2): a pass running on an affected day
/// is stopped (its lease ends and the worker is told the lease is gone) and queued again; the
/// finished pass of each day whose history revision fell is queued again with fresh attempts.
/// A day-local revision that fell with its day's base is the day block's to publish again on the
/// new base. What no job will derive again is said in a notice, never left silent.
async fn rederive(tx: &Transaction<'_>, header: &Header, same_day: bool, fallen: &[Fallen], lease: Option<&work::Held>) -> Result<(), Error> {
    let date = header.represented_date.as_str();
    let stopped: Vec<String> = tx
        .query(
            "WITH stopped AS (
                 UPDATE core.lease l SET ended_at = now(), outcome = 'CANCELLED', failure = 'a day it reads was published again: it is derived again'
                 FROM core.job j
                 WHERE l.job_id = j.job_id AND l.ended_at IS NULL AND j.kind = 'ORDERED_PASS'
                   AND (j.first_session > $1::text::date OR ($2 AND j.first_session = $1::text::date))
                 RETURNING j.job_id)
             UPDATE core.job SET state = 'QUEUED', next_session = first_session, attempts = GREATEST(attempts - 1, 0), finished_at = NULL
             WHERE job_id IN (SELECT job_id FROM stopped) AND state = 'LEASED'
             RETURNING first_session::text",
            &[&date, &same_day],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let history: Vec<&Fallen> = fallen.iter().filter(|f| !f.day_local).collect();
    let days: Vec<&str> = history.iter().map(|f| f.day.as_str()).collect();
    let profiles: Vec<&str> = history.iter().map(|f| f.profile_id.as_str()).collect();
    let builds: Vec<&str> = history.iter().map(|f| f.engine_build.as_str()).collect();
    const FALLEN: &str = "fallen AS (SELECT * FROM unnest($1::text[]::date[], $2::text[], $3::text[]) AS f(day, profile_id, engine_build))";
    const UNFINISHED: &str = "SELECT FROM core.job u WHERE u.kind = 'ORDERED_PASS' AND u.profile_id = f.profile_id AND u.first_session = f.day
                                  AND u.state IN ('QUEUED', 'LEASED', 'FAILED')";
    let requeued: Vec<String> = tx
        .query(
            &format!(
                "WITH {FALLEN},
                 redo AS (SELECT DISTINCT ON (f.day, f.profile_id, f.engine_build) j.job_id
                          FROM fallen f JOIN core.job j ON j.kind = 'ORDERED_PASS' AND j.state = 'DONE' AND j.profile_id = f.profile_id
                                                       AND j.engine_build = f.engine_build AND j.first_session = f.day
                          WHERE NOT EXISTS ({UNFINISHED})
                          ORDER BY f.day, f.profile_id, f.engine_build, j.job_id DESC)
                 UPDATE core.job SET state = 'QUEUED', next_session = first_session, attempts = 0, finished_at = NULL
                 WHERE job_id IN (SELECT job_id FROM redo)
                 RETURNING first_session::text"
            ),
            &[&days, &profiles, &builds],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut orphans: Vec<String> = tx
        .query(&format!("WITH {FALLEN} SELECT DISTINCT f.day::text FROM fallen f WHERE NOT EXISTS ({UNFINISHED}) ORDER BY 1"), &[&days, &profiles, &builds])
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // a day block publishes its day's day-local revision again on the base it is publishing
    if lease.is_none_or(|l| l.kind != "DAY_BLOCK") {
        orphans.extend(fallen.iter().filter(|f| f.day_local).map(|f| format!("{} (day-local)", f.day)));
    }
    if stopped.is_empty() && requeued.is_empty() && orphans.is_empty() {
        return Ok(());
    }
    let mut rerun: Vec<&String> = stopped.iter().chain(&requeued).collect();
    rerun.sort();
    rerun.dedup();
    let mut text = format!("{} of {date} published: {} revisions that read it are superseded", header.profile_revision, fallen.len());
    if let (Some(first), Some(last)) = (rerun.first(), rerun.last()) {
        text.push_str(&format!("; the ordered pass derives {} days again, {first} to {last}", rerun.len()));
    }
    if !orphans.is_empty() {
        let shown = orphans.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
        let more = if orphans.len() > 10 { format!(" and {} more", orphans.len() - 10) } else { String::new() };
        text.push_str(&format!("; no job derives these again, schedule them: {shown}{more}"));
    }
    crate::notice::post(tx, &text).await.map_err(Error::Failed)
}
