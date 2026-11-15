//! The publish contract between the engine and core (C-019): one day of one profile as a bundle.
//! The engine writes it, core checks it and is the only writer of the store. This crate is the
//! whole contract and holds no engine code and no database code.
//!
//! A bundle is a gzip-compressed tar with these entries, in this order:
//!
//!   bundle.json                         the `Header`: the day, the build, one `Slot` per measurement
//!   scalars.json                        the `Scalars` of the day's bodies
//!   slots/00000/body.json               slot 0's body, exactly as the engine wrote it (it ends
//!                                       with a newline; `body_sha256` is over these bytes)
//!   slots/00000/receipt.json            its receipt
//!   slots/00000/coordinates.json        only for a slot the engine projects: its coordinates view
//!                                       (each coordinate's value, state, reason, unit and quality)
//!   slots/00000/events/root.json        only for a slot with a stream: the stream's root,
//!   slots/00000/events/events.jsonl     and its events, uncompressed
//!   slots/00001/...
//!
//! What the bundle states and core takes as given: each slot's identity, state and versions (the
//! engine checks them against its bodies and receipts, whose shapes are its own), its coordinates
//! view (the engine's projection of its own body; core cannot recompute it), and the header's
//! `report` (the run's stage timings and request counts). What core
//! checks: every digest, the stream commitments, the stated listed members against core's own
//! universe, and the slot list against the inventory registered for the build, narrowed to the
//! field and the members listed that day.
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

pub const CONTRACT: &str = "VYREON-DAY-BUNDLE@2";

/// The cross-asset object. Every other object of an inventory is a member's asset key, and a
/// member has slots only on the sessions it is listed (C-036).
pub const FIELD_OBJECT: &str = "FIELD.CORE64";

/// The header field that states the session's listed members. Its name is this constant and the
/// `rename` on `Header::listed_members` (they must agree: a test checks it).
pub const LISTED_MEMBERS: &str = "listed_members";

/// The most bytes one entry of a bundle may hold uncompressed. Read before anything is allocated,
/// so a damaged or hostile bundle cannot make its reader take more memory than this per entry
/// (the upload limit bounds only the compressed bytes). Unmeasured at its maximum; a measured day
/// (2025-12-10) holds 320 event streams of 724 MB in all, about 2.3 MB each, so 256 MiB is two
/// orders of magnitude above the typical entry.
pub const MAX_ENTRY_BYTES: u64 = 256 << 20;

/// Whether `text` is a calendar date written `YYYY-MM-DD`. Dates cross the contract in this form
/// only: PostgreSQL would also read words such as "yesterday" as dates.
pub fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// `CONTRACT`.
    pub contract: String,
    /// The trading day the measurements represent, `YYYY-MM-DD`.
    pub represented_date: String,
    pub profile_id: String,
    pub profile_revision: String,
    /// The exact binary identity; it must be registered with core.
    pub engine_build: String,
    /// RFC 3339.
    pub snapshot_cutoff: String,
    /// For a revision computed from the day's base revision: that base's profile revision. Core
    /// binds the new revision to the day's published revision of that profile.
    pub base_profile_revision: Option<String>,
    /// Core's id of the base revision the bodies were computed from, when the engine knows it;
    /// the bundle is refused if that is no longer the day's published base.
    pub base_revision: Option<i64>,
    /// For a revision computed from the day's base: true when it reads that day only (C-031). A
    /// base republished on an earlier day then leaves it standing; a based revision that reads
    /// history (the default) is superseded and derived again.
    #[serde(default)]
    pub day_local: bool,
    /// What the run that computed the day measured of itself: stage timings, request counts by
    /// category. A JSON object of the engine's choosing; core stores it with the revision.
    #[serde(default)]
    pub report: Option<serde_json::Value>,
    /// The members listed on the day, as asset keys in profile order: the worker's view of the
    /// universe it computed with (C-036). Core refuses a bundle whose set differs from its own
    /// (`core.members_on`, members with a ticker that day) and expects slots for those members and
    /// the field only. Optional in the format so an older bundle still reads; core refuses one
    /// without it.
    #[serde(default, rename = "listed_members", skip_serializing_if = "Option::is_none")]
    pub listed_members: Option<Vec<String>>,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub measurement_id: String,
    pub object_id: String,
    pub slot_state: String,
    pub science_version: String,
    pub method_version: String,
    pub body_sha256: String,
    pub receipt_sha256: String,
    /// The SHA-256 of the stream's root.json, for a body that commits to an event stream.
    pub stream_root_sha256: Option<String>,
    /// The SHA-256 of the slot's coordinates.json, when the engine projects the slot.
    #[serde(default)]
    pub coordinates_sha256: Option<String>,
}

