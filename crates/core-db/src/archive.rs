//! The perishable archive (migration 0020, C-052). A published day's perishable inputs, which the
//! provider will not serve again (the cohort prints, kept by it about five sessions, and the ACT01
//! interval pulls), go from the worker straight to the Spaces bucket. Workers hold no storage
//! credential: for one named object a worker asks core, core decides the object's key, records the
//! request and returns a pre-signed upload URL valid for `URL_SECONDS` (AWS Signature Version 4 in
//! the query string, which DigitalOcean Spaces accepts: <https://docs.digitalocean.com/reference/api/spaces/>,
//! "Authentication"; the signing itself is AWS's,
//! <https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html>). The worker
//! PUTs the archive and reports it; core reads the object back (a signed HEAD) and marks it KEPT
//! only when its size, its MD5 (the store's ETag of a single-part upload) and the SHA-256 the upload
//! was signed with are those requested.
//!
//! The upload URL signs the object's `Content-Length`, `Content-MD5` and `x-amz-meta-sha256`: the
//! store refuses a body of another size or MD5, and the SHA-256 travels with the object.
//!
//! Keys are content-addressed, `perishables/<category>/<session>/<sha256>.tar.zst`: a later archive
//! of the same session and category is another object beside the first, never in its place, and a
//! KEPT object is never issued a URL again. As on the droplet (`deploy/droplet/archive-perishables.sh`),
//! a session's archive is final `FREEZE_DAYS` after the session: once one is KEPT, another version
//! is no longer taken.
//!
//! An upload URL is a credential: it is never stored, logged or put in an error (only when it
//! expires is recorded); a reason a worker reports is kept only if no credential pattern is in it.
//!
//! The signer is written here on RustCrypto's HMAC and SHA-256: no maintained S3 signing crate
//! (`aws-sigv4`, `rusty-s3`) was available to this workspace's offline build. It is checked
//! against AWS's published example of a pre-signed URL (the unit test below).
use crate::evidence;
use crate::work::{self, LeaseError};
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, GenericClient};

/// The categories archived (the droplet's, less the company capture, which core keeps in its
/// database, C-043): the cohort prints and the ACT01 interval pulls.
pub const CATEGORIES: [&str; 2] = ["exq01-events", "act01-auto"];
/// How long an upload URL is valid.
pub const URL_SECONDS: i32 = 600;
/// A session's archive is final this many calendar days after the session (the droplet's
/// `VYREON_ARCHIVE_FREEZE_DAYS`).
pub const FREEZE_DAYS: i32 = 14;
/// The largest object taken (S3's single PUT allows 5 GB; a day's prints are some tens of MB packed).
pub const MAX_OBJECT_BYTES: i64 = 4 << 30;
/// The signed header that carries the archive's SHA-256 with the object.
pub const META_SHA256: &str = "x-amz-meta-sha256";
/// A request not kept this long after it was made, or a published day without its archives this
/// long after it was published, counts as missing.
pub const GRACE_MINUTES: i32 = 60;
const MAX_FAILURE_CHARS: usize = 2000;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The bucket and the key core signs with. The key is never shown (`Debug` hides it).
#[derive(Clone)]
pub struct Spaces {
    scheme: String,
    host: String,
    region: String,
    bucket: String,
    key_id: String,
    secret: String,
}

impl std::fmt::Debug for Spaces {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Spaces {{ endpoint: {}, region: {}, bucket: {}, key: (held, not shown) }}", self.endpoint(), self.region, self.bucket)
    }
}

