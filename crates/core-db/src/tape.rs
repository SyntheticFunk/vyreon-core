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
pub async fn put(client: &impl GenericClient, asset: &str, session: &str, method: &str, cache: &Cache) -> Result<bool, String> {
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
    // C-059: a verify job's session is done when every member listed on it has its check
    if is_verify(&tx, lease.job_id).await.map_err(LeaseError::Failed)? {
        let unchecked: Vec<String> = tx
            .query(
                "SELECT m.asset_key FROM core.members_on($1, $2::text::date) m
                 WHERE m.ticker IS NOT NULL
                   AND NOT EXISTS (SELECT FROM core.tape_check c WHERE c.job_id = $3 AND c.session = $2::text::date AND c.asset_key = m.asset_key)
                 ORDER BY m.ordinal",
                &[&lease.profile_id, &session, &lease.job_id],
            )
            .await
            .map_err(|e| LeaseError::Failed(db(e)))?
            .iter()
            .map(|r| r.get(0))
            .collect();
        if !unchecked.is_empty() {
            return Err(LeaseError::Refused(format!("{} members listed on {session} have no check from verify job {}: {}", unchecked.len(), lease.job_id, unchecked.join(" "))));
        }
        work::session_done(&tx, &lease, session).await?;
        return tx.commit().await.map_err(|e| LeaseError::Failed(db(e)));
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
pub async fn put_verdict(client: &impl GenericClient, asset: &str, session: &str, method: &str, v: &Verdict) -> Result<bool, String> {
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

/// Every asset's current verdict of a session for a method version, by asset key, as its reader
/// sent it: the latest reader's (C-058 amended 2026-11-21, `refcheck::CURRENT_VERDICTS`; the day
/// check reads the current verdict), the kept one or a sample's on the same bytes.
pub async fn verdicts_of_session(client: &Client, session: &str, method: &str) -> Result<Vec<(String, serde_json::Value)>, String> {
    if !core_contract::is_date(session) {
        return Err(format!("{session} is not a date written YYYY-MM-DD"));
    }
    let rows = client
        .query(
            &format!(
                "WITH {}
                 SELECT asset_key, document::text FROM current
                 WHERE session = $2::text::date AND method_version = $3 ORDER BY asset_key COLLATE \"C\"",
                crate::refcheck::CURRENT_VERDICTS
            ),
            &[&"infinity", &session, &method],
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

/// C-059: a verify job's scope (a TAPE job that recomputes the caches core holds and stores none).
pub const VERIFY_SCOPE: &str = r#"{"verify": true}"#;
/// C-059: a verify job's report on one member's cache (engine `tape_check::SCHEMA`).
pub const CHECK_SCHEMA: &str = "VYREON-TAPE-CHECK@1";
/// The most bytes a check may hold: its verdict (at most `MAX_VERDICT_BYTES`) and the differences.
pub const MAX_CHECK_BYTES: usize = 2 << 20;

/// Whether `job` is a verify job (C-059).
pub async fn is_verify(c: &impl GenericClient, job: i64) -> Result<bool, String> {
    Ok(c
        .query_opt("SELECT COALESCE(scope @> $2::text::jsonb, false) FROM core.job WHERE job_id = $1", &[&job, &VERIFY_SCOPE])
        .await
        .map_err(db)?
        .is_some_and(|r| r.get(0)))
}

/// A check that passed `check_check`: only it makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// The worker's recomputed cache; None: it recomputed none (`why`).
    pub recomputed_sha256: Option<String>,
    pub why: Option<String>,
    /// The session's replay files as the recomputation downloaded them, in bytes.
    pub tape_bytes: Option<i64>,
    /// The tape reader's verdict on the recomputed cache.
    pub verdict: Option<Verdict>,
    document: String,
}

/// Checks a verify job's report on `asset`'s cache of `session` under `method`: a JSON object
/// within the size bound with no unredacted credential, of the schema, naming the same asset,
/// session and method version; the recomputed cache's digest, or none with why; the session's
/// downloaded bytes when stated; the verdict, when there is one, a verdict on that recomputed
/// cache (`check_verdict`).
pub fn check_check(bytes: &[u8], asset: &str, session: &str, method: &str) -> Result<Check, String> {
    if bytes.len() > MAX_CHECK_BYTES {
        return Err(format!("a tape check of {} bytes (at most {MAX_CHECK_BYTES})", bytes.len()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "a tape check that is not UTF-8".to_owned())?;
    if let Some(marker) = crate::evidence::unredacted(text) {
        return Err(format!("the tape check holds an unredacted {marker}"));
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("a tape check that is not JSON: {e}"))?;
    let field = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
    if field("schema") != Some(CHECK_SCHEMA) {
        return Err(format!("a tape check's schema must be {CHECK_SCHEMA}"));
    }
    for (k, want) in [("asset", asset), ("session", session), ("method_version", method)] {
        if field(k) != Some(want) {
            return Err(format!("a tape check of {k} {:?}, sent for {want}", field(k)));
        }
    }
    let why = field("why").filter(|w| !w.trim().is_empty()).map(str::to_owned);
    let recomputed = match v.get("recomputed_sha256") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) if hex64(s) => Some(s.clone()),
        Some(_) => return Err("a tape check's recomputed_sha256 is a sha256 or null".into()),
    };
    if recomputed.is_none() && why.is_none() {
        return Err("a tape check that recomputed no cache says why".into());
    }
    let tape_bytes = match v.get("tape_bytes") {
        None | Some(serde_json::Value::Null) => None,
        Some(n) => Some(n.as_i64().filter(|n| *n >= 0).ok_or("a tape check's tape_bytes is a count of bytes")?),
    };
    let verdict = match v.get("verdict") {
        None | Some(serde_json::Value::Null) => None,
        Some(d @ serde_json::Value::Object(_)) => {
            let verdict = check_verdict(&serde_json::to_vec(d).map_err(|e| e.to_string())?, asset, session, method)?;
            if Some(&verdict.cache_sha256) != recomputed.as_ref() {
                return Err("a tape check's verdict judged another cache than the one it recomputed".into());
            }
            Some(verdict)
        }
        Some(_) => return Err("a tape check's verdict is an object or null".into()),
    };
    Ok(Check { recomputed_sha256: recomputed, why, tape_bytes, verdict, document: text.to_owned() })
}

/// What core made of a check: its outcome, whether the verdict was kept beside the stored cache,
/// why (when it was not compared, or the verdict not kept), and whether the check was new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub outcome: String,
    pub verdict_kept: bool,
    pub why: Option<String>,
    pub new: bool,
}

/// C-059: stores a verify job's check of `asset`'s cache of `session` under `method`, under the
/// job's lease (the session must be its next, the asset listed on it). Core compares the
/// recomputed cache's digest with the cache it holds: MATCH when equal, and the verdict is then kept
/// beside the stored cache (C-058; one already there stands); DIFFER when not (the verdict stays in
/// the check only); NOT_COMPARED when either side has no cache. The stored cache is never touched.
/// One check per job and asset-session: another sent for it changes nothing (`new: false`).
pub async fn put_check(client: &mut Client, token: &str, session: &str, asset: &str, method: &str, c: &Check) -> Result<Checked, LeaseError> {
    let failed = |e: String| LeaseError::Failed(e);
    let tx = client.transaction().await.map_err(|e| failed(db(e)))?;
    let lease = work::held(&tx, token).await?;
    if lease.kind != "TAPE" || !is_verify(&tx, lease.job_id).await.map_err(failed)? || lease.next_session.as_deref() != Some(session) {
        return Err(LeaseError::Refused(format!("job {} is not the verify job of {session}", lease.job_id)));
    }
    let listed = tx
        .query_opt("SELECT FROM core.members_on($1, $2::text::date) WHERE asset_key = $3 AND ticker IS NOT NULL", &[&lease.profile_id, &session, &asset])
        .await
        .map_err(|e| failed(db(e)))?;
    if listed.is_none() {
        return Err(LeaseError::Refused(format!("{asset} is not listed on {session} in profile {}", lease.profile_id)));
    }
    if let Some(r) = tx
        .query_opt(
            "SELECT outcome, verdict_kept, why FROM core.tape_check WHERE job_id = $1 AND session = $2::text::date AND asset_key = $3",
            &[&lease.job_id, &session, &asset],
        )
        .await
        .map_err(|e| failed(db(e)))?
    {
        return Ok(Checked { outcome: r.get(0), verdict_kept: r.get(1), why: r.get(2), new: false });
    }
    let stored = tx
        .query_opt(
            "SELECT body_sha256, absence FROM core.tape_cache WHERE asset_key = $1 AND session = $2::text::date AND method_version = $3",
            &[&asset, &session, &method],
        )
        .await
        .map_err(|e| failed(db(e)))?
        .map(|r| (r.get::<_, Option<String>>(0), r.get::<_, Option<String>>(1)));
    let stored_sha256 = stored.as_ref().and_then(|s| s.0.clone());
    let (outcome, mut why) = match (&c.recomputed_sha256, &stored) {
        (None, _) => ("NOT_COMPARED", c.why.clone()),
        (Some(_), None) => ("NOT_COMPARED", Some(format!("core holds no tape cache of {asset} {session} under {method}"))),
        (Some(_), Some((None, absence))) => ("NOT_COMPARED", Some(format!("core holds a stated absence: {}", absence.as_deref().unwrap_or("")))),
        (Some(r), Some((Some(s), _))) if r == s => ("MATCH", None),
        (Some(_), Some(_)) => ("DIFFER", Some(c.why.clone().unwrap_or_else(|| "the recomputed cache's digest differs from the stored one".into()))),
    };
    let mut kept = false;
    if outcome == "MATCH"
        && let Some(v) = &c.verdict
    {
        let already = tx
            .query_opt("SELECT outcome FROM core.tape_verdict WHERE session = $1::text::date AND asset_key = $2 AND method_version = $3", &[&session, &asset, &method])
            .await
            .map_err(|e| failed(db(e)))?;
        match already {
            Some(r) => why = Some(format!("a {} verdict already stored beside the cache stands", r.get::<_, String>(0))),
            None => kept = put_verdict(&tx, asset, session, method, v).await.map_err(failed)?,
        }
    }
    tx.execute(
        "INSERT INTO core.tape_check (job_id, lease_id, session, asset_key, method_version, stored_sha256, recomputed_sha256, outcome, verdict_outcome, verdict_kept, why, tape_bytes, document)
         VALUES ($1, $2, $3::text::date, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::text::jsonb)",
        &[
            &lease.job_id,
            &lease.lease_id,
            &session,
            &asset,
            &method,
            &stored_sha256,
            &c.recomputed_sha256,
            &outcome,
            &c.verdict.as_ref().map(|v| v.outcome.clone()),
            &kept,
            &why,
            &c.tape_bytes,
            &c.document,
        ],
    )
    .await
    .map_err(|e| failed(db(e)))?;
    tx.commit().await.map_err(|e| failed(db(e)))?;
    Ok(Checked { outcome: outcome.to_owned(), verdict_kept: kept, why, new: true })
}

/// C-059: the sample, one entry per session checked, oldest first: the members listed on it (the
/// checking job's profile), each member's latest check counted by outcome, the verdicts of the
/// MATCH checks by outcome (a verdict on a recomputed cache that differs from the stored one names
/// other bytes: it stays in that check's row, never in these counts; round cc, 10), the replay
/// bytes downloaded, and whether the session passed: every member listed has a latest check that
/// is MATCH with a PASS verdict (WITHHELD for a capture withheld whole) (round cc, 7: a job that
/// stopped after one member never passes its session); with every listed member not passed (a
/// member without a check as NOT_CHECKED).
pub async fn sample(c: &impl GenericClient) -> Result<Vec<serde_json::Value>, String> {
    let listed_rows = c
        .query(
            "SELECT s.session::text, m.asset_key
             FROM (SELECT DISTINCT c.session, j.profile_id FROM core.tape_check c JOIN core.job j USING (job_id)) s,
                  core.members_on(s.profile_id, s.session) m
             WHERE m.ticker IS NOT NULL ORDER BY 1, 2",
            &[],
        )
        .await
        .map_err(db)?;
    let mut listed: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = std::collections::BTreeMap::new();
    for r in &listed_rows {
        listed.entry(r.get(0)).or_default().insert(r.get(1));
    }
    let rows = c
        .query(
            "SELECT DISTINCT ON (session, asset_key) session::text, asset_key, outcome, verdict_outcome, why, tape_bytes, job_id,
                    to_char(checked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             FROM core.tape_check ORDER BY session, asset_key, checked_at DESC, tape_check_id DESC",
            &[],
        )
        .await
        .map_err(db)?;
    let mut out: Vec<serde_json::Value> = Vec::new();
    for r in &rows {
        let (session, asset, outcome): (String, String, String) = (r.get(0), r.get(1), r.get(2));
        let (verdict, why, bytes): (Option<String>, Option<String>, Option<i64>) = (r.get(3), r.get(4), r.get(5));
        let (job, at): (i64, String) = (r.get(6), r.get(7));
        if out.last().is_none_or(|s| s["session"] != session.as_str()) {
            let n = listed.get(&session).map_or(0, |l| l.len());
            out.push(serde_json::json!({"session": session, "listed": n, "checked": 0, "unchecked": 0, "MATCH": 0, "DIFFER": 0, "NOT_COMPARED": 0, "PASS": 0, "FAIL": 0,
                                        "WITHHELD": 0, "NOT_RUN": 0, "no_verdict": 0, "passed_members": 0, "tape_bytes": null, "jobs": [],
                                        "last_checked_at": at, "not_passed": [], "seen": []}));
        }
        let s = out.last_mut().expect("pushed");
        let bump = |s: &mut serde_json::Value, k: &str| s[k] = serde_json::json!(s[k].as_i64().unwrap_or(0) + 1);
        bump(s, "checked");
        bump(s, &outcome);
        if outcome == "MATCH" {
            bump(s, verdict.as_deref().unwrap_or("no_verdict"));
        }
        s["seen"].as_array_mut().expect("a list").push(serde_json::json!(asset));
        let is_listed = listed.get(&session).is_some_and(|l| l.contains(&asset));
        let passed = is_listed && outcome == "MATCH" && matches!(verdict.as_deref(), Some("PASS" | "WITHHELD"));
        if passed {
            bump(s, "passed_members");
        } else {
            s["not_passed"].as_array_mut().expect("a list").push(serde_json::json!({"asset": asset, "outcome": outcome, "verdict": verdict, "why": why}));
        }
        if let Some(b) = bytes {
            s["tape_bytes"] = serde_json::json!(s["tape_bytes"].as_i64().unwrap_or(0).max(b));
        }
        if !s["jobs"].as_array().expect("a list").contains(&serde_json::json!(job)) {
            s["jobs"].as_array_mut().expect("a list").push(serde_json::json!(job));
        }
        if s["last_checked_at"].as_str().is_some_and(|l| l < at.as_str()) {
            s["last_checked_at"] = serde_json::json!(at);
        }
    }
    for s in &mut out {
        let seen: Vec<String> = s["seen"].as_array().expect("a list").iter().filter_map(|a| a.as_str().map(str::to_owned)).collect();
        let session = s["session"].as_str().unwrap_or_default().to_owned();
        for asset in listed.get(&session).into_iter().flatten().filter(|a| !seen.contains(a)) {
            s["not_passed"].as_array_mut().expect("a list").push(serde_json::json!({"asset": asset, "outcome": "NOT_CHECKED", "verdict": null, "why": "no check of this member"}));
            s["unchecked"] = serde_json::json!(s["unchecked"].as_i64().unwrap_or(0) + 1);
        }
        s.as_object_mut().expect("an object").remove("seen");
        s["passed"] = serde_json::json!(s["listed"].as_i64() > Some(0) && s["passed_members"] == s["listed"]);
    }
    Ok(out)
}

fn thousands(n: i64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// C-059: the sample in one line (the digest's and `tape checks`'); None before any check.
pub fn sample_line(sessions: &[serde_json::Value]) -> Option<String> {
    if sessions.is_empty() {
        return None;
    }
    let sum = |k: &str| sessions.iter().map(|s| s[k].as_i64().unwrap_or(0)).sum::<i64>();
    let passed = sessions.iter().filter(|s| s["passed"] == true).count();
    let (members, ok) = (sum("listed"), sum("passed_members"));
    let mut misses = Vec::new();
    for (n, words) in [
        (sum("DIFFER"), "differ from the stored cache"),
        (sum("MATCH") - sum("PASS") - sum("WITHHELD"), "recomputed equal, the tape reader not agreeing"),
        (sum("NOT_COMPARED"), "not compared"),
        (sum("unchecked"), "not checked"),
    ] {
        if n > 0 {
            misses.push(format!("{} {words}", thousands(n)));
        }
    }
    let misses = if misses.is_empty() { String::new() } else { format!("; {}", misses.join(", ")) };
    Some(format!(
        "Tape cache code verified by sample (C-059): {passed} of {} sessions passed ({} of {} asset-sessions recomputed byte for byte with the tape reader agreeing{misses})",
        sessions.len(),
        thousands(ok),
        thousands(members)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C-059: the sample's line counts sessions and asset-sessions, and names each kind of miss
    /// only when there is one (the counts are a sample's as `sample` gives them).
    #[test]
    fn the_sample_line_counts_never_lists() {
        assert_eq!(sample_line(&[]), None);
        let session = |checked: i64, matched: i64, pass: i64, withheld: i64, differ: i64| {
            serde_json::json!({"listed": 64, "checked": checked, "unchecked": 64 - checked, "MATCH": matched, "PASS": pass, "WITHHELD": withheld, "DIFFER": differ,
                               "NOT_COMPARED": checked - matched - differ, "passed_members": pass + withheld,
                               "passed": pass + withheld == 64})
        };
        let all: Vec<_> = (0..20).map(|i| session(64, 64, 64 - i % 2, i % 2, 0)).collect();
        assert_eq!(
            sample_line(&all).unwrap(),
            "Tape cache code verified by sample (C-059): 20 of 20 sessions passed (1,280 of 1,280 asset-sessions recomputed byte for byte with the tape reader agreeing)"
        );
        let mut some = all.clone();
        some[3] = session(64, 62, 61, 0, 2);
        assert_eq!(
            sample_line(&some).unwrap(),
            "Tape cache code verified by sample (C-059): 19 of 20 sessions passed (1,277 of 1,280 asset-sessions recomputed byte for byte with the tape reader agreeing; 2 differ from the stored cache, 1 recomputed equal, the tape reader not agreeing)"
        );
        // round cc, 7: a job that stopped after one member (AAPL) passes no session
        some[5] = session(1, 1, 1, 0, 0);
        assert_eq!(
            sample_line(&some).unwrap(),
            "Tape cache code verified by sample (C-059): 18 of 20 sessions passed (1,214 of 1,280 asset-sessions recomputed byte for byte with the tape reader agreeing; 2 differ from the stored cache, 1 recomputed equal, the tape reader not agreeing, 63 not checked)"
        );
    }
}

/// C-083 (engine D-472, D-480: a new tape-reader identity; every tape is captured again): what
/// `recapture` queued.
#[derive(Debug, PartialEq, Eq)]
pub struct Recaptured {
    pub first: String,
    pub last: String,
    /// One TAPE job per session queued now.
    pub jobs: u64,
    /// Sessions in the range left as they are: a TAPE job of this build already holds them (any state
    /// but cancelled), or an unfinished one of another build (the hand-over carries it).
    pub kept: i64,
}

/// C-083: every session's tape captured again under `build` (whose reader identity is new): one TAPE job a
/// session (scope `{"recapture": true}`, ordinary priority, claimed oldest first), from the replay files'
/// first session (`core.source_coverage`, 2023-05-24) or `first`, to the last session before today in New
/// York (today's is the nightly's) or `last`. Day blocks wait for every tape up to their last session
/// (`core.claimable_job`), so the backfill resumes behind the re-capture; a republish reading the tapes
/// waits too. Run again, it queues only what is missing. The recompute of the tape families' published days
/// is C-079's, planned once the build publishes a whole day.
pub async fn recapture(client: &mut Client, profile: &str, build: &str, first: Option<&str>, last: Option<&str>) -> Result<Recaptured, String> {
    for d in [first, last].into_iter().flatten() {
        if !core_contract::is_date(d) {
            return Err(format!("{d} is not a date written YYYY-MM-DD"));
        }
    }
    let runs: Option<bool> = client.query_opt("SELECT core.build_runs($1) FROM core.engine_build WHERE engine_build = $1", &[&build]).await.map_err(db)?.map(|r| r.get(0));
    match runs {
        None => return Err(format!("engine build {build} is not registered")),
        Some(false) => return Err(format!("engine build {build} is not admitted, or withdrawn: no worker would capture its tapes")),
        Some(true) => {}
    }
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    let range = tx
        .query_one(
            "SELECT COALESCE($1::text::date, (SELECT first_date FROM core.source_coverage WHERE source = 'replay_tapes'))::text,
                    COALESCE($2::text::date, (SELECT max(session) FROM core.session WHERE session < (now() AT TIME ZONE 'America/New_York')::date))::text",
            &[&first, &last],
        )
        .await
        .map_err(db)?;
    let (from, to): (Option<String>, Option<String>) = (range.get(0), range.get(1));
    let (from, to) = (from.ok_or("no first session: core.source_coverage has no replay_tapes")?, to.ok_or("no session before today")?);
    if from > to {
        return Err(format!("{from} is after {to}: no session to capture"));
    }
    let held = "EXISTS (SELECT FROM core.job t WHERE t.kind = 'TAPE' AND t.first_session = s.session AND t.profile_id = $1
                         AND NOT COALESCE(t.scope @> '{\"verify\": true}', false)
                         AND ((t.engine_build = $2 AND t.state <> 'CANCELLED') OR t.state IN ('QUEUED', 'LEASED', 'FAILED')))";
    let kept: i64 = tx
        .query_one(&format!("SELECT count(*) FROM core.session s WHERE s.session BETWEEN $3::text::date AND $4::text::date AND {held}"), &[&profile, &build, &from, &to])
        .await
        .map_err(db)?
        .get(0);
    let jobs = tx
        .execute(
            &format!(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope)
                 SELECT 'TAPE', $1, $2, s.session, s.session, s.session, '{{\"recapture\": true}}'::jsonb
                 FROM core.session s WHERE s.session BETWEEN $3::text::date AND $4::text::date AND NOT {held}
                 ORDER BY s.session"
            ),
            &[&profile, &build, &from, &to],
        )
        .await
        .map_err(db)?;
    if jobs > 0 {
        crate::notice::log(
            &tx,
            &format!("Tape re-capture for build {build} (a new tape-reader identity): {jobs} sessions queued, {from} to {to}; day blocks wait for the tapes up to their last session."),
        )
        .await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Recaptured { first: from, last: to, jobs, kept })
}
