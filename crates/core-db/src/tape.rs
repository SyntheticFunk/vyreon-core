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
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
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
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
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

/// The profile's members listed on the session that have no cache or absence for it yet under the
/// method version (a cache of another method version is not one of this method's).
pub async fn missing(db_client: &impl GenericClient, profile: &str, session: &str, method: &str) -> Result<Vec<String>, String> {
    let rows = db_client
        .query(
            "SELECT m.asset_key FROM core.members_on($1, $2::text::date) m
             WHERE m.ticker IS NOT NULL
               AND NOT EXISTS (SELECT FROM core.tape_cache t WHERE t.session = $2::text::date AND t.asset_key = m.asset_key AND t.method_version = $3)
             ORDER BY m.ordinal",
            &[&profile, &session, &method],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// Marks a tape job's session done, under its lease: every member listed on the session must have
/// its cache, or its stated absence, stored under one method version: `method` when the worker
/// names it, else the method version of the session's most recently stored cache (a session being
/// redone under a new method stores the new one's last).
pub async fn session_done(client: &mut Client, token: &str, session: &str, method: Option<&str>) -> Result<(), LeaseError> {
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
    let method: String = match method {
        Some(method) => method.to_owned(),
        None => tx
            .query_opt("SELECT method_version FROM core.tape_cache WHERE session = $1::text::date ORDER BY stored_at DESC, method_version DESC LIMIT 1", &[&session])
            .await
            .map_err(|e| LeaseError::Failed(db(e)))?
            .map(|r| r.get(0))
            .ok_or_else(|| LeaseError::Refused(format!("no tape cache is stored for {session}")))?,
    };
    let missing = missing(&tx, &lease.profile_id, session, &method).await.map_err(LeaseError::Failed)?;
    if !missing.is_empty() {
        return Err(LeaseError::Refused(format!(
            "{} of {listed} members have no tape cache of method {method} for {session}: {}",
            missing.len(),
            missing.join(" ")
        )));
    }
    work::session_done(&tx, &lease, session).await?;
    tx.commit().await.map_err(|e| LeaseError::Failed(db(e)))
}

/// C-058 (engine D-370 amended 2026-11-20): the tape worker's verdict on a cache it stored, its
/// schema (engine `tape_verdict::SCHEMA`).
pub const VERDICT_SCHEMA: &str = "VYREON-TAPE-VERDICT@1";
/// The most bytes a verdict may hold (a real one is a few kilobytes: 26 bins of differences at most
/// a few hundred entries).
pub const MAX_VERDICT_BYTES: usize = 1 << 20;
pub const VERDICT_OUTCOMES: [&str; 4] = ["PASS", "FAIL", "WITHHELD", "NOT_RUN"];

/// A verdict that passed `check_verdict`: only it makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub outcome: String,
    pub differences: i32,
    pub reader_bins_sha256: Option<String>,
    pub why: Option<String>,
    /// The packed cache it judged: the stored cache's digest.
    pub cache_sha256: String,
    document: String,
}

fn hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Checks a verdict sent for `asset`'s cache of `session` under `method`: a JSON object within the
/// size bound with no unredacted credential, of the schema, naming the same asset, session and
/// method version, a known outcome, the packed cache's digest; the reader's bins digest exactly
/// when it compared (PASS, FAIL); a reason whenever it did not pass; the differences a list.
pub fn check_verdict(bytes: &[u8], asset: &str, session: &str, method: &str) -> Result<Verdict, String> {
    if bytes.len() > MAX_VERDICT_BYTES {
        return Err(format!("a tape verdict of {} bytes (at most {MAX_VERDICT_BYTES})", bytes.len()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "a tape verdict that is not UTF-8".to_owned())?;
    if let Some(marker) = crate::evidence::unredacted(text) {
        return Err(format!("the tape verdict holds an unredacted {marker}"));
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("a tape verdict that is not JSON: {e}"))?;
    let field = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
    if field("schema") != Some(VERDICT_SCHEMA) {
        return Err(format!("a tape verdict's schema must be {VERDICT_SCHEMA}"));
    }
    for (k, want) in [("asset", asset), ("session", session), ("method_version", method)] {
        if field(k) != Some(want) {
            return Err(format!("a tape verdict of {k} {:?}, sent for {want}", field(k)));
        }
    }
    let outcome = field("outcome").filter(|o| VERDICT_OUTCOMES.contains(o)).ok_or_else(|| format!("a tape verdict's outcome must be one of {}", VERDICT_OUTCOMES.join(", ")))?;
    let cache_sha256 = field("cache_sha256").filter(|s| hex64(s)).ok_or("a tape verdict names its packed cache's sha256")?.to_owned();
    let compared = matches!(outcome, "PASS" | "FAIL");
    let reader = field("reader_bins_sha256").map(str::to_owned);
    match (&reader, compared) {
        (Some(r), true) if hex64(r) => {}
        (None, false) => {}
        _ => return Err(format!("a {outcome} tape verdict {} the reader's bins sha256", if compared { "names" } else { "names no" })),
    }
    let why = field("why").filter(|w| !w.trim().is_empty()).map(str::to_owned);
    if outcome != "PASS" && why.is_none() {
        return Err(format!("a {outcome} tape verdict says why"));
    }
    let differences = match v.get("differences") {
        None | Some(serde_json::Value::Null) => 0,
        Some(serde_json::Value::Array(a)) => i32::try_from(a.len()).map_err(|_| "too many differences")?,
        Some(_) => return Err("a tape verdict's differences are a list".into()),
    };
    if outcome == "PASS" && differences > 0 {
        return Err("a PASS tape verdict lists differences".into());
    }
    Ok(Verdict { outcome: outcome.to_owned(), differences, reader_bins_sha256: reader, why, cache_sha256, document: text.to_owned() })
}

/// Stores a verdict beside the cache it judged, once: true when it was new; the same document
/// again changes nothing (false); another for the same cache, a verdict of bytes core does not
/// hold, or one for a stated absence, is refused.
pub async fn put_verdict(client: &Client, asset: &str, session: &str, method: &str, v: &Verdict) -> Result<bool, String> {
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
    let new = client
        .execute(
            "INSERT INTO core.tape_verdict (session, asset_key, method_version, cache_sha256, outcome, differences, reader_bins_sha256, why, document)
             SELECT t.session, t.asset_key, t.method_version, $4, $5, $6, $7, $8, $9::text::jsonb FROM core.tape_cache t
             WHERE t.session = $2::text::date AND t.asset_key = $1 AND t.method_version = $3 AND t.body_sha256 = $4
             ON CONFLICT DO NOTHING",
            &[&asset, &session, &method, &v.cache_sha256, &v.outcome, &v.differences, &v.reader_bins_sha256, &v.why, &v.document],
        )
        .await
        .map_err(db)?;
    if new == 1 {
        return Ok(true);
    }
    let row = client
        .query_opt(
            "SELECT t.body_sha256, v.document = $4::text::jsonb FROM core.tape_cache t
             LEFT JOIN core.tape_verdict v USING (session, asset_key, method_version)
             WHERE t.session = $2::text::date AND t.asset_key = $1 AND t.method_version = $3",
            &[&asset, &session, &method, &v.document],
        )
        .await
        .map_err(db)?;
    match row.map(|r| (r.get::<_, Option<String>>(0), r.get::<_, Option<bool>>(1))) {
        None => Err(format!("{asset} {session} {method}: no tape cache is stored to judge")),
        Some((None, _)) => Err(format!("{asset} {session} {method}: core holds a stated absence, not a cache")),
        Some((Some(d), _)) if d != v.cache_sha256 => Err(format!("{asset} {session} {method}: the verdict judged other bytes than the cache stored")),
        Some((_, Some(true))) => Ok(false),
        Some(_) => Err(format!("{asset} {session} {method}: a verdict is already stored for this cache")),
    }
}

/// Every asset's verdict of a session for a method version, by asset key, as sent.
pub async fn verdicts_of_session(client: &Client, session: &str, method: &str) -> Result<Vec<(String, serde_json::Value)>, String> {
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
    let rows = client
        .query(
            "SELECT asset_key, document::text FROM core.tape_verdict
             WHERE session = $1::text::date AND method_version = $2 ORDER BY asset_key COLLATE \"C\"",
            &[&session, &method],
        )
        .await
        .map_err(db)?;
    rows.iter()
        .map(|r| {
            let (asset, text): (String, String) = (r.get(0), r.get(1));
            serde_json::from_str(&text).map(|v| (asset, v)).map_err(|e| format!("a stored tape verdict: {e}"))
        })
        .collect()
}
