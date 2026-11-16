//! Failure evidence (migration 0017, C-049): what a worker whose attempt failed or was refused
//! leaves with core before it exits, so the decisive error does not go with the pod.
//!
//! The worker sends one document per lease: a gzip whose text is one JSON line, the account
//! (`schema`, `stage`, `message`, `session`, `refusals`), and after it an excerpt of its logs. The
//! worker removes credentials before it sends anything (it alone knows the literal secrets); core
//! refuses a document in which a credential pattern survives (`unredacted`), so a gap in the
//! worker's redaction is a refusal, never a stored secret. Core stores the document as sent.
use crate::work::LeaseError;
use core_contract::sha256_hex;
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::io::Read;
use tokio_postgres::Client;

/// The account's schema, the document's first line.
pub const SCHEMA: &str = "VYREON-JOB-EVIDENCE@1";
/// The most bytes a document may hold as sent. The worker keeps about 4 MiB of its fill log's
/// tail and 1 MiB of each other log, with the lines before them that name an error: some 10 MiB
/// of text at most, about 1 MiB gzipped.
pub const MAX_DOCUMENT_BYTES: usize = 16 << 20;
/// The most bytes a document may unpack to.
pub const MAX_UNPACKED_BYTES: u64 = 64 << 20;
/// What the worker writes in place of a credential.
pub const REDACTED: &str = "[redacted]";
/// Assignments whose value is a credential (matched without regard to case): a query
/// parameter, a `key=value` in a connection string, a presigned URL's signature. The engine's
/// redaction (`vyreon_cli::failure_evidence`) writes `[redacted]` after each.
pub const SECRET_ASSIGNMENTS: [&str; 8] = ["api_key=", "apikey=", "token=", "password=", "x-amz-signature=", "x-amz-credential=", "signature=", "secret="];

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// A document that passed `check`: only `check` makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub stage: String,
    pub message: String,
    pub session: Option<String>,
    pub refusals: Option<Value>,
    document: Vec<u8>,
    document_sha256: String,
    pub excerpt_bytes: u64,
}

/// The first credential pattern a text still carries unredacted, if any: an assignment of
/// `SECRET_ASSIGNMENTS`, a bearer token, a password in a URL (`scheme://user:PASSWORD@host`), or a
/// Discord webhook's token.
pub fn unredacted(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    // after any spaces and an optional opening quote, the value is
    // `[redacted]` or empty (it ends at once)
    let after_is_redacted = |at: usize| {
        let rest = text[at..].trim_start_matches(' ');
        let rest = ["\\\"", "\"", "'"].iter().find_map(|q| rest.strip_prefix(q)).unwrap_or(rest);
        rest.is_empty() || rest.starts_with(REDACTED) || rest.starts_with(|c: char| c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ',' | ';' | ')' | '\\' | '<' | '>'))
    };
    let markers = SECRET_ASSIGNMENTS.iter().copied().chain(["bearer ", "/api/webhooks/"]);
    for marker in markers {
        let mut from = 0;
        while let Some(i) = lower[from..].find(marker) {
            let at = from + i + marker.len();
            if !after_is_redacted(at) {
                return Some(format!("an unredacted {}", marker.trim_end()));
            }
            from = at;
        }
    }
    let mut from = 0;
    while let Some(i) = text[from..].find("://") {
        let start = from + i + 3;
        let authority = &text[start..];
        let authority = &authority[..authority.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(authority.len())];
        if let Some(at) = authority.rfind('@')
            && let Some(colon) = authority[..at].find(':')
            && &authority[colon + 1..at] != REDACTED
        {
            return Some("a password in a URL".into());
        }
        from = start;
    }
    None
}

