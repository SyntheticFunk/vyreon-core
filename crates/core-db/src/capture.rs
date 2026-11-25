//! Input captures (migration 0016, C-043): the small inputs the provider restates in place, kept as
//! a published day read them. A capture is the engine's packing of one capture directory: a gzip
//! tar whose first entry is the capture's `manifest.json` and whose other entries are the
//! directory's files. Its identity is the SHA-256 of that manifest. Core checks what is generic
//! (the archive reads to its end, the first entry is the manifest and hashes to the identity, every
//! entry is a file with a relative name, the sizes are bounded) and stores the archive as sent.
use crate::work::{self, LeaseError};
use core_contract::sha256_hex;
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::io::Read;
use tokio_postgres::Client;

/// The kinds of capture kept: the company capture (filings, share reports, float, fundamentals,
/// the daily-metrics row), the macro capture and the carry capture (dividends, spot, rates).
pub const KINDS: [&str; 3] = ["company", "macro", "carry"];
/// The entry an archive starts with; its SHA-256 is the capture's identity.
pub const MANIFEST: &str = "manifest.json";
/// The most bytes an archive may hold as sent. Measured on a cluster worker (2026-11-16): the
/// company capture about 4.2 MB gzipped, the carry 0.8 MB and the macro 0.2 MB unpacked.
pub const MAX_ARCHIVE_BYTES: usize = 64 << 20;
/// The most bytes an archive may unpack to (the company capture, the largest, about 44 MB). The
/// archive is read as a stream to check it, never unpacked in memory; this bounds the work.
pub const MAX_UNPACKED_BYTES: u64 = 512 << 20;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// An archive that passed `check`: only `check` makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    identity: String,
    archive_sha256: String,
    archive: Vec<u8>,
    /// The archive's files, the manifest first.
    pub files: usize,
    pub unpacked_bytes: u64,
}

impl Checked {
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }
}

/// Checks an archive against the identity it is sent under. Reads the whole gzip stream (its CRC
/// and length are checked at the end, as for a bundle, C-042) without keeping it.
pub fn check(archive: Vec<u8>, identity: &str) -> Result<Checked, String> {
    if !is_digest(identity) {
        return Err(format!("{identity} is not a SHA-256 in lowercase hex"));
    }
    if archive.is_empty() {
        return Err("an empty archive".into());
    }
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(format!("an archive is at most {MAX_ARCHIVE_BYTES} bytes; this one is {}", archive.len()));
    }
    let (mut files, mut unpacked) = (0usize, 0u64);
    let bad = |e: std::io::Error| format!("not a gzip tar: {e}");
    let mut gz = GzDecoder::new(archive.as_slice());
    {
        let mut tar = tar::Archive::new(&mut gz);
        for entry in tar.entries().map_err(bad)? {
            let mut entry = entry.map_err(bad)?;
            let name = entry.path().map_err(bad)?.to_string_lossy().into_owned();
            if !entry.header().entry_type().is_file() {
                return Err(format!("{name}: an archive holds files only"));
            }
            let safe = !name.is_empty() && !name.starts_with('/') && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
            if !safe {
                return Err(format!("{name}: not a plain relative name"));
            }
            if (files == 0) != (name == MANIFEST) {
                return Err(format!("the first entry, and only it, is {MANIFEST} (entry {files} is {name})"));
            }
            unpacked += entry.size();
            if unpacked > MAX_UNPACKED_BYTES {
                return Err(format!("an archive unpacks to at most {MAX_UNPACKED_BYTES} bytes"));
            }
            if files == 0 {
                let mut manifest = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut manifest).map_err(bad)?;
                if sha256_hex(&manifest) != identity {
                    return Err(format!("the archive's {MANIFEST} hashes to {}, not to its identity {identity}", sha256_hex(&manifest)));
                }
            } else {
                std::io::copy(&mut entry, &mut std::io::sink()).map_err(bad)?;
            }
            files += 1;
        }
    }
    if files == 0 {
        return Err(format!("an archive without {MANIFEST}"));
    }
    // what follows the tar's end-of-archive blocks, to the gzip trailer (its CRC and length)
    std::io::copy(&mut (&mut gz).take(1 << 20), &mut std::io::sink()).map_err(bad)?;
    if gz.read(&mut [0u8; 1]).map_err(bad)? != 0 {
        return Err("more than a MiB after the archive's end".into());
    }
    Ok(Checked { identity: identity.to_owned(), archive_sha256: sha256_hex(&archive), archive, files, unpacked_bytes: unpacked })
}