/// The reader-facing numbers of the day's bodies (the engine's projection of them) and, for the
/// profile that carries unusualness, its robust z values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scalars {
    pub reader_scalar: Vec<Scalar>,
    pub unusual_score: Vec<Scalar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scalar {
    pub object_id: String,
    pub measurement_id: String,
    pub role_id: String,
    pub value: f64,
}

/// A stream's root.json, as far as the contract reads it.
#[derive(Debug, Deserialize)]
pub struct StreamRoot {
    pub events_sha256: String,
    pub event_count: i64,
    pub byte_length: i64,
}

/// One slot's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotFiles {
    pub body: Vec<u8>,
    pub receipt: Vec<u8>,
    /// coordinates.json
    pub coordinates: Option<Vec<u8>>,
    /// (root.json, events.jsonl)
    pub stream: Option<(Vec<u8>, Vec<u8>)>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Writes a bundle. The header is written first, so every slot is described before its files
/// are added; `add` then takes the slots' files in the header's order and checks each against
/// its description.
pub struct BundleWriter<W: Write> {
    tar: tar::Builder<GzEncoder<W>>,
    slots: Vec<Slot>,
    next: usize,
}

impl<W: Write> BundleWriter<W> {
    pub fn new(out: W, header: &Header, scalars: &Scalars) -> io::Result<Self> {
        if header.contract != CONTRACT {
            return Err(invalid(format!("contract is {}, this writer writes {CONTRACT}", header.contract)));
        }
        let mut writer = BundleWriter { tar: tar::Builder::new(GzEncoder::new(out, Compression::default())), slots: header.slots.clone(), next: 0 };
        writer.entry("bundle.json", &serde_json::to_vec(header).map_err(io::Error::other)?)?;
        writer.entry("scalars.json", &serde_json::to_vec(scalars).map_err(io::Error::other)?)?;
        Ok(writer)
    }

    fn entry(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let mut head = tar::Header::new_gnu();
        head.set_size(bytes.len() as u64);
        head.set_mode(0o644);
        head.set_cksum();
        self.tar.append_data(&mut head, path, bytes)
    }

    pub fn add(&mut self, files: &SlotFiles) -> io::Result<()> {
        let slot = self.slots.get(self.next).ok_or_else(|| invalid("more slots added than the header lists"))?.clone();
        let name = format!("{}/{}", slot.measurement_id, slot.object_id);
        if sha256_hex(&files.body) != slot.body_sha256 || sha256_hex(&files.receipt) != slot.receipt_sha256 {
            return Err(invalid(format!("{name}: files do not match the header's digests")));
        }
        match (&slot.stream_root_sha256, &files.stream) {
            (Some(root), Some((root_bytes, _))) if *root == sha256_hex(root_bytes) => {}
            (None, None) => {}
            _ => return Err(invalid(format!("{name}: stream does not match the header"))),
        }
        if slot.coordinates_sha256 != files.coordinates.as_deref().map(sha256_hex) {
            return Err(invalid(format!("{name}: coordinates do not match the header")));
        }
        let dir = format!("slots/{:05}", self.next);
        self.entry(&format!("{dir}/body.json"), &files.body)?;
        self.entry(&format!("{dir}/receipt.json"), &files.receipt)?;
        if let Some(coordinates) = &files.coordinates {
            self.entry(&format!("{dir}/coordinates.json"), coordinates)?;
        }
        if let Some((root, events)) = &files.stream {
            self.entry(&format!("{dir}/events/root.json"), root)?;
            self.entry(&format!("{dir}/events/events.jsonl"), events)?;
        }
        self.next += 1;
        Ok(())
    }

    pub fn finish(self) -> io::Result<W> {
        if self.next != self.slots.len() {
            return Err(invalid(format!("{} of {} slots added", self.next, self.slots.len())));
        }
        self.tar.into_inner()?.finish()
    }
}

/// What a bundle holds, in the order it is read.
#[derive(Debug)]
pub enum Item {
    Header(Box<Header>),
    Scalars(Scalars),
    /// A slot's files, in the header's order.
    Slot(SlotFiles),
}

