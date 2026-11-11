//! Tape caches (migration 0006, C-010): one asset-session's reduced tape, stored as the worker
//! packed it, written once.
use core_contract::sha256_hex;
use crate::work::{self, LeaseError};
use tokio_postgres::{Client, GenericClient};

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// A cache's content: the packed body, or why there is none for the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cache {
    Body(Vec<u8>),
    Absent(String),
}

/// Stores one asset-session's cache. Storing the same content again changes nothing (true is
/// returned only when it was new); other content for the same key is refused.
pub async fn put(client: &Client, asset: &str, session: &str, method: &str, cache: &Cache) -> Result<bool, String> {
    let (body, digest, absence) = match cache {
        Cache::Body(bytes) if bytes.is_empty() => return Err("an empty tape cache".into()),
        Cache::Body(bytes) => (Some(bytes.as_slice()), Some(sha256_hex(bytes)), None),
        Cache::Absent(reason) if reason.trim().is_empty() => return Err("an absence needs its reason".into()),
        Cache::Absent(reason) => (None, None, Some(reason.as_str())),
    };
    let new = client
        .execute(
            "INSERT INTO core.tape_cache (asset_key, session, method_version, body_sha256, body, absence)
             VALUES ($1, $2::text::date, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            &[&asset, &session, &method, &digest, &body, &absence],
        )
        .await
        .map_err(db)?;
    if new == 1 {
        return Ok(true);
    }
    let same: bool = client
        .query_one(
            "SELECT body_sha256 IS NOT DISTINCT FROM $4 AND absence IS NOT DISTINCT FROM $5 FROM core.tape_cache
             WHERE asset_key = $1 AND session = $2::text::date AND method_version = $3",
            &[&asset, &session, &method, &digest, &absence],
        )
        .await
        .map_err(db)?
        .get(0);
    if same { Ok(false) } else { Err(format!("{asset} {session} {method} is already stored with other content")) }
}

/// Every asset's cache of a session for a method version, by asset key.
pub async fn of_session(client: &Client, session: &str, method: &str) -> Result<Vec<(String, Cache)>, String> {
    let rows = client
        .query(
            "SELECT asset_key, body, body_sha256, absence FROM core.tape_cache
             WHERE session = $1::text::date AND method_version = $2 ORDER BY asset_key COLLATE \"C\"",
            &[&session, &method],
        )
        .await
        .map_err(db)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (asset, body, digest, absence): (String, Option<Vec<u8>>, Option<String>, Option<String>) = (row.get(0), row.get(1), row.get(2), row.get(3));
        let cache = match (body, absence) {
            (Some(body), _) if Some(sha256_hex(&body)) == digest => Cache::Body(body),
            (Some(_), _) => return Err(format!("{asset} {session}: the stored tape cache does not hash to its digest")),
            (None, Some(reason)) => Cache::Absent(reason),
            (None, None) => return Err(format!("{asset} {session}: an empty row")),
        };
        out.push((asset, cache));
    }
    Ok(out)
}

/// The profile's members listed on the session that have no cache or absence for it yet.
pub async fn missing(db_client: &impl GenericClient, profile: &str, session: &str) -> Result<Vec<String>, String> {
    let rows = db_client
        .query(
            "SELECT m.asset_key FROM core.members_on($1, $2::text::date) m
             WHERE m.ticker IS NOT NULL
               AND NOT EXISTS (SELECT FROM core.tape_cache t WHERE t.session = $2::text::date AND t.asset_key = m.asset_key)
             ORDER BY m.ordinal",
            &[&profile, &session],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Marks a tape job's session done, under its lease: every member listed on the session must have
/// its cache, or its stated absence, stored.
pub async fn session_done(client: &mut Client, token: &str, session: &str) -> Result<(), LeaseError> {
    let tx = client.transaction().await.map_err(|e| LeaseError::Failed(db(e)))?;
    let lease = work::held(&tx, token).await?;
    if lease.kind != "TAPE" {
        return Err(LeaseError::Refused(format!("job {} is not a tape job", lease.job_id)));
    }
    let listed: i64 = tx
        .query_one("SELECT count(*) FROM core.members_on($1, $2::text::date) WHERE ticker IS NOT NULL", &[&lease.profile_id, &session])
        .await
        .map_err(|e| LeaseError::Failed(db(e)))?
        .get(0);
    if listed == 0 {
        return Err(LeaseError::Refused(format!("profile {} has no member listed on {session}: is the universe loaded?", lease.profile_id)));
    }
    let missing = missing(&tx, &lease.profile_id, session).await.map_err(LeaseError::Failed)?;
    if !missing.is_empty() {
        return Err(LeaseError::Refused(format!("{} of {listed} members have no tape cache for {session}: {}", missing.len(), missing.join(" "))));
    }
    work::session_done(&tx, &lease, session).await?;
    tx.commit().await.map_err(|e| LeaseError::Failed(db(e)))
}
