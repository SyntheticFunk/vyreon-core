//! The worker API, under `/worker/v1`. JSON in and out unless noted.
//!
//!   POST /claims                     {kind, engine_build, worker[, daily]} -> the claim, or 204 when nothing is claimable;
//!                                                                          `daily: true` (the daily CronJob's worker) claims
//!                                                                          only a daily run's job (C-051 amended, round x 13)
//!   POST /lease/heartbeat                                               -> {expires_at, paused}
//!   POST /lease/finish                                                  -> {job_id}
//!   POST /lease/fail                 {stage, message, log}              -> {requeued}
//!   PUT  /evidence                   body: the attempt's failure evidence (application/gzip) -> {new} (C-049)
//!   PUT  /refcheck/{session}         body: the day's reference-check document (application/gzip) -> {new, report_id} (C-054)
//!   POST /days                       body: a day bundle                 -> what was published
//!   PUT  /tape-caches/{session}/{asset}?method_version=V[&absence=WHY]  body: the packed cache -> {new}
//!   POST /tape-caches/{session}/done[?method_version=V]                 the tape job's session is complete (under V; without
//!                                                                          it, under the method of the session's newest cache)
//!   GET  /tape-caches/{session}?method_version=V                        -> a tar: one entry per asset, absences.json, and
//!                                                                          verdicts.json (asset -> the tape worker's verdict, C-058)
//!   PUT  /tape-caches/{session}/{asset}/verdict?method_version=V        body: the tape verdict (JSON) -> {new} (C-058)
//!   PUT  /tape-caches/{session}/{asset}/check?method_version=V          body: a verify job's check (JSON) -> {outcome,
//!                                                                          verdict_kept, why, new} (C-059)
//!   PUT  /input-captures/{session}/{kind}/{identity}?captured_at=T      body: the capture's archive -> {new} (C-043)
//!   POST /input-captures/{session}/{kind}/absence  {reason}             a capture the worker could not keep -> {new}
//!   GET  /input-captures/{session}                                      -> {session, captures: [...]}: every version and absence
//!   GET  /input-captures/{session}/{kind}/{identity}                    -> the archive as it was sent (application/gzip)
//!   POST /archives                   {category, session, bytes, sha256, md5}   a perishable archive (C-052) -> {state: off | kept |
//!                                                                          frozen | upload, archive_id, key, url, headers, expires_seconds}
//!   POST /archives/{id}/done         {failed: null | why}               the upload is done (or failed) -> {kept, why}
//!   POST /archives/unpacked          {category, session, why}           a category the worker could not pack (round w, 7)
//!                                                                          -> {recorded} | {state: off}
//!   POST /budget                     {requests}                         -> {budget_lease_id, requests, valid_seconds}
//!                                                                          or {requests: 0, retry_after_seconds}
//!   POST /budget/{id}/release        {used: {category: requests}}       -> {counted}
//!
//! A claim carries, beside the job: `days`, for each session its members in profile order
//! (`ordinal, asset_key, security_id, ticker` (null when not listed that session), `group_name,
//! company`); `tickers`, for each member's asset key every ticker it traded under,
//! `[{ticker, from, to}]` oldest first, dates inclusive and `to` null while open (a day's history
//! windows cross ticker changes: Meta is FB to 2022-06-08, META from 2022-06-09); `sources`, each
//! provider source's first date; `tape_history_from`; `adjustments` (C-055), for each member with
//! one its dated OCC contract adjustments, the engine's `VYREON-ADJUSTMENTS@1` `assets`
//! (`crate::adjustments`), `{}` when none has one; `first_option_sessions` (C-057), for each member
//! with one the first session its option EOD chain holds a contract (`core.option_listing`), `{}`
//! when none has one. A day bundle states the members listed on
//! its day (`listed_members`, the asset keys with a ticker in `days`), and core refuses one that
//! differs from its own (C-036).
//!
//! An input capture (C-043) is one of a published day's company, macro or carry captures, as the
//! engine packed it: a gzip tar whose first entry is the capture's `manifest.json`, whose SHA-256 is
//! the `identity`, then the directory's other files. It is sent under a day block's lease, for a
//! session on or before the latest day the job has published; at most
//! `core_db::capture::MAX_ARCHIVE_BYTES` (413 above). The same identity again is answered
//! `new: false`; the same identity with other bytes is refused; another identity of the same
//! session and kind is kept as another version, with `captured_at` and the time core stored it.
//!
//! Failure evidence (C-049) is what a worker whose attempt failed or was refused leaves before it
//! exits, sent before its `fail` (or after a lost lease): a gzip of one JSON line (`schema`
//! `VYREON-JOB-EVIDENCE@1`, `stage`, `message`, `session`, `refusals`) and an excerpt of its logs
//! after it, credentials removed by the worker. Any lease the token names, live or ended, may send
//! it once; the same lease again is answered `new: false`. At most
//! `core_db::evidence::MAX_DOCUMENT_BYTES` (413 above); a document in which a credential pattern
//! survives is refused (422) and nothing is stored.
//!
//! A reference check (C-054) is what the worker found when it recomputed a day its job published
//! with the reference suite, before deleting the day's raw inputs: a gzip of one JSON line (`schema`
//! `VYREON-REFCHECK@1`, `session`, `outcome` COMPLETED | TIMEOUT | CRASHED | NOT_RUN, `why`, times,
//! `counts`, `summary`) and the suite's report as JSON after it (a completed check's; the account
//! alone otherwise), credentials removed by the worker. Sent under the live lease of the day block
//! that published the session (its base, its build), once per lease and session: the same again is
//! answered `new: false` with the first's id. At most `core_db::refcheck::MAX_DOCUMENT_BYTES` (413
//! above); a document in which a credential pattern survives is refused (422) and nothing is stored.
//!
//! A perishable archive (C-052) is one category of a day the job published (`core_db::archive::
//! CATEGORIES`), packed by the worker. Core decides the object's key and answers with a pre-signed
//! PUT URL valid for minutes, which signs the archive's length, its `Content-MD5` and its
//! `x-amz-meta-sha256`; the worker sends exactly those. The URL is a credential: the worker never
//! logs it and core never stores it. On `done` core reads the object back (a signed HEAD) and keeps
//! it only if its size, MD5 and SHA-256 are those requested; core cannot reach the bucket: 503,
//! sent again. With the archive off every request is answered `{state: "off"}`.
//!
//! Statuses: 401 no or wrong token; 410 the lease is gone (stop working); 422 the call or the
//! bundle is wrong (a refused bundle also pauses the queue); 5xx core or the database failed.
use crate::{ApiError, App, bearer, failed, refused};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use core_contract::sha256_hex;
use core_db::archive;
use core_db::budget::{self, Grant};
use core_db::capture;
use core_db::evidence;
use core_db::tape::{self, Cache};
use core_db::work::{self, Held, Kind};
use core_db::publish;
use core_db::refcheck;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;
use tokio_postgres::Client;