impl Spaces {
    /// `endpoint` is `https://<host>` (e.g. `https://nyc3.digitaloceanspaces.com`); plain http
    /// only to a local test endpoint.
    pub fn new(endpoint: &str, region: &str, bucket: &str, key_id: &str, secret: &str) -> Result<Spaces, String> {
        let endpoint = endpoint.trim().trim_end_matches('/');
        let (scheme, host) = endpoint.split_once("://").ok_or_else(|| format!("the endpoint {endpoint} is not https://<host>"))?;
        let local = host.starts_with("127.0.0.1:") || host.starts_with("localhost:");
        if !(scheme == "https" || (scheme == "http" && local)) || host.is_empty() || host.contains(['/', '?', '#', '@', ' ']) {
            return Err(format!("the endpoint {endpoint} is not https://<host> (plain http only to a local test endpoint)"));
        }
        if region.is_empty() || !region.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("the region {region} is not a region name"));
        }
        let bucket_ok = (3..=63).contains(&bucket.len()) && bucket.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        if !bucket_ok {
            return Err(format!("the bucket {bucket} is not a bucket name"));
        }
        let (key_id, secret) = (key_id.trim(), secret.trim());
        if key_id.is_empty() || secret.is_empty() || key_id.contains(char::is_whitespace) || secret.contains(char::is_whitespace) {
            return Err("the Spaces key's id and secret are each one word".into());
        }
        Ok(Spaces { scheme: scheme.into(), host: host.into(), region: region.into(), bucket: bucket.into(), key_id: key_id.into(), secret: secret.into() })
    }

    pub fn endpoint(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// The object's key: content-addressed, so another version never replaces one.
    pub fn object_key(category: &str, session: &str, sha256: &str) -> String {
        format!("perishables/{category}/{session}/{sha256}.tar.zst")
    }

    /// A pre-signed URL (path-style: `<endpoint>/<bucket>/<key>`) for `method` on `key`, signing
    /// `headers` (lowercase names; `host` is added), dated `amz_date` (`YYYYMMDDTHHMMSSZ`), valid
    /// `expires` seconds. A credential: never log it.
    pub fn presign(&self, method: &str, key: &str, headers: &[(&str, &str)], amz_date: &str, expires: i32) -> String {
        let path = format!("/{}/{}", self.bucket, uri_encode(key, false));
        let mut all: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect();
        all.push(("host".into(), self.host.clone()));
        let query = presign_query(method, &path, &self.region, &self.key_id, &self.secret, amz_date, expires, &all);
        format!("{}://{}{}?{}", self.scheme, self.host, path, query)
    }
}

/// RFC 3986 percent-encoding as Signature Version 4 requires: the unreserved characters as they
/// are, every other byte `%XX` (uppercase); `/` kept in a path (`encode_slash` false).
pub fn uri_encode(text: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || (b == b'/' && !encode_slash) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The query string of a URL pre-signed with AWS Signature Version 4 (S3, unsigned payload):
/// `X-Amz-Algorithm`, `-Credential`, `-Date`, `-Expires`, `-SignedHeaders` and `-Signature`.
/// `headers` holds every signed header, `host` among them, with lowercase names.
#[allow(clippy::too_many_arguments)]
pub fn presign_query(method: &str, canonical_uri: &str, region: &str, key_id: &str, secret: &str, amz_date: &str, expires: i32, headers: &[(String, String)]) -> String {
    let date = &amz_date[..8.min(amz_date.len())];
    let scope = format!("{date}/{region}/s3/aws4_request");
    let mut headers: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned())).collect();
    headers.sort();
    let signed = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential", format!("{key_id}/{scope}")),
        ("X-Amz-Date", amz_date.to_owned()),
        ("X-Amz-Expires", expires.to_string()),
        ("X-Amz-SignedHeaders", signed.clone()),
    ];
    // already in byte order of their names
    let query = params.iter().map(|(k, v)| format!("{k}={}", uri_encode(v, true))).collect::<Vec<_>>().join("&");
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let canonical = format!("{method}\n{canonical_uri}\n{query}\n{canonical_headers}\n{signed}\nUNSIGNED-PAYLOAD");
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex(&Sha256::digest(canonical.as_bytes())));
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date);
    for part in [region, "s3", "aws4_request"] {
        key = hmac(&key, part);
    }
    format!("{query}&X-Amz-Signature={}", hex(&hmac(&key, &to_sign)))
}

