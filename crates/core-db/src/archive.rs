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
//! The signing is the `rusty-s3` crate's (Signature Version 4, query-string pre-signed URLs, built
//! for S3-compatible stores); its output is checked against AWS's published example of a
//! pre-signed URL (the unit test below).
use crate::evidence;
use crate::work::{self, LeaseError};
use base64::Engine;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient};

/// The categories archived (the droplet's, less the company capture, which core keeps in its
/// database, C-043): the cohort prints and the ACT01 interval pulls.
pub const CATEGORIES: [&str; 2] = ["exq01-events", "act01-auto"];
/// C-096: a day's own logs (the fill's, the captures', the day's), kept for every day a block worked on,
/// published or not: not a perishable input (nothing counts it missing, nothing freezes it), kept the same way.
pub const RUN_LOGS: &str = "run-logs";
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
    endpoint: String,
    bucket: Bucket,
    credentials: Credentials,
}

impl std::fmt::Debug for Spaces {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Spaces {{ endpoint: {}, region: {}, bucket: {}, key: (held, not shown) }}", self.endpoint, self.bucket.region(), self.bucket.name())
    }
}

/// `at` (Unix seconds) as the signer takes it.
fn timestamp(at: i64) -> jiff::Timestamp {
    jiff::Timestamp::from_second(at).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
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
        let url = endpoint.parse().map_err(|e| format!("the endpoint {endpoint}: {e}"))?;
        // path-style: `<endpoint>/<bucket>/<key>`
        let bucket = Bucket::new(url, UrlStyle::Path, bucket.to_owned(), region.to_owned()).map_err(|e| format!("the bucket {bucket}: {e}"))?;
        Ok(Spaces { endpoint: endpoint.to_owned(), bucket, credentials: Credentials::new(key_id, secret) })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn bucket(&self) -> &str {
        self.bucket.name()
    }

    /// The object's key: content-addressed, so another version never replaces one.
    pub fn object_key(category: &str, session: &str, sha256: &str) -> String {
        format!("perishables/{category}/{session}/{sha256}.tar.zst")
    }

    /// A pre-signed PUT URL for `key`, signing `headers` (lowercase names; `host` is added), at
    /// `at` (Unix seconds), valid `expires` seconds. A credential: never log it.
    pub fn presign_put(&self, key: &str, headers: &[(&str, &str)], at: i64, expires: i32) -> String {
        let mut action = rusty_s3::actions::PutObject::new(&self.bucket, Some(&self.credentials), key);
        for (k, v) in headers {
            action.headers_mut().insert(k.to_ascii_lowercase(), v.to_string());
        }
        action.sign_with_time(std::time::Duration::from_secs(expires.max(1) as u64), &timestamp(at)).to_string()
    }

    /// A pre-signed HEAD URL for `key` at `at`, valid `expires` seconds. A credential.
    pub fn presign_head(&self, key: &str, at: i64, expires: i32) -> String {
        let action = rusty_s3::actions::HeadObject::new(&self.bucket, Some(&self.credentials), key);
        action.sign_with_time(std::time::Duration::from_secs(expires.max(1) as u64), &timestamp(at)).to_string()
    }
}

