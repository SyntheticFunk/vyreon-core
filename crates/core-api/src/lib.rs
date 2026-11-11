//! vyreon-core's HTTP service. `worker` is the private API the engine's workers use (ARCHITECTURE
//! 4.1): claim a job, renew the lease, draw request budget, read and write tape caches, publish a
//! day, finish or fail. `reader` is what vyreon-reporter reads.
//!
//! Who may call: a claim carries the workers' shared token (core is given only its SHA-256); the
//! claim returns a lease token, and every other call carries that. Both travel as
//! `Authorization: Bearer <token>`. Workers hold no database or storage credentials (C-016).
pub mod reader;
pub mod worker;

use axum::Json;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use core_db::budget::Budget;
use core_db::work::LeaseError;
use deadpool_postgres::{Object, Pool};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

/// The service's settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// SHA-256 (lowercase hex) of the token workers present to claim work.
    pub worker_token_sha256: String,
    /// Where a bundle is kept while it is checked.
    pub spool: PathBuf,
    /// How long a job lease lasts without a heartbeat.
    pub lease_seconds: f64,
    pub budget: Budget,
    /// The largest bundle accepted.
    pub max_bundle_bytes: u64,
}

pub struct App {
    pub pool: Pool,
    pub config: Config,
    /// One publish at a time: a day's bundle is large in memory while it is checked (measured
    /// ~0.7 GB here and ~0.6 GB in its database backend), and takes seconds.
    publishing: tokio::sync::Mutex<()>,
    spooled: AtomicU64,
}

impl App {
    pub fn new(pool: Pool, config: Config) -> Arc<App> {
        Arc::new(App { pool, config, publishing: tokio::sync::Mutex::new(()), spooled: AtomicU64::new(0) })
    }

    async fn db(&self) -> Result<Object, ApiError> {
        self.pool.get().await.map_err(|e| ApiError(StatusCode::SERVICE_UNAVAILABLE, format!("database: {e}")))
    }
}

/// The router for every route core serves.
pub fn router(app: Arc<App>) -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(ready))
        .nest("/worker/v1", worker::routes())
        .nest("/v1", reader::routes().layer(tower_http::compression::CompressionLayer::new()))
        .with_state(app)
}

async fn ready(axum::extract::State(app): axum::extract::State<Arc<App>>) -> Result<&'static str, ApiError> {
    let client = app.db().await?;
    client.query_one("SELECT 1", &[]).await.map_err(|e| ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    Ok("ready")
}

/// Ends leases that were not renewed, every half minute, so their jobs become claimable (and
/// counted for the autoscaler) without waiting for the next claim.
pub fn spawn_sweeper(app: Arc<App>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Ok(client) = app.pool.get().await {
                let client: &tokio_postgres::Client = &client;
                if let Err(e) = core_db::work::sweep(client).await {
                    eprintln!("vyreon-core: lease sweep: {e}");
                }
            }
        }
    })
}

/// An error as the API returns it: a status and `{"error": "..."}`.
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<LeaseError> for ApiError {
    fn from(e: LeaseError) -> ApiError {
        match e {
            // 410: the lease is gone for good; the worker must stop
            LeaseError::Gone => ApiError(StatusCode::GONE, "no live lease has this token".into()),
            LeaseError::Refused(m) => ApiError(StatusCode::UNPROCESSABLE_ENTITY, m),
            LeaseError::Failed(m) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, m),
        }
    }
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, message.into())
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, message.into())
}

/// The bearer token of a request.
fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "send Authorization: Bearer <token>".into()))
}