/// `now` as Signature Version 4 dates it (`YYYYMMDDTHHMMSSZ`, UTC), by the database's clock.
pub async fn amz_date(c: &impl GenericClient, now: &str) -> Result<String, String> {
    Ok(c.query_one("SELECT to_char($1::text::timestamptz AT TIME ZONE 'UTC', 'YYYYMMDD\"T\"HH24MISS\"Z\"')", &[&now]).await.map_err(db)?.get(0))
}

/// What a worker asks to archive: one category of one session, packed (`<session>/...` in a
/// tar.zst), its size and digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub category: String,
    pub session: String,
    pub bytes: i64,
    pub sha256: String,
    pub md5: String,
}

/// Core's answer to a request.
#[derive(Clone, PartialEq, Eq)]
pub enum Answer {
    /// This archive is kept already: nothing to send.
    Kept { archive_id: i64, key: String },
    /// The session's archive is final (another version is kept and the session is more than
    /// `FREEZE_DAYS` old): nothing to send.
    Frozen { key: String },
    /// PUT the archive to `url` with these headers (and its length), before `expires_seconds`.
    Upload { archive_id: i64, key: String, url: String, headers: Vec<(String, String)>, expires_seconds: i32 },
}

// the URL is a credential: a debug print never shows it
impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Answer::Kept { archive_id, key } => write!(f, "Kept {{ archive_id: {archive_id}, key: {key} }}"),
            Answer::Frozen { key } => write!(f, "Frozen {{ key: {key} }}"),
            Answer::Upload { archive_id, key, expires_seconds, .. } => write!(f, "Upload {{ archive_id: {archive_id}, key: {key}, url: (not shown), expires_seconds: {expires_seconds} }}"),
        }
    }
}

impl Answer {
    /// The worker API's reply (the URL included: it goes to the worker that asked, and nowhere else).
    pub fn reply(&self) -> Value {
        match self {
            Answer::Kept { archive_id, key } => json!({"state": "kept", "archive_id": archive_id, "key": key}),
            Answer::Frozen { key } => json!({"state": "frozen", "key": key}),
            Answer::Upload { archive_id, key, url, headers, expires_seconds } => json!({
                "state": "upload", "archive_id": archive_id, "key": key, "url": url, "expires_seconds": expires_seconds,
                "headers": headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<String, Value>>(),
            }),
        }
    }
}

fn check(r: &Request) -> Result<(), LeaseError> {
    let refuse = |m: String| Err(LeaseError::Refused(m));
    if !CATEGORIES.contains(&r.category.as_str()) {
        return refuse(format!("{} is not a perishable category ({})", r.category, CATEGORIES.join(", ")));
    }
    if !core_contract::is_date(&r.session) {
        return refuse(format!("{} is not a date written YYYY-MM-DD", r.session));
    }
    if !(1..=MAX_OBJECT_BYTES).contains(&r.bytes) {
        return refuse(format!("an archive is 1 to {MAX_OBJECT_BYTES} bytes; this one is {}", r.bytes));
    }
    if !is_hex(&r.sha256, 64) || !is_hex(&r.md5, 32) {
        return refuse("sha256 and md5 are lowercase hex digests".into());
    }
    Ok(())
}