type Shared = State<Arc<App>>;
type Reply = Result<Response, ApiError>;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/claims", post(claim))
        .route("/lease/heartbeat", post(heartbeat))
        .route("/lease/finish", post(finish))
        .route("/lease/fail", post(fail))
        .route("/evidence", put(evidence_put))
        .route("/refcheck/{session}", put(refcheck_put))
        .route("/days", post(publish_day))
        .route("/days/{session}/done", post(day_done))
        .route("/tape-caches/{session}", get(tape_get))
        .route("/tape-caches/{session}/done", post(tape_done))
        .route("/tape-caches/{session}/{asset}", put(tape_put))
        .route("/tape-caches/{session}/{asset}/verdict", put(tape_verdict_put))
        .route("/tape-caches/{session}/{asset}/check", put(tape_check_put))
        .route("/input-captures/{session}", get(captures_list))
        .route("/input-captures/{session}/{kind}/absence", post(capture_absent))
        .route("/input-captures/{session}/{kind}/{identity}", put(capture_put).get(capture_get))
        .route("/archives", post(archive_request))
        .route("/archives/{id}/done", post(archive_done))
        .route("/archives/unpacked", post(archive_unpacked))
        .route("/budget", post(budget_draw))
        .route("/budget/{id}/release", post(budget_release))
}

