//! The worker API, under `/worker/v1`. JSON in and out unless noted.
//!
//!   POST /claims                     {kind, engine_build, worker}       -> the claim, or 204 when nothing is claimable
//!   POST /lease/heartbeat                                               -> {expires_at, paused}
//!   POST /lease/finish                                                  -> {job_id}
//!   POST /lease/fail                 {stage, message, log}              -> {requeued}
//!   POST /days                       body: a day bundle                 -> what was published
//!   PUT  /tape-caches/{session}/{asset}?method_version=V[&absence=WHY]  body: the packed cache -> {new}
//!   POST /tape-caches/{session}/done                                    the tape job's session is complete
//!   GET  /tape-caches/{session}?method_version=V                        -> a tar: one entry per asset, and absences.json
//!   POST /budget                     {requests}                         -> {budget_lease_id, requests, valid_seconds}
//!                                                                          or {requests: 0, retry_after_seconds}
//!   POST /budget/{id}/release        {used: {category: requests}}       -> {counted}
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
use core_db::budget::{self, Grant};
use core_db::tape::{self, Cache};
use core_db::work::{self, Held, Kind};
use core_db::publish;
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
        .route("/days", post(publish_day))
        .route("/days/{session}/done", post(day_done))
        .route("/tape-caches/{session}", get(tape_get))
        .route("/tape-caches/{session}/done", post(tape_done))
        .route("/tape-caches/{session}/{asset}", put(tape_put))
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
}

async fn claim(State(app): Shared, headers: HeaderMap, Json(request): Json<ClaimRequest>) -> Reply {
    if sha256_hex(bearer(&headers)?.as_bytes()) != app.config.worker_token_sha256 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "not the workers' token".into()));
    }
    let kind = Kind::parse(&request.kind).map_err(refused)?;
    if request.worker.trim().is_empty() {
        return Err(refused("a claim names its worker"));
    }
    let mut client = app.db().await?;
    let Some(c) = work::claim(&mut client, kind, &request.engine_build, &request.worker, app.config.lease_seconds).await.map_err(failed)? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    // the member manifest: for each session, who is listed and under which ticker
    let manifest: String = client
        .query_one(
            "SELECT COALESCE(jsonb_agg(jsonb_build_object('session', s.session, 'members',
                        (SELECT jsonb_agg(to_jsonb(m) ORDER BY m.ordinal) FROM core.members_on($1, s.session) m)) ORDER BY s.session), '[]'::jsonb)::text,
                    (SELECT jsonb_object_agg(source, first_date)::text FROM core.source_coverage),
                    (SELECT COALESCE(to_jsonb(min(first_session)), 'null'::jsonb)::text FROM core.job WHERE kind = 'TAPE' AND profile_id = $1 AND state <> 'CANCELLED')
             FROM core.session s WHERE s.session = ANY($2::text[]::date[])",
            &[&c.profile_id, &c.sessions],
        )
        .await
        .map(|r| format!("{{\"days\":{},\"sources\":{},\"tape_history_from\":{}}}", r.get::<_, String>(0), r.get::<_, String>(1), r.get::<_, String>(2)))
        .map_err(|e| failed(e.to_string()))?;
    let manifest: Value = serde_json::from_str(&manifest).map_err(|e| failed(e.to_string()))?;
    ok(json!({
        "job_id": c.job_id, "kind": c.kind, "profile_id": c.profile_id, "engine_build": c.engine_build, "sessions": c.sessions,
        "scope": c.scope.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|e| failed(e.to_string()))?,
        "attempt": c.attempt, "lease_token": c.token, "expires_at": c.expires_at, "lease_seconds": app.config.lease_seconds,
        "days": manifest["days"], "sources": manifest["sources"],
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
    let mut client = app.db().await?;
    let lease = caller(&client, &headers).await?;
    // spool the upload, then check and publish it; one bundle at a time
    let path = app.config.spool.join(format!("bundle-{}-{}.tar.gz", std::process::id(), app.spooled.fetch_add(1, Ordering::Relaxed)));
    let spooled = spool(body, &path, app.config.max_bundle_bytes).await;
    let result = match spooled {
        Err(e) => Err(e),
        Ok(()) => {
            let _one = app.publishing.lock().await;
            publish::publish_for_lease(&mut client, &path, &token).await.map_err(ApiError::from)
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
                // a failed check: nothing more is claimed until someone has looked
                let client: &Client = &client;
                work::pause(client, &format!("job {} ({}): bundle refused: {}", lease.job_id, lease.worker, e.1)).await.map_err(failed)?;
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

async fn tape_put(State(app): Shared, headers: HeaderMap, Path((session, asset)): Path<(String, String)>, Query(q): Query<TapeQuery>, body: axum::body::Bytes) -> Reply {
    let client = app.db().await?;
    let lease = caller(&client, &headers).await?;
    if lease.kind != "TAPE" || lease.next_session.as_deref() != Some(session.as_str()) {
        return Err(refused(format!("job {} is not the tape job of {session}", lease.job_id)));
    }
    let cache = match q.absence {
        Some(reason) if body.is_empty() => Cache::Absent(reason),
        Some(_) => return Err(refused("an absence carries no body")),
        None => Cache::Body(body.to_vec()),
    };
    ok(json!({"new": tape::put(&client, &asset, &session, &q.method_version, &cache).await.map_err(refused)?}))
}

async fn tape_done(State(app): Shared, headers: HeaderMap, Path(session): Path<String>) -> Reply {
    let mut client = app.db().await?;
    if !core_contract::is_date(&session) {
        return Err(refused(format!("{session} is not a date written YYYY-MM-DD")));
    }
    tape::session_done(&mut client, bearer(&headers)?, &session).await?;
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
    let bytes = archive.into_inner().map_err(|e| failed(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/x-tar")], bytes).into_response())
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