/// Takes a worker's request to archive one object of a day its job published (a day block's
/// lease) at `now` (the database's clock): the archive kept already, the session frozen, or an
/// upload URL for the object's key, the request recorded (REQUESTED; a request for an archive
/// not kept is sent again with a new URL).
pub async fn request(client: &mut Client, spaces: &Spaces, token: &str, r: &Request, now: &str) -> Result<Answer, LeaseError> {
    check(r)?;
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    if lease.kind != "DAY_BLOCK" {
        return Err(LeaseError::Refused(format!("job {} is not a day block", lease.job_id)));
    }
    let published: bool = tx
        .query_one(
            "SELECT EXISTS (SELECT FROM vyreon.day_revision
                            WHERE profile_id = $1 AND engine_build = $2 AND state = 'PUBLISHED' AND base_revision IS NULL
                              AND represented_date = $3::text::date
                              AND represented_date BETWEEN $4::text::date AND COALESCE($5::text::date, $6::text::date))",
            &[&lease.profile_id, &lease.engine_build, &r.session, &lease.first_session, &lease.next_session, &lease.last_session],
        )
        .await
        .map_err(failed)?
        .get(0);
    if !published {
        return Err(LeaseError::Refused(format!("{} is not a day job {} has published: a day's perishables are archived once it is published", r.session, lease.job_id)));
    }
    let held = tx
        .query_opt(
            "SELECT archive_id, state, object_key, bytes, md5 FROM core.perishable_archive
             WHERE session = $1::text::date AND category = $2 AND sha256 = $3 FOR UPDATE",
            &[&r.session, &r.category, &r.sha256],
        )
        .await
        .map_err(failed)?;
    if let Some(row) = &held {
        let (id, state, key, bytes, md5): (i64, String, String, i64, String) = (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4));
        if (bytes, md5.as_str()) != (r.bytes, r.md5.as_str()) {
            return Err(LeaseError::Refused(format!("{} {} {} was requested as {bytes} bytes of MD5 {md5}; this request differs", r.category, r.session, r.sha256)));
        }
        if state == "KEPT" {
            tx.commit().await.map_err(failed)?;
            return Ok(Answer::Kept { archive_id: id, key });
        }
    }
    let frozen = tx
        .query_opt(
            "SELECT object_key FROM core.perishable_archive
             WHERE session = $1::text::date AND category = $2 AND state = 'KEPT'
               AND session < ($3::text::timestamptz AT TIME ZONE 'UTC')::date - $4::int
             ORDER BY kept_at, archive_id LIMIT 1",
            &[&r.session, &r.category, &now, &FREEZE_DAYS],
        )
        .await
        .map_err(failed)?;
    if let Some(row) = frozen {
        tx.commit().await.map_err(failed)?;
        return Ok(Answer::Frozen { key: row.get(0) });
    }
    let key = Spaces::object_key(&r.category, &r.session, &r.sha256);
    let id: i64 = match held {
        Some(row) => tx
            .query_one(
                "UPDATE core.perishable_archive SET state = 'REQUESTED', urls_issued = urls_issued + 1, requested_at = $2::text::timestamptz,
                        url_expires_at = $2::text::timestamptz + make_interval(secs => $3::int), failure = NULL,
                        job_id = $4, engine_build = $5, worker = $6
                 WHERE archive_id = $1 RETURNING archive_id",
                &[&row.get::<_, i64>(0), &now, &URL_SECONDS, &lease.job_id, &lease.engine_build, &lease.worker],
            )
            .await
            .map_err(failed)?
            .get(0),
        None => tx
            .query_one(
                "INSERT INTO core.perishable_archive (session, category, sha256, md5, bytes, bucket, object_key, state, requested_at, url_expires_at, job_id, engine_build, worker)
                 VALUES ($1::text::date, $2, $3, $4, $5, $6, $7, 'REQUESTED', $8::text::timestamptz, $8::text::timestamptz + make_interval(secs => $9::int), $10, $11, $12)
                 RETURNING archive_id",
                &[&r.session, &r.category, &r.sha256, &r.md5, &r.bytes, &spaces.bucket(), &key, &now, &URL_SECONDS, &lease.job_id, &lease.engine_build, &lease.worker],
            )
            .await
            .map_err(failed)?
            .get(0),
    };
    let date = amz_date(&tx, now).await.map_err(LeaseError::Failed)?;
    tx.commit().await.map_err(failed)?;
    let md5: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&r.md5[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
    let headers = vec![("content-md5".to_owned(), base64::engine::general_purpose::STANDARD.encode(md5)), (META_SHA256.to_owned(), r.sha256.clone())];
    let length = r.bytes.to_string();
    let signed: Vec<(&str, &str)> = [("content-length", length.as_str())].into_iter().chain(headers.iter().map(|(k, v)| (k.as_str(), v.as_str()))).collect();
    let url = spaces.presign("PUT", &key, &signed, &date, URL_SECONDS);
    Ok(Answer::Upload { archive_id: id, key, url, headers, expires_seconds: URL_SECONDS })
}