/// Checks a document: a gzip read to its end (CRC and length, as a bundle's, C-042) within the
/// size bounds, UTF-8, whose first line is the account and which carries no unredacted credential.
pub fn check(document: Vec<u8>) -> Result<Checked, String> {
    if document.is_empty() {
        return Err("an empty document".into());
    }
    if document.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("a document is at most {MAX_DOCUMENT_BYTES} bytes; this one is {}", document.len()));
    }
    let mut text = Vec::new();
    GzDecoder::new(document.as_slice()).take(MAX_UNPACKED_BYTES + 1).read_to_end(&mut text).map_err(|e| format!("not a gzip: {e}"))?;
    if text.len() as u64 > MAX_UNPACKED_BYTES {
        return Err(format!("a document unpacks to at most {MAX_UNPACKED_BYTES} bytes"));
    }
    let text = String::from_utf8(text).map_err(|_| "a document is UTF-8 text".to_string())?;
    if let Some(found) = unredacted(&text) {
        return Err(format!("{found}: the worker's redaction left a credential; nothing was stored"));
    }
    let (line, excerpt) = text.split_once('\n').unwrap_or((&text, ""));
    let account: Value = serde_json::from_str(line).map_err(|e| format!("the first line is not the account: {e}"))?;
    if account["schema"] != SCHEMA {
        return Err(format!("the first line is not a {SCHEMA} account"));
    }
    let words = |k: &str| account[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).ok_or_else(|| format!("the account has no {k}"));
    let (stage, message) = (words("stage")?, words("message")?);
    let session = match &account["session"] {
        Value::Null => None,
        Value::String(s) if core_contract::is_date(s) => Some(s.clone()),
        other => return Err(format!("session {other} is not a date written YYYY-MM-DD")),
    };
    let refusals = Some(account["refusals"].clone()).filter(|r| !r.is_null());
    Ok(Checked { stage, message, session, refusals, document_sha256: sha256_hex(&document), excerpt_bytes: excerpt.len() as u64, document })
}

/// Stores a lease's evidence. Any lease the token names may send it, live or ended (a worker
/// whose lease was lost still says why it stopped), once: true when stored, false when the lease
/// already has its evidence (the first is kept). The session must be one of the job's.
pub async fn put(client: &mut Client, token: &str, evidence: &Checked) -> Result<bool, LeaseError> {
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = tx
        .query_opt(
            "SELECT l.lease_id, j.job_id, j.first_session::text, j.last_session::text,
                    (SELECT count(*)::int FROM core.lease o WHERE o.job_id = l.job_id AND o.lease_id <= l.lease_id)
             FROM core.lease l JOIN core.job j USING (job_id) WHERE l.token_sha256 = encode(sha256(convert_to($1, 'UTF8')), 'hex')
             FOR UPDATE OF j",
            &[&token],
        )
        .await
        .map_err(failed)?
        .ok_or(LeaseError::Gone)?;
    let (lease_id, job_id, first, last, attempt): (i64, i64, String, String, i32) = (lease.get(0), lease.get(1), lease.get(2), lease.get(3), lease.get(4));
    if let Some(session) = &evidence.session
        && (session.as_str() < first.as_str() || session.as_str() > last.as_str())
    {
        return Err(LeaseError::Refused(format!("{session} is not a session of job {job_id} ({first} to {last})")));
    }
    let inserted = tx
        .execute(
            "INSERT INTO core.job_evidence (lease_id, job_id, attempt, kind, session, first_session, last_session, engine_build, worker,
                                            stage, message, refusals, document_sha256, document, excerpt_bytes)
             SELECT l.lease_id, j.job_id, $2, j.kind, $3::text::date, j.first_session, j.last_session, j.engine_build, l.worker,
                    $4, $5, $6::text::jsonb, $7, $8, $9
             FROM core.lease l JOIN core.job j USING (job_id) WHERE l.lease_id = $1
             ON CONFLICT (lease_id) DO NOTHING",
            &[
                &lease_id,
                &attempt,
                &evidence.session,
                &evidence.stage,
                &evidence.message,
                &evidence.refusals.as_ref().map(Value::to_string),
                &evidence.document_sha256,
                &evidence.document,
                &(evidence.excerpt_bytes as i64),
            ],
        )
        .await
        .map_err(failed)?;
    tx.commit().await.map_err(failed)?;
    Ok(inserted == 1)
}

const TIME: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";