/// Reads a bundle front to back and hands each item to `visit`, holding one slot's files in
/// memory at a time. It checks the layout only (every slot of the header present, in order,
/// nothing else); the digests are the visitor's to check. An error from `visit` stops the read.
pub fn read_bundle<R: Read>(input: R, visit: &mut dyn FnMut(Item) -> io::Result<()>) -> io::Result<()> {
    type Entry = Option<(String, Vec<u8>)>;
    fn named(found: Entry, path: &str) -> io::Result<Vec<u8>> {
        match found {
            Some((name, bytes)) if name == path => Ok(bytes),
            Some((name, _)) => Err(invalid(format!("expected {path}, found {name}"))),
            None => Err(invalid(format!("expected {path}, found the end of the bundle"))),
        }
    }
    let mut archive = tar::Archive::new(GzDecoder::new(input));
    let mut entries = archive.entries()?;
    // the next entry: the one read ahead while looking for a slot's optional stream, or a new one
    let mut next = |held: &mut Entry| -> io::Result<Entry> {
        if held.is_some() {
            return Ok(held.take());
        }
        let Some(entry) = entries.next() else { return Ok(None) };
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().into_owned();
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!("{name} is {} bytes uncompressed; an entry is at most {MAX_ENTRY_BYTES}", entry.size())));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        (&mut entry).take(MAX_ENTRY_BYTES).read_to_end(&mut bytes)?;
        Ok(Some((name, bytes)))
    };
    let mut held: Entry = None;
    let header: Header = serde_json::from_slice(&named(next(&mut held)?, "bundle.json")?).map_err(|e| invalid(format!("bundle.json: {e}")))?;
    if header.contract != CONTRACT {
        return Err(invalid(format!("contract is {}, this reader reads {CONTRACT}", header.contract)));
    }
    let slots = header.slots.len();
    visit(Item::Header(Box::new(header)))?;
    let scalars = serde_json::from_slice(&named(next(&mut held)?, "scalars.json")?).map_err(|e| invalid(format!("scalars.json: {e}")))?;
    visit(Item::Scalars(scalars))?;
    for i in 0..slots {
        let dir = format!("slots/{i:05}");
        let body = named(next(&mut held)?, &format!("{dir}/body.json"))?;
        let receipt = named(next(&mut held)?, &format!("{dir}/receipt.json"))?;
        let coordinates = match next(&mut held)? {
            Some((name, bytes)) if name == format!("{dir}/coordinates.json") => Some(bytes),
            other => {
                held = other;
                None
            }
        };
        let stream = match next(&mut held)? {
            Some((name, root)) if name == format!("{dir}/events/root.json") => {
                Some((root, named(next(&mut held)?, &format!("{dir}/events/events.jsonl"))?))
            }
            other => {
                held = other;
                None
            }
        };
        visit(Item::Slot(SlotFiles { body, receipt, coordinates, stream }))?;
    }
    if let Some((name, _)) = next(&mut held)? {
        return Err(invalid(format!("unexpected entry {name} after the last slot")));
    }
    // the gzip stream is read to its end, where its CRC and length are checked: bundle.json and
    // scalars.json carry no digest of their own, and the tar ends before the trailer
    io::copy(&mut archive.into_inner(), &mut io::sink()).map_err(|e| invalid(format!("the gzip stream after the last entry: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's own bundle of 2025-12-10 (contract 2, written before the listed set existed).
    const ENGINE_BUNDLE_AAPL: &[u8] = include_bytes!("../../test-support/fixtures/engine-bundle-2025-12-10-aapl.tar.gz");

    fn engine_header() -> Header {
        let mut header = None;
        read_bundle(ENGINE_BUNDLE_AAPL, &mut |item| {
            if let Item::Header(h) = item {
                header = Some(*h);
            }
            Ok(())
        })
        .unwrap();
        header.unwrap()
    }

    /// Audit 2026-11-14 (core.md) 9: an entry whose header claims more than `MAX_ENTRY_BYTES` is
    /// refused before anything is allocated for it (the reader used to reserve whatever the
    /// header claimed). The engine's real header is the entry; only its declared size is wrong.
    #[test]
    fn an_entry_larger_than_the_limit_is_refused_before_it_is_read() {
        let header = serde_json::to_vec(&engine_header()).unwrap();
        let mut head = tar::Header::new_gnu();
        head.set_path("bundle.json").unwrap();
        head.set_size(MAX_ENTRY_BYTES + 1);
        head.set_mode(0o644);
        head.set_cksum();
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(head.as_bytes()).unwrap();
        gz.write_all(&header).unwrap();
        let bundle = gz.finish().unwrap();
        let mut visited = 0;
        let refused = read_bundle(bundle.as_slice(), &mut |_| {
            visited += 1;
            Ok(())
        })
        .unwrap_err();
        assert!(refused.to_string().contains(&format!("bundle.json is {} bytes uncompressed; an entry is at most {MAX_ENTRY_BYTES}", MAX_ENTRY_BYTES + 1)), "{refused}");
        assert_eq!(visited, 0);
    }

    #[test]
    fn the_listed_set_travels_under_its_one_name() {
        // a header written before the field still reads, without a listed set
        let mut header = engine_header();
        assert_eq!(header.listed_members, None);
        assert!(serde_json::to_value(&header).unwrap().get(LISTED_MEMBERS).is_none());
        // the field's name in the JSON is the constant
        header.listed_members = Some(vec!["AAPL".into()]);
        let text = serde_json::to_value(&header).unwrap();
        assert_eq!(text[LISTED_MEMBERS], serde_json::json!(["AAPL"]));
        assert_eq!(serde_json::from_value::<Header>(text).unwrap(), header);
    }
}