/// One requested archive, as core checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub archive_id: i64,
    pub key: String,
    pub bytes: i64,
    pub sha256: String,
    pub md5: String,
    pub state: String,
}

/// The archive `archive_id`, requested by the job whose live lease `token` names.
pub async fn target(client: &impl GenericClient, token: &str, archive_id: i64) -> Result<Target, LeaseError> {
    let lease = work::identify(client, token).await?;
    let row = client
        .query_opt("SELECT object_key, bytes, sha256, md5, state FROM core.perishable_archive WHERE archive_id = $1 AND job_id = $2", &[&archive_id, &lease.job_id])
        .await
        .map_err(|e| LeaseError::Failed(db(e)))?
        .ok_or_else(|| LeaseError::Refused(format!("archive {archive_id} is not one job {} requested", lease.job_id)))?;
    Ok(Target { archive_id, key: row.get(0), bytes: row.get(1), sha256: row.get(2), md5: row.get(3), state: row.get(4) })
}

/// What reading the object back (a HEAD) answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub bytes: Option<i64>,
    /// The ETag as sent (quoted).
    pub etag: Option<String>,
    /// `x-amz-meta-sha256`.
    pub sha256: Option<String>,
}

/// Whether the object read back is the archive requested: there, of its size, its MD5 (a
/// single-part upload's ETag) and its signed SHA-256.
pub fn verify(t: &Target, head: &Head) -> Result<(), String> {
    match head.status {
        200 => {}
        404 => return Err("the object is not in the bucket".into()),
        s => return Err(format!("reading the object back answered {s}")),
    }
    if head.bytes != Some(t.bytes) {
        return Err(format!("the bucket holds {} bytes, not {}", head.bytes.map_or("an unstated number of".into(), |b| b.to_string()), t.bytes));
    }
    let etag = head.etag.as_deref().map(|e| e.trim().trim_matches('"').to_ascii_lowercase());
    if etag.as_deref() != Some(t.md5.as_str()) {
        return Err(format!("the bucket's MD5 (its ETag) is {}, not {}", etag.as_deref().unwrap_or("not stated"), t.md5));
    }
    if head.sha256.as_deref() != Some(t.sha256.as_str()) {
        return Err(format!("the object's {META_SHA256} is {}, not {}", head.sha256.as_deref().unwrap_or("not stated"), t.sha256));
    }
    Ok(())
}

/// A reason as kept: cut to `MAX_FAILURE_CHARS`, and withheld whole when a credential pattern is
/// in it (an upload URL, a key).
pub fn failure_text(text: &str) -> String {
    if evidence::unredacted(text).is_some() {
        return "(the reason is not kept: it carried a credential pattern)".into();
    }
    if text.chars().count() > MAX_FAILURE_CHARS { text.chars().take(MAX_FAILURE_CHARS - 1).chain(['…']).collect() } else { text.to_owned() }
}

/// Records what became of an archive at `now`: KEPT (Ok) or FAILED with the reason, under the
/// requesting job's live lease. An archive KEPT stays KEPT. True when it is kept.
pub async fn record(client: &mut Client, token: &str, archive_id: i64, outcome: &Result<(), String>, now: &str) -> Result<bool, LeaseError> {
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    let failure = outcome.as_ref().err().map(|e| failure_text(e));
    let row = tx
        .query_opt(
            "UPDATE core.perishable_archive SET state = CASE WHEN $3::text IS NULL THEN 'KEPT' ELSE 'FAILED' END,
                    kept_at = CASE WHEN $3::text IS NULL THEN $4::text::timestamptz END, failure = $3
             WHERE archive_id = $1 AND job_id = $2 AND state <> 'KEPT' RETURNING state",
            &[&archive_id, &lease.job_id, &failure, &now],
        )
        .await
        .map_err(failed)?;
    let kept = match row {
        Some(r) => r.get::<_, String>(0) == "KEPT",
        None => tx
            .query_opt("SELECT state FROM core.perishable_archive WHERE archive_id = $1 AND job_id = $2", &[&archive_id, &lease.job_id])
            .await
            .map_err(failed)?
            .ok_or_else(|| LeaseError::Refused(format!("archive {archive_id} is not one job {} requested", lease.job_id)))?
            .get::<_, String>(0)
            == "KEPT",
    };
    tx.commit().await.map_err(failed)?;
    Ok(kept)
}