fn ok(value: Value) -> Reply {
    Ok(Json(value).into_response())
}

/// Who is calling, from the lease token.
async fn caller(client: &Client, headers: &HeaderMap) -> Result<Held, ApiError> {
    Ok(work::identify(client, bearer(headers)?).await?)
}

#[derive(Deserialize)]
struct ClaimRequest {
    kind: String,
    engine_build: String,
    worker: String,
    #[serde(default)]
    daily: bool,
}

async fn claim(State(app): Shared, headers: HeaderMap, Json(request): Json<ClaimRequest>) -> Reply {
    if sha256_hex(bearer(&headers)?.as_bytes()) != app.config.worker_token_sha256 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "not the workers' token".into()));
    }
    let kind = Kind::parse(&request.kind).map_err(refused)?;
    // audit 2026-11-22 prebuild round 2 (core-watch.md) 5: the name is stored and posted, so one
    // that could carry a credential is refused before anything is stored
    work::worker_name(&request.worker).map_err(refused)?;
    let mut client = app.db().await?;
    let Some(c) = work::claim_as(&mut client, kind, &request.engine_build, &request.worker, app.config.lease_seconds, request.daily).await.map_err(failed)? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    // the member manifest: for each session, who is listed and under which ticker; and for each
    // member every ticker it has traded under, with its dates, since a day's history windows
    // reach back across ticker changes
    let manifest: String = client
        .query_one(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object('session', s.session, 'members',
                        (SELECT jsonb_agg(to_jsonb(m) ORDER BY m.ordinal) FROM core.members_on($1, s.session) m)) ORDER BY s.session), '[]'::jsonb)::text,
                    (SELECT jsonb_object_agg(source, first_date)::text FROM core.source_coverage),
                    (SELECT COALESCE(to_jsonb(min(first_session)), 'null'::jsonb)::text FROM core.job WHERE kind = 'TAPE' AND profile_id = $1 AND state <> 'CANCELLED'
                     AND NOT COALESCE(scope @> '{\"verify\": true}', false)),
                    (SELECT COALESCE(jsonb_object_agg(asset_key, spans), '{}'::jsonb)::text FROM (
                         SELECT m.asset_key,
                                jsonb_agg(jsonb_build_object('ticker', t.ticker, 'from', lower(t.during), 'to', upper(t.during) - 1) ORDER BY lower(t.during)) AS spans
                         FROM core.member m JOIN core.ticker_span t ON t.security_id = m.security_id
                         WHERE m.profile_id = $1 GROUP BY m.asset_key) x),
                    (SELECT COALESCE(jsonb_object_agg(m.asset_key, o.first_session ORDER BY m.ordinal), '{}'::jsonb)::text
                     FROM core.member m JOIN core.option_listing o USING (asset_key) WHERE m.profile_id = $1)
             FROM core.session s WHERE s.session = ANY($2::text[]::date[])",
            &[&c.profile_id, &c.sessions],
        )
        .await
        .map(|r| {
            format!(
                "{{\"days\":{},\"sources\":{},\"tape_history_from\":{},\"tickers\":{},\"first_option_sessions\":{}}}",
                r.get::<_, String>(0),
                r.get::<_, String>(1),
                r.get::<_, String>(2),
                r.get::<_, String>(3),
                r.get::<_, String>(4)
            )
        })
        .map_err(|e| failed(e.to_string()))?;
    let manifest: Value = serde_json::from_str(&manifest).map_err(|e| failed(e.to_string()))?;
    // C-055: the OCC adjustment ledger's entries of the same members
    let members = manifest["tickers"].as_object().into_iter().flat_map(|t| t.keys().map(String::as_str));
    let adjustments = crate::adjustments::for_members(members).map_err(failed)?;
    ok(json!({
        "job_id": c.job_id, "kind": c.kind, "profile_id": c.profile_id, "engine_build": c.engine_build, "sessions": c.sessions,
        "scope": c.scope.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|e| failed(e.to_string()))?,
        "attempt": c.attempt, "lease_token": c.token, "expires_at": c.expires_at, "lease_seconds": app.config.lease_seconds,
        "days": manifest["days"], "sources": manifest["sources"], "tickers": manifest["tickers"], "adjustments": adjustments,
        // C-057: each member's first session with a listed option chain, where core has the
        // provider's evidence of it (`core.option_listing`); `{}` when none has one
        "first_option_sessions": manifest["first_option_sessions"],
        // E4: where this database's tape history starts (its first scheduled tape session), so
        // every worker of a build reads the same history; null while no tape job is scheduled
        "tape_history_from": manifest["tape_history_from"],
    }))
}