/// `now` in Unix seconds, by the database's clock: the time a URL is signed at.
pub async fn signing_time(c: &impl GenericClient, now: &str) -> Result<i64, String> {
    Ok(c.query_one("SELECT floor(extract(epoch FROM $1::text::timestamptz))::bigint", &[&now]).await.map_err(db)?.get(0))
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
    if !CATEGORIES.contains(&r.category.as_str()) && r.category != RUN_LOGS {
        return refuse(format!("{} is not a perishable category ({}, {RUN_LOGS})", r.category, CATEGORIES.join(", ")));
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

/// C-096: a day's logs are kept whether it published or not: a day block's, for a session of its job. C-098
/// (audit 2026-11-26, core 14): a session of the calendar (a date inside the block's range that is no session
/// reached the archive's foreign key and answered as core's failure, not the worker's refused request).
async fn logs_session(tx: &impl GenericClient, lease: &work::Held, session: &str) -> Result<(), LeaseError> {
    let within: bool = tx
        .query_one(
            "SELECT $1::text::date BETWEEN $2::text::date AND $3::text::date AND EXISTS (SELECT FROM core.session s WHERE s.session = $1::text::date)",
            &[&session, &lease.first_session, &lease.last_session],
        )
        .await
        .map_err(|e| LeaseError::Failed(db(e)))?
        .get(0);
    // C-098 addendum (engine D-568, G13): a republished day keeps its logs too (a REPUBLISH job's worker sends them)
    if !matches!(lease.kind.as_str(), "DAY_BLOCK" | "REPUBLISH") || !within {
        return Err(LeaseError::Refused(format!("{session} is not a session of day block or republish job {}: a day's logs are kept by the job that worked on it", lease.job_id)));
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
    if r.category == RUN_LOGS {
        logs_session(&tx, &lease, &r.session).await?;
    } else {
        published_by(&tx, &lease, &r.session).await?;
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
             WHERE session = $1::text::date AND category = $2 AND state = 'KEPT' AND category <> 'run-logs'
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
                // `requested_at` stays the first request's: the age a missing archive is counted
                // from (round v, 5); the row moves to the place this URL is signed for (round v, 3)
                "UPDATE core.perishable_archive SET state = 'REQUESTED', urls_issued = urls_issued + 1,
                        url_expires_at = $2::text::timestamptz + make_interval(secs => $3::int), failure = NULL,
                        job_id = $4, engine_build = $5, worker = $6, endpoint = $7, bucket = $8
                 WHERE archive_id = $1 RETURNING archive_id",
                &[&row.get::<_, i64>(0), &now, &URL_SECONDS, &lease.job_id, &lease.engine_build, &lease.worker, &spaces.endpoint(), &spaces.bucket()],
            )
            .await
            .map_err(failed)?
            .get(0),
        None => tx
            .query_one(
                "INSERT INTO core.perishable_archive (session, category, sha256, md5, bytes, endpoint, bucket, object_key, state, requested_at, url_expires_at, job_id, engine_build, worker)
                 VALUES ($1::text::date, $2, $3, $4, $5, $13, $6, $7, 'REQUESTED', $8::text::timestamptz, $8::text::timestamptz + make_interval(secs => $9::int), $10, $11, $12)
                 RETURNING archive_id",
                &[&r.session, &r.category, &r.sha256, &r.md5, &r.bytes, &spaces.bucket(), &key, &now, &URL_SECONDS, &lease.job_id, &lease.engine_build, &lease.worker, &spaces.endpoint()],
            )
            .await
            .map_err(failed)?
            .get(0),
    };
    let at = signing_time(&tx, now).await.map_err(LeaseError::Failed)?;
    tx.commit().await.map_err(failed)?;
    let md5: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&r.md5[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
    let headers = vec![("content-md5".to_owned(), base64::engine::general_purpose::STANDARD.encode(md5)), (META_SHA256.to_owned(), r.sha256.clone())];
    let length = r.bytes.to_string();
    let signed: Vec<(&str, &str)> = [("content-length", length.as_str())].into_iter().chain(headers.iter().map(|(k, v)| (k.as_str(), v.as_str()))).collect();
    let url = spaces.presign_put(&key, &signed, at, URL_SECONDS);
    Ok(Answer::Upload { archive_id: id, key, url, headers, expires_seconds: URL_SECONDS })
}

/// Refused unless `lease` is a day block's and its job has published `session` (its base, its own
/// build): a day's perishables are archived (or reported not packed), and its reference check kept
/// (C-054), once it is published.
/// Revisions do not record their job; a day the job published is one it has marked done
/// (`work::day_done`, which needs the day's base of the job's build published, and which the
/// worker sends after publishing, before its archive phase): a day before the job's next, never
/// the next itself (audit round x, 9: a block just leased on a day published by hand before it).
pub(crate) async fn published_by(tx: &impl GenericClient, lease: &work::Held, session: &str) -> Result<(), LeaseError> {
    if lease.kind != "DAY_BLOCK" {
        return Err(LeaseError::Refused(format!("job {} is not a day block", lease.job_id)));
    }
    // C-074: a compare-only day block keeps nothing (it publishes nothing either)
    if crate::publish::compare_only(tx, lease).await.map_err(|e| LeaseError::Failed(e.to_string()))? {
        return Err(LeaseError::Refused(format!("job {} is a compare-only day block: it keeps nothing", lease.job_id)));
    }
    let published: bool = tx
        .query_one(
            "SELECT EXISTS (SELECT FROM vyreon.day_revision
                            WHERE profile_id = $1 AND engine_build = $2 AND state = 'PUBLISHED' AND base_revision IS NULL
                              AND represented_date = $3::text::date
                              AND represented_date BETWEEN $4::text::date AND $6::text::date
                              AND ($5::text IS NULL OR represented_date < $5::text::date))",
            &[&lease.profile_id, &lease.engine_build, &session, &lease.first_session, &lease.next_session, &lease.last_session],
        )
        .await
        .map_err(|e| LeaseError::Failed(db(e)))?
        .get(0);
    if !published {
        return Err(LeaseError::Refused(format!("{session} is not a day job {} has published: a day's perishables are archived, and its reference check kept, once it is published", lease.job_id)));
    }
    Ok(())
}

/// Records that a worker could not pack a category of a day its job published (round w, 7): no
/// archive was requested, and the (session, category) is counted missing from this report
/// (`missing`), whatever `VYREON_CORE_ARCHIVE_FROM` says.
pub async fn unpacked(client: &mut Client, token: &str, category: &str, session: &str, why: &str, now: &str) -> Result<(), LeaseError> {
    // C-098 (audit 2026-11-26, core 10): the logs too (a failed packing of them left no word why); they are never
    // counted missing (`missing` reads the perishable categories only)
    if !CATEGORIES.contains(&category) && category != RUN_LOGS {
        return Err(LeaseError::Refused(format!("{category} is not a perishable category ({}, {RUN_LOGS})", CATEGORIES.join(", "))));
    }
    if !core_contract::is_date(session) {
        return Err(LeaseError::Refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    if category == RUN_LOGS {
        logs_session(&tx, &lease, session).await?;
    } else {
        published_by(&tx, &lease, session).await?;
    }
    // sent again after its answer was lost: the one record stands (audit round x, 17)
    tx.execute(
        "INSERT INTO core.perishable_unpacked (session, category, failure, reported_at, job_id, engine_build, worker)
         VALUES ($1::text::date, $2, $3, $4::text::timestamptz, $5, $6, $7)
         ON CONFLICT (job_id, worker, session, category) DO NOTHING",
        &[&session, &category, &failure_text(why), &now, &lease.job_id, &lease.engine_build, &lease.worker],
    )
    .await
    .map_err(failed)?;
    tx.commit().await.map_err(failed)
}

/// What workers reported they could not pack (of a session, or the newest), newest session first:
/// (session, category, why, job, worker, when, in UTC). A report of a category the same job and
/// worker had KEPT (the upload done, its outcome line lost when the child was stopped) is
/// superseded and not listed, in whichever order the two came (audit round x, 8); `missing`
/// already counts a KEPT category as kept.
pub async fn unpacked_list(client: &impl GenericClient, session: Option<&str>, limit: i64) -> Result<Vec<(String, String, String, i64, String, String)>, String> {
    let rows = client
        .query(
            "SELECT session::text, category, failure, job_id, worker, to_char(reported_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             FROM core.perishable_unpacked u WHERE ($1::text IS NULL OR session = $1::text::date)
               AND NOT EXISTS (SELECT FROM core.perishable_archive k WHERE k.session = u.session AND k.category = u.category
                                 AND k.job_id = u.job_id AND k.worker = u.worker AND k.state = 'KEPT')
             ORDER BY session DESC, category, reported_at LIMIT $2",
            &[&session, &limit],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5))).collect())
}

/// One requested archive, as core checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub archive_id: i64,
    /// Where its URL was signed for: the endpoint (None: a row from before migration 0022) and
    /// the bucket.
    pub endpoint: Option<String>,
    pub bucket: String,
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
        .query_opt("SELECT object_key, bytes, sha256, md5, state, endpoint, bucket FROM core.perishable_archive WHERE archive_id = $1 AND job_id = $2", &[&archive_id, &lease.job_id])
        .await
        .map_err(|e| LeaseError::Failed(db(e)))?
        .ok_or_else(|| LeaseError::Refused(format!("archive {archive_id} is not one job {} requested", lease.job_id)))?;
    Ok(Target { archive_id, endpoint: row.get(5), bucket: row.get(6), key: row.get(0), bytes: row.get(1), sha256: row.get(2), md5: row.get(3), state: row.get(4) })
}