/// The archives of a session (or of the newest sessions, at most `limit` rows), newest session
/// first, then by category and request.
pub async fn list(client: &impl GenericClient, session: Option<&str>, limit: i64) -> Result<Vec<Value>, String> {
    if let Some(s) = session
        && !core_contract::is_date(s)
    {
        return Err(format!("{s} is not a date written YYYY-MM-DD"));
    }
    let rows = client
        .query(
            "SELECT session::text, category, state, bytes, sha256, bucket, object_key, urls_issued,
                    to_char(requested_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                    to_char(kept_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), failure, job_id, worker, archive_id
             FROM core.perishable_archive WHERE $1::text IS NULL OR session = $1::text::date
             ORDER BY session DESC, category, requested_at, archive_id LIMIT $2",
            &[&session, &limit],
        )
        .await
        .map_err(db)?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "session": r.get::<_, String>(0), "category": r.get::<_, String>(1), "state": r.get::<_, String>(2), "bytes": r.get::<_, i64>(3),
                "sha256": r.get::<_, String>(4), "bucket": r.get::<_, String>(5), "key": r.get::<_, String>(6), "urls_issued": r.get::<_, i32>(7),
                "requested_at": r.get::<_, String>(8), "kept_at": r.get::<_, Option<String>>(9), "failure": r.get::<_, Option<String>>(10),
                "job_id": r.get::<_, i64>(11), "worker": r.get::<_, String>(12), "archive_id": r.get::<_, i64>(13),
            })
        })
        .collect())
}

/// One archive missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub session: String,
    pub category: String,
    /// Seconds since it was first requested, or since its day was published.
    pub age: i64,
}