async fn heartbeat(State(app): Shared, headers: HeaderMap) -> Reply {
    let mut client = app.db().await?;
    let (expires_at, paused) = work::heartbeat(&mut client, bearer(&headers)?, app.config.lease_seconds).await?;
    ok(json!({"expires_at": expires_at, "paused": paused}))
}

async fn finish(State(app): Shared, headers: HeaderMap) -> Reply {
    let mut client = app.db().await?;
    ok(json!({"job_id": work::finish(&mut client, bearer(&headers)?).await?}))
}

#[derive(Deserialize)]
struct Failure {
    stage: String,
    message: String,
    #[serde(default)]
    log: String,
}

async fn fail(State(app): Shared, headers: HeaderMap, Json(failure): Json<Failure>) -> Reply {
    let mut client = app.db().await?;
    let requeued = work::fail(&mut client, bearer(&headers)?, &failure.stage, &failure.message, &failure.log).await?;
    ok(json!({"requeued": requeued}))
}

async fn publish_day(State(app): Shared, headers: HeaderMap, body: Body) -> Reply {
    let token = bearer(&headers)?.to_owned();
    // C-068 (audit opsurface ops.md, the core node's suspicion): a database connection is held only
    // to name the caller, then to publish; never while the bundle uploads (minutes for ~200 MB) nor
    // while it waits for the bundle before it, so twelve publishing nodes hold one connection, not
    // twelve of the pool's
    let lease = caller(&*app.db().await?, &headers).await?;
    // spool the upload, then check and publish it; one bundle at a time
    let path = app.config.spool.join(format!("bundle-{}-{}.tar.gz", std::process::id(), app.spooled.fetch_add(1, Ordering::Relaxed)));
    let spooled = spool(body, &path, app.config.max_bundle_bytes).await;
    let result = match spooled {
        Err(e) => Err(e),
        Ok(()) => {
            let _one = app.publishing.lock().await;
            match app.db().await {
                Ok(mut client) => publish::publish_for_lease(&mut client, &path, &token).await.map_err(ApiError::from),
                Err(e) => Err(e),
            }
        }
    };
    let _ = tokio::fs::remove_file(&path).await;
    match result {
        Ok(p) => ok(json!({
            "revision_id": p.revision_id, "represented_date": p.represented_date, "profile_revision": p.profile_revision, "slots": p.slots,
            "streams": p.streams, "stream_bytes": p.stream_bytes, "reader_scalars": p.reader_scalars, "unusual_scores": p.unusual_scores,
            "superseded": p.superseded, "already_published": p.already_published,
        })),
        Err(e) => {
            if e.0 == StatusCode::UNPROCESSABLE_ENTITY {
                // a failed check: nothing more is claimed until someone has looked; the pause and
                // its notice are one transaction (C-027's outbox)
                let mut client = app.db().await?;
                let client: &mut Client = &mut client;
                let tx = client.transaction().await.map_err(|e| failed(e.to_string()))?;
                let what = format!("job {} ({}): bundle refused: {}", lease.job_id, lease.worker, e.1);
                work::pause(&tx, &what).await.map_err(failed)?;
                // C-072 round 3: alerted now, in the refusal's transaction, whatever the pause becomes
                core_db::watch::refused(&tx, &what).await.map_err(failed)?;
                tx.commit().await.map_err(|e| failed(e.to_string()))?;
            }
            Err(e)
        }
    }
}