/// Whether `spaces` (what core signs for now) is the place the archive's URL was signed for: the
/// object is read back only there, never in another bucket (round v, 3). Err: why not.
pub fn same_place(t: &Target, spaces: &Spaces) -> Result<(), String> {
    if t.endpoint.as_deref() == Some(spaces.endpoint()) && t.bucket == spaces.bucket() {
        return Ok(());
    }
    Err(format!(
        "its upload URL was signed for {}/{}, and core now signs for {}/{}: not read back (ask again for a URL)",
        t.endpoint.as_deref().unwrap_or("(an endpoint not recorded)"),
        t.bucket,
        spaces.endpoint(),
        spaces.bucket()
    ))
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

/// Records what became of the archive `read` (as `target` read it, before it was read back) at
/// `now`: KEPT (Ok) or FAILED with the reason, under the requesting job's live lease. An archive
/// KEPT stays KEPT. The row must still be in the place `read` names (endpoint, bucket and key): an
/// archive asked again elsewhere since is not marked by an outcome read in the old place (audit
/// round x, 1: a core that lost its lock to another, still answering). True when it is kept.
pub async fn record(client: &mut Client, token: &str, read: &Target, outcome: &Result<(), String>, now: &str) -> Result<bool, LeaseError> {
    let failed = |e: tokio_postgres::Error| LeaseError::Failed(db(e));
    let archive_id = read.archive_id;
    let tx = client.transaction().await.map_err(failed)?;
    let lease = work::held(&tx, token).await?;
    let failure = outcome.as_ref().err().map(|e| failure_text(e));
    let row = tx
        .query_opt(
            "UPDATE core.perishable_archive SET state = CASE WHEN $3::text IS NULL THEN 'KEPT' ELSE 'FAILED' END,
                    kept_at = CASE WHEN $3::text IS NULL THEN $4::text::timestamptz END, failure = $3
             WHERE archive_id = $1 AND job_id = $2 AND state <> 'KEPT'
               AND endpoint IS NOT DISTINCT FROM $5 AND bucket = $6 AND object_key = $7 RETURNING state",
            &[&archive_id, &lease.job_id, &failure, &now, &read.endpoint, &read.bucket, &read.key],
        )
        .await
        .map_err(failed)?;
    let kept = match row {
        Some(r) => r.get::<_, String>(0) == "KEPT",
        None => {
            let row = tx
                .query_opt(
                    "SELECT state, endpoint IS NOT DISTINCT FROM $3 AND bucket = $4 AND object_key = $5 FROM core.perishable_archive WHERE archive_id = $1 AND job_id = $2",
                    &[&archive_id, &lease.job_id, &read.endpoint, &read.bucket, &read.key],
                )
                .await
                .map_err(failed)?
                .ok_or_else(|| LeaseError::Refused(format!("archive {archive_id} is not one job {} requested", lease.job_id)))?;
            let (state, same): (String, bool) = (row.get(0), row.get(1));
            if state != "KEPT" && !same {
                return Err(LeaseError::Refused(format!("archive {archive_id} was asked again in another place since it was read: this outcome is not recorded")));
            }
            state == "KEPT"
        }
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
                    to_char(kept_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), failure, job_id, worker, archive_id, endpoint
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
                "endpoint": r.get::<_, Option<String>>(14),
            })
        })
        .collect())
}