/// The archives missing at `now`, oldest session first: a (session, category) with nothing KEPT
/// that was requested, or, from `expected_from`, of a day of `profile` published (its base), each
/// at least `GRACE_MINUTES` ago. Without `expected_from` only what was requested counts.
pub async fn missing(client: &impl GenericClient, profile: &str, expected_from: Option<&str>, now: &str) -> Result<Vec<Missing>, String> {
    let categories: Vec<&str> = CATEGORIES.to_vec();
    let rows = client
        .query(
            "WITH candidate AS (
                 SELECT session, category, requested_at AS since FROM core.perishable_archive
                 UNION ALL
                 SELECT d.represented_date, c.category, d.published_at FROM vyreon.day_revision d CROSS JOIN unnest($4::text[]) AS c(category)
                 WHERE $3::text IS NOT NULL AND d.represented_date >= $3::text::date AND d.profile_id = $2
                   AND d.base_revision IS NULL AND d.published_at IS NOT NULL)
             SELECT session::text, category, GREATEST(extract(epoch FROM $1::text::timestamptz - min(since)), 0)::bigint FROM candidate c
             WHERE NOT EXISTS (SELECT FROM core.perishable_archive k WHERE k.session = c.session AND k.category = c.category AND k.state = 'KEPT')
             GROUP BY session, category
             HAVING min(since) <= $1::text::timestamptz - make_interval(mins => $5::int)
             ORDER BY session, category",
            &[&now, &profile, &expected_from, &categories, &GRACE_MINUTES],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| Missing { session: r.get(0), category: r.get(1), age: r.get(2) }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS's published example of a pre-signed URL ("Authenticating Requests: Using Query
    /// Parameters (AWS Signature Version 4)", Amazon S3 API Reference, section "An Example"):
    /// GET `test.txt` of `examplebucket` (virtual-hosted), 24 hours from 2013-05-24T00:00:00Z,
    /// us-east-1, the documentation's example key. Its stated signature:
    /// aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404.
    #[test]
    fn the_signer_reproduces_aws_published_presigned_example() {
        let query = presign_query(
            "GET",
            "/test.txt",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "20130524T000000Z",
            86400,
            &[("host".into(), "examplebucket.s3.amazonaws.com".into())],
        );
        assert_eq!(
            query,
            "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
             &X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host\
             &X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
        );
    }

    #[test]
    fn a_url_is_path_style_and_signs_the_objects_headers() {
        let s = Spaces::new("https://nyc3.digitaloceanspaces.com/", "nyc3", "vyreon-data", "DO00EXAMPLEKEYID", "an-example-secret").unwrap();
        let key = Spaces::object_key("exq01-events", "2026-11-16", &"a".repeat(64));
        let url = s.presign("PUT", &key, &[("content-length", "42"), ("Content-MD5", "1B2M2Y8AsgTpgAmY7PhCfg=="), (META_SHA256, "ab")], "20261117T130000Z", URL_SECONDS);
        assert!(url.starts_with(&format!("https://nyc3.digitaloceanspaces.com/vyreon-data/perishables/exq01-events/2026-11-16/{}.tar.zst?", "a".repeat(64))), "{url}");
        assert!(url.contains("&X-Amz-Expires=600&") && url.contains("X-Amz-SignedHeaders=content-length%3Bcontent-md5%3Bhost%3Bx-amz-meta-sha256&"), "{url}");
        assert!(url.contains("X-Amz-Credential=DO00EXAMPLEKEYID%2F20261117%2Fnyc3%2Fs3%2Faws4_request&"));
        // the key is never shown
        let shown = format!("{s:?}");
        assert!(!shown.contains("DO00EXAMPLEKEYID") && !shown.contains("an-example-secret"), "{shown}");
        for bad in ["http://nyc3.digitaloceanspaces.com", "nyc3.digitaloceanspaces.com", "https://nyc3.digitaloceanspaces.com/vyreon-data", "https://"] {
            assert!(Spaces::new(bad, "nyc3", "vyreon-data", "k", "s").is_err(), "{bad}");
        }
        assert!(Spaces::new("http://127.0.0.1:9000", "nyc3", "vyreon-data", "k", "s").is_ok());
        assert!(Spaces::new("https://nyc3.digitaloceanspaces.com", "nyc3", "Vyreon_Data", "k", "s").is_err());
    }

    #[test]
    fn a_read_back_is_kept_only_when_it_is_the_archive_requested() {
        let t = Target { archive_id: 1, key: "k".into(), bytes: 42, sha256: "b".repeat(64), md5: "c".repeat(32), state: "REQUESTED".into() };
        let head = Head { status: 200, bytes: Some(42), etag: Some(format!("\"{}\"", "C".repeat(32))), sha256: Some("b".repeat(64)) };
        assert_eq!(verify(&t, &head), Ok(()));
        assert!(verify(&t, &Head { status: 404, ..head.clone() }).unwrap_err().contains("not in the bucket"));
        assert!(verify(&t, &Head { bytes: Some(41), ..head.clone() }).unwrap_err().contains("41 bytes"));
        assert!(verify(&t, &Head { etag: Some(format!("\"{}-2\"", "c".repeat(32))), ..head.clone() }).unwrap_err().contains("ETag"));
        assert!(verify(&t, &Head { sha256: None, ..head.clone() }).unwrap_err().contains("not stated"));
    }

    #[test]
    fn a_reason_carrying_a_credential_is_not_kept() {
        let url = "PUT https://nyc3.digitaloceanspaces.com/vyreon-data/k?X-Amz-Credential=DO00X%2F20261117&X-Amz-Signature=0123abcd: 403";
        assert!(failure_text(url).contains("not kept"));
        assert_eq!(failure_text("the PUT answered 403 (SignatureDoesNotMatch)"), "the PUT answered 403 (SignatureDoesNotMatch)");
        assert_eq!(failure_text(&"x".repeat(5000)).chars().count(), MAX_FAILURE_CHARS);
    }
}