/// Writes a request body to a file, refusing more than `limit` bytes.
async fn spool(body: Body, path: &std::path::Path, limit: u64) -> Result<(), ApiError> {
    let mut file = tokio::fs::File::create(path).await.map_err(|e| failed(format!("spool: {e}")))?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("upload: {e}")))?;
        written += chunk.len() as u64;
        if written > limit {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, format!("a bundle is at most {limit} bytes")));
        }
        file.write_all(&chunk).await.map_err(|e| failed(format!("spool: {e}")))?;
    }
    file.flush().await.map_err(|e| failed(format!("spool: {e}")))
}

#[derive(Deserialize)]
struct TapeQuery {
    method_version: String,
    absence: Option<String>,
}

/// The cache, and its verdict below, are stored in the transaction that holds the lease (locked:
/// `work::held`), so a lease cancelled or expired after its check stores nothing (round cc, 4).
async fn tape_put(State(app): Shared, headers: HeaderMap, Path((session, asset)): Path<(String, String)>, Query(q): Query<TapeQuery>, body: axum::body::Bytes) -> Reply {
    let mut client = app.db().await?;
    let client: &mut Client = &mut client;
    let tx = client.transaction().await.map_err(|e| failed(e.to_string()))?;
    let lease = work::held(&tx, bearer(&headers)?).await?;
    if lease.kind != "TAPE" || lease.next_session.as_deref() != Some(session.as_str()) {
        return Err(refused(format!("job {} is not the tape job of {session}", lease.job_id)));
    }
    verify_stores_nothing(&tx, lease.job_id).await?;
    let cache = match q.absence {
        Some(reason) if body.is_empty() => Cache::Absent(reason),
        Some(_) => return Err(refused("an absence carries no body")),
        None => Cache::Body(body.to_vec()),
    };
    let new = tape::put(&tx, &asset, &session, &q.method_version, &cache).await.map_err(refused)?;
    tx.commit().await.map_err(|e| failed(e.to_string()))?;
    ok(json!({"new": new}))
}

#[derive(Deserialize)]
struct VerdictQuery {
    method_version: String,
}

/// C-058: the tape worker's verdict on a cache it stored, under the session's tape job's lease,
/// kept once beside the cache (`tape::put_verdict`).
async fn tape_verdict_put(State(app): Shared, headers: HeaderMap, Path((session, asset)): Path<(String, String)>, Query(q): Query<VerdictQuery>, body: Body) -> Reply {
    let bytes = read_limited(body, tape::MAX_VERDICT_BYTES, "a tape verdict").await?;
    let verdict = tape::check_verdict(&bytes, &asset, &session, &q.method_version).map_err(refused)?;
    let mut client = app.db().await?;
    let client: &mut Client = &mut client;
    let tx = client.transaction().await.map_err(|e| failed(e.to_string()))?;
    let lease = work::held(&tx, bearer(&headers)?).await?;
    if lease.kind != "TAPE" || lease.next_session.as_deref() != Some(session.as_str()) {
        return Err(refused(format!("job {} is not the tape job of {session}", lease.job_id)));
    }
    verify_stores_nothing(&tx, lease.job_id).await?;
    let new = tape::put_verdict(&tx, &asset, &session, &q.method_version, &verdict).await.map_err(refused)?;
    tx.commit().await.map_err(|e| failed(e.to_string()))?;
    ok(json!({"new": new}))
}