/// The evidence kept, newest first (of one job, or of every job): who, where, when, the stage and
/// message, the number of refusals and the excerpt's size; never the document.
pub async fn list(client: &Client, job: Option<i64>, limit: i64) -> Result<Vec<Value>, String> {
    let rows = client
        .query(
            &format!(
                "SELECT job_id, attempt, kind, session::text, worker, engine_build, stage, message,
                        CASE WHEN jsonb_typeof(refusals) = 'array' THEN jsonb_array_length(refusals) END, excerpt_bytes,
                        to_char(stored_at AT TIME ZONE 'UTC', {TIME})
                 FROM core.job_evidence WHERE $1::bigint IS NULL OR job_id = $1 ORDER BY stored_at DESC, job_evidence_id DESC LIMIT $2"
            ),
            &[&job, &limit],
        )
        .await
        .map_err(db)?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "job_id": r.get::<_, i64>(0), "attempt": r.get::<_, i32>(1), "kind": r.get::<_, String>(2), "session": r.get::<_, Option<String>>(3),
                "worker": r.get::<_, String>(4), "engine_build": r.get::<_, String>(5), "stage": r.get::<_, String>(6), "message": r.get::<_, String>(7),
                "refusals": r.get::<_, Option<i32>>(8), "excerpt_bytes": r.get::<_, i64>(9), "stored_at": r.get::<_, String>(10),
            })
        })
        .collect())
}

/// One attempt's evidence in full (the latest attempt of the job when none is named): the
/// record, its refusals, and the excerpt's text. Checked against its stored digest.
pub async fn show(client: &Client, job: i64, attempt: Option<i32>) -> Result<Option<(Value, String)>, String> {
    let row = client
        .query_opt(
            &format!(
                "SELECT job_id, attempt, kind, session::text, first_session::text, last_session::text, worker, engine_build, stage, message,
                        refusals::text, document, document_sha256, to_char(stored_at AT TIME ZONE 'UTC', {TIME})
                 FROM core.job_evidence WHERE job_id = $1 AND ($2::int IS NULL OR attempt = $2) ORDER BY attempt DESC LIMIT 1"
            ),
            &[&job, &attempt],
        )
        .await
        .map_err(db)?;
    let Some(r) = row else { return Ok(None) };
    let (document, digest): (Vec<u8>, String) = (r.get(11), r.get(12));
    if sha256_hex(&document) != digest {
        return Err(format!("job {job}: the stored evidence does not hash to its digest"));
    }
    let mut text = String::new();
    GzDecoder::new(document.as_slice()).read_to_string(&mut text).map_err(|e| format!("job {job}: the stored evidence: {e}"))?;
    let excerpt = text.split_once('\n').map_or("", |(_, rest)| rest).to_owned();
    let refusals: Value = r.get::<_, Option<String>>(10).map(|t| serde_json::from_str(&t).unwrap_or(Value::Null)).unwrap_or(Value::Null);
    let record = json!({
        "job_id": r.get::<_, i64>(0), "attempt": r.get::<_, i32>(1), "kind": r.get::<_, String>(2), "session": r.get::<_, Option<String>>(3),
        "first_session": r.get::<_, String>(4), "last_session": r.get::<_, String>(5), "worker": r.get::<_, String>(6),
        "engine_build": r.get::<_, String>(7), "stage": r.get::<_, String>(8), "message": r.get::<_, String>(9), "refusals": refusals,
        "stored_at": r.get::<_, String>(13),
    });
    Ok(Some((record, excerpt)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_left_in_the_text_is_found() {
        for bad in [
            "GET https://api-v2.intrinio.com/securities/SPY/prices?api_key=OmFiY2Rl&page_size=100",
            "GET https://api-v2.intrinio.com/x?API_KEY=abc",
            "header Authorization: Bearer abcdef",
            "postgresql://vyreon:s3cret@db:5432/vyreon",
            "host=db password=s3cret dbname=vyreon",
            "https://bucket.example/x?X-Amz-Signature=deadbeef",
            "https://discord.com/api/webhooks/1234/abcd",
            "lease_token=abc",
            "host=db password='with space'",
            "Authorization: Bearer   abc",
            "api_key= abc",
        ] {
            assert!(unredacted(bad).is_some(), "{bad}");
        }
        for good in [
            "GET https://api-v2.intrinio.com/securities/SPY/prices?api_key=[redacted]&page_size=100",
            "header Authorization: Bearer [redacted]",
            "postgresql://vyreon:[redacted]@db:5432/vyreon",
            "postgres://db:5432/vyreon and http://vyreon-core:8080/worker/v1",
            "host=db password=[redacted] dbname=vyreon",
            "{\"error\":\"prepare:Science(\\\"close quotes cover under half of the population\\\")\"}",
            "api_key=",
            "host=db password='[redacted]' and Bearer   [redacted]",
            "{\"message\":\"api_key=\\\"[redacted]\\\" password=\\\"\\\"\"}",
        ] {
            assert_eq!(unredacted(good), None, "{good}");
        }
    }
}