/// One archive missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub session: String,
    pub category: String,
    /// Seconds since it was first requested, or since its day was published (a request sent
    /// again does not reset it).
    pub age: i64,
}

/// The archives missing at `now`, oldest session first: a (session, category) with nothing KEPT
/// that was requested or reported not packed, or, from `expected_from`, of a day of `profile` published (its base), each
/// at least `GRACE_MINUTES` ago. Without `expected_from` only what was requested counts.
pub async fn missing(client: &impl GenericClient, profile: &str, expected_from: Option<&str>, now: &str) -> Result<Vec<Missing>, String> {
    let categories: Vec<&str> = CATEGORIES.to_vec();
    let rows = client
        .query(
            "WITH candidate AS (
                 SELECT session, category, requested_at AS since FROM core.perishable_archive WHERE category = ANY($4::text[])
                 UNION ALL
                 SELECT session, category, reported_at FROM core.perishable_unpacked WHERE category = ANY($4::text[])
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
    /// aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404. Reproduced by the
    /// signer core uses (rusty-s3).
    #[test]
    fn the_signer_reproduces_aws_published_presigned_example() {
        let bucket = Bucket::new("https://s3.amazonaws.com".parse().unwrap(), UrlStyle::VirtualHost, "examplebucket", "us-east-1").unwrap();
        let credentials = Credentials::new("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        let url = rusty_s3::actions::GetObject::new(&bucket, Some(&credentials), "test.txt").sign_with_time(std::time::Duration::from_secs(86400), &timestamp(1_369_353_600));
        assert_eq!(
            url.as_str(),
            "https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256\
             &X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
             &X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host\
             &X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
        );
    }

    #[test]
    fn a_url_is_path_style_and_signs_the_objects_headers() {
        let s = Spaces::new("https://nyc3.digitaloceanspaces.com/", "nyc3", "vyreon-data", "DO00EXAMPLEKEYID", "an-example-secret").unwrap();
        let key = Spaces::object_key("exq01-events", "2026-11-16", &"a".repeat(64));
        // 2026-11-17T13:00:00Z
        let url = s.presign_put(&key, &[("content-length", "42"), ("Content-MD5", "1B2M2Y8AsgTpgAmY7PhCfg=="), (META_SHA256, "ab")], 1_794_920_400, URL_SECONDS);
        assert!(url.starts_with(&format!("https://nyc3.digitaloceanspaces.com/vyreon-data/perishables/exq01-events/2026-11-16/{}.tar.zst?", "a".repeat(64))), "{url}");
        assert!(url.contains("&X-Amz-Expires=600&") && url.contains("X-Amz-SignedHeaders=content-length%3Bcontent-md5%3Bhost%3Bx-amz-meta-sha256&"), "{url}");
        assert!(url.contains("X-Amz-Credential=DO00EXAMPLEKEYID%2F20261117%2Fnyc3%2Fs3%2Faws4_request&") && url.contains("X-Amz-Date=20261117T130000Z&"), "{url}");
        let head = s.presign_head(&key, 1_794_920_400, 60);
        assert!(head.contains("X-Amz-SignedHeaders=host&") && head.contains("X-Amz-Expires=60&"), "{head}");
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
        let t = Target { archive_id: 1, endpoint: None, bucket: "b".into(), key: "k".into(), bytes: 42, sha256: "b".repeat(64), md5: "c".repeat(32), state: "REQUESTED".into() };
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