/// C-059: a verify job stores no cache and no verdict of its own: what it found goes in its checks.
async fn verify_stores_nothing(client: &impl tokio_postgres::GenericClient, job: i64) -> Result<(), ApiError> {
    if tape::is_verify(client, job).await.map_err(failed)? {
        return Err(refused(format!("job {job} verifies the caches core holds (C-059): it stores none, and sends its checks")));
    }
    Ok(())
}

/// C-059: a verify job's check of one member's cache: core compares the recomputed cache's digest
/// with the cache it holds and keeps the verdict beside it only when they are equal
/// (`tape::put_check`).
async fn tape_check_put(State(app): Shared, headers: HeaderMap, Path((session, asset)): Path<(String, String)>, Query(q): Query<VerdictQuery>, body: Body) -> Reply {
    let bytes = read_limited(body, tape::MAX_CHECK_BYTES, "a tape check").await?;
    let check = tape::check_check(&bytes, &asset, &session, &q.method_version).map_err(refused)?;
    let mut client = app.db().await?;
    let c = tape::put_check(&mut client, bearer(&headers)?, &session, &asset, &q.method_version, &check).await?;
    ok(json!({"outcome": c.outcome, "verdict_kept": c.verdict_kept, "why": c.why, "new": c.new}))
}

#[derive(Deserialize)]
struct DoneQuery {
    method_version: Option<String>,
}