fn is_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z`: an instant in UTC. PostgreSQL also reads words such as
/// "now" as times, so what reaches it is checked first (as `core_contract::is_date` for dates).
pub fn is_instant(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let fixed = b.len() >= 20
        && text.get(..10).is_some_and(core_contract::is_date)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[b.len() - 1] == b'Z';
    if !fixed {
        return false;
    }
    let fraction = &b[19..b.len() - 1];
    fraction.is_empty() || (fraction.len() >= 2 && fraction.len() <= 10 && fraction[0] == b'.' && fraction[1..].iter().all(u8::is_ascii_digit))
}

/// What a worker sends for a capture: the checked archive and when its capture wrote the
/// manifest, or why it was not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    Archive { checked: Checked, captured_at: String },
    Absent(String),
}

/// Stores a capture of `session` under a day block's lease. The lease's job must have published a
/// day (its base, from its own build) on or after `session`, among its sessions done or under way:
/// a day's captures are kept only once the day is published, and a day reads only sessions up to
/// its own. True when something new was stored: the same identity again is answered with false;
/// the same identity with other bytes is refused (the first is kept); another identity is another
/// version. An absence is recorded each time it is reported.
pub async fn put(client: &mut Client, token: &str, session: &str, kind: &str, capture: &Capture) -> Result<bool, LeaseError> {
    if !core_contract::is_date(session) {
        return Err(LeaseError::Refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    if !KINDS.contains(&kind) {
        return Err(LeaseError::Refused(format!("{kind} is not a kind of input capture ({})", KINDS.join(", "))));
    }
    match capture {
        Capture::Archive { captured_at, .. } if !is_instant(captured_at) => {
            return Err(LeaseError::Refused(format!("captured_at {captured_at} is not an instant written YYYY-MM-DDTHH:MM:SS[.f]Z")));
        }
        Capture::Absent(reason) if reason.trim().is_empty() => return Err(LeaseError::Refused("an absence needs its reason".into())),
        _ => {}
    }
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    if lease.kind != "DAY_BLOCK" {
        return Err(LeaseError::Refused(format!("job {} is not a day block", lease.job_id)));
    }
    // C-074: a compare-only day block keeps nothing (it publishes nothing either)
    if crate::publish::compare_only(&tx, &lease).await.map_err(|e| LeaseError::Failed(e.to_string()))? {
        return Err(LeaseError::Refused(format!("job {} is a compare-only day block: it keeps nothing", lease.job_id)));
    }
    let published: Option<String> = tx
        .query_one(
            "SELECT max(represented_date)::text FROM vyreon.day_revision
             WHERE profile_id = $1 AND engine_build = $2 AND state = 'PUBLISHED' AND base_revision IS NULL
               AND represented_date BETWEEN $3::text::date AND COALESCE($4::text::date, $5::text::date)",
            &[&lease.profile_id, &lease.engine_build, &lease.first_session, &lease.next_session, &lease.last_session],
        )
        .await
        .map_err(failed)?
        .get(0);
    match published.as_deref() {
        None => return Err(LeaseError::Refused(format!("job {} has published no day: a day's captures are kept once it is published", lease.job_id))),
        Some(day) if session > day => {
            return Err(LeaseError::Refused(format!("{session} is after {day}, the latest day job {} has published", lease.job_id)));
        }
        Some(_) => {}
    }
    if tx.query_opt("SELECT FROM core.session WHERE session = $1::text::date", &[&session]).await.map_err(failed)?.is_none() {
        return Err(LeaseError::Refused(format!("{session} is not a session of the calendar core holds")));
    }
    let new = match capture {
        Capture::Absent(reason) => {
            tx.execute(
                "INSERT INTO core.input_capture (session, kind, absence, job_id, engine_build, worker) VALUES ($1::text::date, $2, $3, $4, $5, $6)",
                &[&session, &kind, reason, &lease.job_id, &lease.engine_build, &lease.worker],
            )
            .await
            .map_err(failed)?;
            true
        }
        Capture::Archive { checked, captured_at } => {
            let inserted = tx
                .execute(
                    "INSERT INTO core.input_capture (session, kind, identity, archive_sha256, archive, captured_at, job_id, engine_build, worker)
                     VALUES ($1::text::date, $2, $3, $4, $5, $6::text::timestamptz, $7, $8, $9) ON CONFLICT (session, kind, identity) DO NOTHING",
                    &[&session, &kind, &checked.identity, &checked.archive_sha256, &checked.archive, captured_at, &lease.job_id, &lease.engine_build, &lease.worker],
                )
                .await
                .map_err(failed)?;
            if inserted == 0 {
                let held: String = tx
                    .query_one(
                        "SELECT archive_sha256 FROM core.input_capture WHERE session = $1::text::date AND kind = $2 AND identity = $3",
                        &[&session, &kind, &checked.identity],
                    )
                    .await
                    .map_err(failed)?
                    .get(0);
                if held != checked.archive_sha256 {
                    return Err(LeaseError::Refused(format!(
                        "{kind} {session} {} is already kept as an archive of SHA-256 {held}; this one is {} (the first is kept)",
                        checked.identity, checked.archive_sha256
                    )));
                }
            }
            inserted == 1
        }
    };
    tx.commit().await.map_err(failed)?;
    Ok(new)
}

/// Every capture and recorded absence of a session, by kind, oldest first: for an archive its
/// identity, the archive's SHA-256 and size, when it was captured and stored, and by which job,
/// build and worker; for an absence its reason.
pub async fn list(client: &Client, session: &str) -> Result<Vec<Value>, String> {
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
    let rows = client
        .query(
            "SELECT kind, identity, archive_sha256, octet_length(archive)::bigint, absence,
                    to_char(captured_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    to_char(stored_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), job_id, engine_build, worker
             FROM core.input_capture WHERE session = $1::text::date ORDER BY kind, stored_at, input_capture_id",
            &[&session],
        )
        .await
        .map_err(db)?;
    Ok(rows
        .iter()
        .map(|r| {
            let mut v = json!({
                "kind": r.get::<_, String>(0), "stored_at": r.get::<_, String>(6), "job_id": r.get::<_, i64>(7),
                "engine_build": r.get::<_, String>(8), "worker": r.get::<_, String>(9),
            });
            match r.get::<_, Option<String>>(4) {
                Some(absence) => v["absence"] = json!(absence),
                None => {
                    v["identity"] = json!(r.get::<_, Option<String>>(1));
                    v["archive_sha256"] = json!(r.get::<_, Option<String>>(2));
                    v["archive_bytes"] = json!(r.get::<_, Option<i64>>(3));
                    v["captured_at"] = json!(r.get::<_, Option<String>>(5));
                }
            }
            v
        })
        .collect())
}

/// One kept archive, as it was sent; None when there is none. Checked against its stored digest.
pub async fn archive(client: &Client, session: &str, kind: &str, identity: &str) -> Result<Option<Vec<u8>>, String> {
    if !core_contract::is_date(session) || !KINDS.contains(&kind) || !is_digest(identity) {
        return Ok(None);
    }
    let row = client
        .query_opt(
            "SELECT archive, archive_sha256 FROM core.input_capture WHERE session = $1::text::date AND kind = $2 AND identity = $3",
            &[&session, &kind, &identity],
        )
        .await
        .map_err(db)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let (archive, digest): (Vec<u8>, String) = (row.get(0), row.get(1));
            if sha256_hex(&archive) != digest {
                return Err(format!("{kind} {session} {identity}: the stored archive does not hash to its digest"));
            }
            Ok(Some(archive))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_are_written_in_utc() {
        for good in ["2026-11-16T04:49:17Z", "2026-11-16T04:49:17.420677Z", "2026-11-16T04:49:17.4Z"] {
            assert!(is_instant(good), "{good}");
        }
        for bad in ["now", "2026-11-16", "2026-11-16 04:49:17Z", "2026-11-16T04:49:17", "2026-11-16T04:49:17+00:00", "2026-11-16T04:49:17.Z", "yesterdayT04:49:17Z", "2026-11-16T04:49:1éZ"] {
            assert!(!is_instant(bad), "{bad}");
        }
    }

    #[test]
    fn an_identity_is_a_lowercase_digest() {
        assert!(is_digest(&"a".repeat(64)));
        assert!(!is_digest(&"A".repeat(64)) && !is_digest(&"a".repeat(63)) && !is_digest(&"g".repeat(64)));
    }
}