async fn tape_done(State(app): Shared, headers: HeaderMap, Path(session): Path<String>, Query(q): Query<DoneQuery>) -> Reply {
    let mut client = app.db().await?;
    if !core_contract::is_date(&session) {
        return Err(refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    tape::session_done(&mut client, bearer(&headers)?, &session, q.method_version.as_deref()).await?;
    ok(json!({"session": session}))
}

async fn day_done(State(app): Shared, headers: HeaderMap, Path(session): Path<String>) -> Reply {
    let mut client = app.db().await?;
    if !core_contract::is_date(&session) {
        return Err(refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    work::day_done(&mut client, bearer(&headers)?, &session).await?;
    ok(json!({"session": session}))
}

async fn tape_get(State(app): Shared, headers: HeaderMap, Path(session): Path<String>, Query(q): Query<TapeQuery>) -> Reply {
    let client = app.db().await?;
    caller(&client, &headers).await?;
    let caches = tape::of_session(&client, &session, &q.method_version).await.map_err(failed)?;
    let mut archive = tar::Builder::new(Vec::new());
    let mut add = |name: &str, bytes: &[u8]| {
        let mut head = tar::Header::new_gnu();
        head.set_size(bytes.len() as u64);
        head.set_mode(0o644);
        head.set_cksum();
        archive.append_data(&mut head, name, bytes).map_err(|e| failed(e.to_string()))
    };
    let mut absences = serde_json::Map::new();
    for (asset, cache) in &caches {
        match cache {
            Cache::Body(bytes) => add(asset, bytes)?,
            Cache::Absent(reason) => {
                absences.insert(asset.clone(), json!(reason));
            }
        }
    }
    add("absences.json", Value::Object(absences).to_string().as_bytes())?;
    // C-058: the tape worker's verdict on each cache, beside it (an engine before it reads none)
    let verdicts: serde_json::Map<String, Value> = tape::verdicts_of_session(&client, &session, &q.method_version).await.map_err(failed)?.into_iter().collect();
    add("verdicts.json", Value::Object(verdicts).to_string().as_bytes())?;
    let bytes = archive.into_inner().map_err(|e| failed(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/x-tar")], bytes).into_response())
}

#[derive(Deserialize)]
struct CaptureQuery {
    captured_at: String,
}

/// Reads a request body into memory, refusing more than `limit` bytes (413).
async fn read_limited(body: Body, limit: usize, what: &str) -> Result<Vec<u8>, ApiError> {
    let mut stream = body.into_data_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("upload: {e}")))?;
        if out.len() + chunk.len() > limit {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, format!("{what} is at most {limit} bytes")));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn capture_put(State(app): Shared, headers: HeaderMap, Path((session, kind, identity)): Path<(String, String, String)>, Query(q): Query<CaptureQuery>, body: Body) -> Reply {
    let token = bearer(&headers)?.to_owned();
    let archive = read_limited(body, capture::MAX_ARCHIVE_BYTES, "an input capture").await?;
    // the archive is read to its end to check it (tens of MB unpacked): off the async threads
    let checked = tokio::task::spawn_blocking(move || capture::check(archive, &identity)).await.map_err(|e| failed(e.to_string()))?.map_err(refused)?;
    let mut client = app.db().await?;
    let new = capture::put(&mut client, &token, &session, &kind, &capture::Capture::Archive { checked, captured_at: q.captured_at }).await?;
    ok(json!({"new": new}))
}

async fn evidence_put(State(app): Shared, headers: HeaderMap, body: Body) -> Reply {
    let token = bearer(&headers)?.to_owned();
    let document = read_limited(body, evidence::MAX_DOCUMENT_BYTES, "a failure's evidence").await?;
    // read to its end to check it (some MiB unpacked): off the async threads
    let checked = tokio::task::spawn_blocking(move || evidence::check(document)).await.map_err(|e| failed(e.to_string()))?.map_err(refused)?;
    let mut client = app.db().await?;
    ok(json!({"new": evidence::put(&mut client, &token, &checked).await?}))
}

async fn refcheck_put(State(app): Shared, headers: HeaderMap, Path(session): Path<String>, body: Body) -> Reply {
    let token = bearer(&headers)?.to_owned();
    if !core_contract::is_date(&session) {
        return Err(refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    let document = read_limited(body, refcheck::MAX_DOCUMENT_BYTES, "a reference check's document").await?;
    // read to its end and its report parsed to check it (tens of MB unpacked): off the async threads
    let checked = tokio::task::spawn_blocking(move || refcheck::check(document, &session)).await.map_err(|e| failed(e.to_string()))?.map_err(refused)?;
    let mut client = app.db().await?;
    let (new, id) = refcheck::put(&mut client, &token, &checked).await?;
    ok(json!({"new": new, "report_id": id}))
}

#[derive(Deserialize)]
struct Absence {
    reason: String,
}

async fn capture_absent(State(app): Shared, headers: HeaderMap, Path((session, kind)): Path<(String, String)>, Json(absence): Json<Absence>) -> Reply {
    let mut client = app.db().await?;
    let new = capture::put(&mut client, bearer(&headers)?, &session, &kind, &capture::Capture::Absent(absence.reason)).await?;
    ok(json!({"new": new}))
}

async fn captures_list(State(app): Shared, headers: HeaderMap, Path(session): Path<String>) -> Reply {
    let client = app.db().await?;
    caller(&client, &headers).await?;
    ok(json!({"session": session, "captures": capture::list(&client, &session).await.map_err(refused)?}))
}

async fn capture_get(State(app): Shared, headers: HeaderMap, Path((session, kind, identity)): Path<(String, String, String)>) -> Reply {
    let client = app.db().await?;
    caller(&client, &headers).await?;
    match capture::archive(&client, &session, &kind, &identity).await.map_err(failed)? {
        Some(bytes) => Ok(([(header::CONTENT_TYPE, "application/gzip")], bytes).into_response()),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no {kind} capture {identity} of {session}"))),
    }
}

#[derive(Deserialize)]
struct ArchiveRequest {
    category: String,
    session: String,
    bytes: i64,
    sha256: String,
    md5: String,
}

async fn archive_request(State(app): Shared, headers: HeaderMap, Json(r): Json<ArchiveRequest>) -> Reply {
    let mut client = app.db().await?;
    let Some(spaces) = &app.config.archive else {
        caller(&client, &headers).await?;
        return ok(json!({"state": "off"}));
    };
    let now = core_db::watch::now(&**client).await.map_err(failed)?;
    let request = archive::Request { category: r.category, session: r.session, bytes: r.bytes, sha256: r.sha256, md5: r.md5 };
    ok(archive::request(&mut client, spaces, bearer(&headers)?, &request, &now).await?.reply())
}

#[derive(Deserialize)]
struct ArchiveUnpacked {
    category: String,
    session: String,
    why: String,
}

/// A category the worker could not pack (round w, 7): counted missing, never silent.
async fn archive_unpacked(State(app): Shared, headers: HeaderMap, Json(u): Json<ArchiveUnpacked>) -> Reply {
    let mut client = app.db().await?;
    if app.config.archive.is_none() {
        caller(&client, &headers).await?;
        return ok(json!({"state": "off"}));
    }
    let now = core_db::watch::now(&**client).await.map_err(failed)?;
    archive::unpacked(&mut client, bearer(&headers)?, &u.category, &u.session, &u.why, &now).await?;
    ok(json!({"recorded": true}))
}

#[derive(Deserialize)]
struct ArchiveDone {
    #[serde(default)]
    failed: Option<String>,
}

async fn archive_done(State(app): Shared, headers: HeaderMap, Path(id): Path<i64>, Json(d): Json<ArchiveDone>) -> Reply {
    let mut client = app.db().await?;
    let Some(spaces) = &app.config.archive else {
        return Err(refused("the perishable archive is off"));
    };
    let token = bearer(&headers)?;
    let target = archive::target(&**client, token, id).await?;
    let now = core_db::watch::now(&**client).await.map_err(failed)?;
    let outcome = match d.failed {
        Some(why) => Err(format!("the worker: {why}")),
        None if target.state == "KEPT" => Ok(()),
        // read back only where its URL was signed for (round v, 3)
        None => match archive::same_place(&target, spaces) {
            Err(e) => Err(e),
            Ok(()) => {
                let at = archive::signing_time(&**client, &now).await.map_err(failed)?;
                let head = head_object(spaces, &target.key, at).await.map_err(|e| ApiError(StatusCode::SERVICE_UNAVAILABLE, format!("reading the object back from the bucket: {e}")))?;
                archive::verify(&target, &head)
            }
        },
    };
    let kept = archive::record(&mut client, token, &target, &outcome, &now).await?;
    ok(json!({"archive_id": id, "kept": kept, "why": outcome.err().map(|e| archive::failure_text(&e))}))
}

/// Reads an object's size, ETag and `x-amz-meta-sha256` back from the bucket (a HEAD pre-signed
/// for a minute). An error never carries the URL.
async fn head_object(spaces: &archive::Spaces, key: &str, at: i64) -> Result<archive::Head, String> {
    let url = spaces.presign_head(key, at, 60);
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(30)).build().map_err(|e| e.without_url().to_string())?;
    let response = http.head(url).send().await.map_err(|e| e.without_url().to_string())?;
    let text = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    Ok(archive::Head {
        status: response.status().as_u16(),
        bytes: text("content-length").and_then(|v| v.parse().ok()),
        etag: text("etag"),
        sha256: text(archive::META_SHA256),
    })
}

#[derive(Deserialize)]
struct Draw {
    requests: i32,
}

async fn budget_draw(State(app): Shared, headers: HeaderMap, Json(draw): Json<Draw>) -> Reply {
    let mut client = app.db().await?;
    let lease = caller(&client, &headers).await?;
    match budget::draw(&mut client, app.config.budget, &lease.worker, draw.requests).await.map_err(refused)? {
        Grant::Granted { budget_lease_id, requests, valid_seconds } => ok(json!({"budget_lease_id": budget_lease_id, "requests": requests, "valid_seconds": valid_seconds})),
        Grant::Wait { seconds } => ok(json!({"requests": 0, "retry_after_seconds": seconds})),
    }
}

#[derive(Deserialize)]
struct Release {
    used: Value,
}

async fn budget_release(State(app): Shared, headers: HeaderMap, Path(id): Path<i64>, Json(release): Json<Release>) -> Reply {
    let mut client = app.db().await?;
    let lease = caller(&client, &headers).await?;
    ok(json!({"counted": budget::release(&mut client, id, &lease.worker, &release.used).await.map_err(refused)?}))
}
