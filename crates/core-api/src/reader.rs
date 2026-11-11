//! The reader API, under `/v1`: what vyreon-reporter (later vyreon-web) reads (C-009, C-020).
//! Read-only; published days only. Every request carries `Authorization: Bearer <key>` for a key
//! with the `read` scope (`admin` for `/v1/status`).
//!
//!   GET /v1/contract                                      the contract version, the calendar's range, the profiles, the latest session
//!   GET /v1/sessions?from=A&to=B                          every trading session from A to B: PUBLISHED with its revisions, or NOT_PUBLISHED
//!   GET /v1/sessions/{date|latest}                        one session, with each revision's slots
//!   GET /v1/assets/{object}/history?from=A&to=B[&detail=full|headline][&measurement=ID]
//!                                                         the object's measurements, session by session
//!   GET /v1/assets/{object}/latest[?detail=...]           the same for the latest published session
//!   GET /v1/revisions/{rev}/slots/{mid}/{obj}             the exact body the engine published
//!   GET /v1/revisions/{rev}/slots/{mid}/{obj}/stream/root     its event stream's root
//!   GET /v1/revisions/{rev}/slots/{mid}/{obj}/stream/events   and events (gzip)
//!   GET /v1/status                                        the queue, the budget and the store at a glance
//!
//! Rules a reader can rely on:
//!   * sessions come from the trading calendar, never from the data; a session without a
//!     published day is listed as NOT_PUBLISHED, never left out and never filled;
//!   * one response is read from one snapshot of the database: it never mixes revisions of a day;
//!   * `detail=full` (the default) carries each measurement's complete body, value-exact; the
//!     body route returns the engine's bytes, and sha256(bytes) is `body_sha256`;
//!   * `detail=headline` carries each measurement's reader scalars (`role_id: value`), the
//!     supported numbers only: an absent role is in the body, with its state and reason;
//!   * a long range is cut at `limit` sessions and `next` names the session to continue from;
//!   * measurements are in the profile's slot order.
use crate::{ApiError, App, bearer, failed};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use core_contract::sha256_hex;
use core_db::{api_key, budget, work};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_postgres::Client;

pub const CONTRACT: &str = "VYREON-CORE-API@1";
/// The most sessions one history response carries.
const FULL_SESSIONS: i64 = 20;
const HEADLINE_SESSIONS: i64 = 260;
const LISTED_SESSIONS: i64 = 400;

type Shared = State<Arc<App>>;
type Reply = Result<Response, ApiError>;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/contract", get(contract))
        .route("/sessions", get(sessions))
        .route("/sessions/{date}", get(session))
        .route("/assets/{object}/history", get(history))
        .route("/assets/{object}/latest", get(latest))
        .route("/revisions/{revision}/slots/{measurement}/{object}", get(body))
        .route("/revisions/{revision}/slots/{measurement}/{object}/stream/root", get(stream_root))
        .route("/revisions/{revision}/slots/{measurement}/{object}/stream/events", get(stream_events))
        .route("/status", get(status))
}

fn bad(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.into())
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, message.into())
}

fn db(e: tokio_postgres::Error) -> ApiError {
    // a date PostgreSQL cannot read is the caller's mistake
    match e.code().map(|c| &c.code()[..2]) {
        Some("22") => bad(e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())),
        _ => failed(e.to_string()),
    }
}

/// The caller's key must be live and hold `scope`.
async fn allowed(client: &Client, headers: &HeaderMap, scope: &str) -> Result<(), ApiError> {
    match api_key::holder(client, bearer(headers)?).await.map_err(failed)? {
        None => Err(ApiError(StatusCode::UNAUTHORIZED, "unknown or revoked key".into())),
        Some((_, scopes)) if scopes.iter().any(|s| s == scope) => Ok(()),
        Some((name, _)) => Err(ApiError(StatusCode::FORBIDDEN, format!("{name}'s key does not have the {scope} scope"))),
    }
}

/// Dates are read as YYYY-MM-DD only.
fn dates(texts: &[&str]) -> Result<(), ApiError> {
    match texts.iter().find(|t| !core_contract::is_date(t)) {
        Some(t) => Err(bad(format!("{t} is not a date written YYYY-MM-DD"))),
        None => Ok(()),
    }
}

fn json_text(text: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], text).into_response()
}

async fn contract(State(app): Shared, headers: HeaderMap) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    let row = client
        .query_one(
            "SELECT jsonb_build_object(
                 'contract_version', $1::text,
                 'calendar', jsonb_build_object('first', (SELECT min(session) FROM core.session), 'last', (SELECT max(session) FROM core.session)),
                 'latest_session', (SELECT max(represented_date) FROM vyreon.day_revision WHERE state = 'PUBLISHED'),
                 'profiles', COALESCE((SELECT jsonb_agg(p ORDER BY p->>'profile_revision')
                                       FROM (SELECT DISTINCT jsonb_build_object('profile_id', profile_id, 'profile_revision', profile_revision,
                                                                                'computed_from_a_base', base_revision IS NOT NULL) AS p
                                             FROM vyreon.day_revision WHERE state = 'PUBLISHED') x), '[]'::jsonb),
                 'limits', jsonb_build_object('history_full_sessions', $2::bigint, 'history_headline_sessions', $3::bigint, 'listed_sessions', $4::bigint))::text",
            &[&CONTRACT, &FULL_SESSIONS, &HEADLINE_SESSIONS, &LISTED_SESSIONS],
        )
        .await
        .map_err(db)?;
    Ok(json_text(row.get(0)))
}

/// One session's entry: its state and published revisions (optionally each revision's slots).
const SESSION_ENTRY: &str = "jsonb_build_object(
    'date', s.session,
    'state', CASE WHEN EXISTS (SELECT FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.state = 'PUBLISHED') THEN 'PUBLISHED' ELSE 'NOT_PUBLISHED' END,
    'revisions', COALESCE((
        SELECT jsonb_agg(jsonb_build_object(
                   'revision_id', d.revision_id, 'profile_id', d.profile_id, 'profile_revision', d.profile_revision, 'engine_build', d.engine_build,
                   'published_at', d.published_at, 'snapshot_cutoff', d.snapshot_cutoff, 'base_revision', d.base_revision,
                   'slot_count', (SELECT count(*) FROM vyreon.measurement m WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date))
                   || CASE WHEN $3 THEN jsonb_build_object('slots', (
                          SELECT jsonb_agg(jsonb_build_object('measurement_id', m.measurement_id, 'object_id', m.object_id, 'slot_state', m.slot_state,
                                                              'body_sha256', m.body_sha256, 'body_bytes', octet_length(m.body_text) + 1,
                                                              'stream_root_sha256', st.root_sha256) ORDER BY e.slot_ordinal)
                          FROM vyreon.measurement m
                          LEFT JOIN core.expected_slot e ON e.engine_build = d.engine_build AND e.profile_revision = d.profile_revision
                                                        AND e.measurement_id = m.measurement_id AND e.object_id = m.object_id
                          LEFT JOIN vyreon.measurement_stream st ON st.revision_id = m.revision_id AND st.represented_date = m.represented_date
                                                                AND st.measurement_id = m.measurement_id AND st.object_id = m.object_id
                          WHERE m.revision_id = d.revision_id AND m.represented_date = d.represented_date))
                      ELSE '{}'::jsonb END
               ORDER BY d.base_revision NULLS FIRST, d.revision_id)
        FROM vyreon.day_revision d WHERE d.represented_date = s.session AND d.state = 'PUBLISHED'), '[]'::jsonb))";

#[derive(Deserialize)]
struct Range {
    from: String,
    to: String,
}

async fn sessions(State(app): Shared, headers: HeaderMap, Query(range): Query<Range>) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    dates(&[&range.from, &range.to])?;
    let row = client
        .query_one(
            &format!(
                "SELECT count(*), COALESCE(jsonb_agg({SESSION_ENTRY} ORDER BY s.session), '[]'::jsonb)::text
                 FROM core.session s WHERE s.session BETWEEN $1::text::date AND $2::text::date"
            ),
            &[&range.from, &range.to, &false],
        )
        .await
        .map_err(db)?;
    if row.get::<_, i64>(0) > LISTED_SESSIONS {
        return Err(bad(format!("{} sessions asked for; at most {LISTED_SESSIONS} a request", row.get::<_, i64>(0))));
    }
    Ok(json_text(format!("{{\"contract_version\":\"{CONTRACT}\",\"sessions\":{}}}", row.get::<_, String>(1))))
}

/// The latest published session.
async fn latest_session(client: &Client) -> Result<String, ApiError> {
    client
        .query_one("SELECT max(represented_date)::text FROM vyreon.day_revision WHERE state = 'PUBLISHED'", &[])
        .await
        .map_err(db)?
        .get::<_, Option<String>>(0)
        .ok_or_else(|| not_found("no session is published yet"))
}

async fn session(State(app): Shared, headers: HeaderMap, Path(date): Path<String>) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    let date = if date == "latest" { latest_session(&client).await? } else { date };
    dates(&[&date])?;
    let row = client
        .query_opt(&format!("SELECT ({SESSION_ENTRY})::text FROM core.session s WHERE s.session = $1::text::date AND $2::text IS NULL"), &[&date, &None::<String>, &true])
        .await
        .map_err(db)?
        .ok_or_else(|| not_found(format!("{date} is not a trading session")))?;
    Ok(json_text(format!("{{\"contract_version\":\"{CONTRACT}\",\"session\":{}}}", row.get::<_, String>(0))))
}

#[derive(Deserialize)]
struct HistoryQuery {
    from: Option<String>,
    to: Option<String>,
    detail: Option<String>,
    measurement: Option<String>,
}

#[derive(Serialize)]
struct Measurement {
    measurement_id: String,
    slot_state: String,
    science_version: String,
    method_version: String,
    body_sha256: String,
    stream_root_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scalars: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<Box<RawValue>>,
}

#[derive(Serialize)]
struct Revision {
    revision_id: i64,
    profile_revision: String,
    engine_build: String,
    published_at: String,
    snapshot_cutoff: String,
    base_revision: Option<i64>,
    measurements: Vec<Measurement>,
}

#[derive(Serialize)]
struct Session {
    date: String,
    state: &'static str,
    revisions: Vec<Revision>,
}

async fn latest(State(app): Shared, headers: HeaderMap, Path(object): Path<String>, Query(mut q): Query<HistoryQuery>) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    let date = latest_session(&client).await?;
    (q.from, q.to) = (Some(date.clone()), Some(date));
    read_history(&client, &headers, &object, q).await
}

async fn history(State(app): Shared, headers: HeaderMap, Path(object): Path<String>, Query(q): Query<HistoryQuery>) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    read_history(&client, &headers, &object, q).await
}

async fn read_history(client: &Client, headers: &HeaderMap, object: &str, q: HistoryQuery) -> Reply {
    let (Some(from), Some(to)) = (&q.from, &q.to) else { return Err(bad("history needs from and to (YYYY-MM-DD)")) };
    dates(&[from, to])?;
    let full = match q.detail.as_deref() {
        None | Some("full") => true,
        Some("headline") => false,
        Some(other) => return Err(bad(format!("detail is full or headline, not {other}"))),
    };
    let limit = if full { FULL_SESSIONS } else { HEADLINE_SESSIONS };
    if client.query_opt("SELECT FROM core.expected_slot WHERE object_id = $1 LIMIT 1", &[&object]).await.map_err(db)?.is_none() {
        return Err(not_found(format!("no profile has an object {object}")));
    }
    // one statement, one snapshot: the sessions of the range (cut at the limit) with the object's
    // published measurements in slot order
    let rows = client
        .query(
            "WITH range AS (SELECT session, row_number() OVER (ORDER BY session) AS n FROM core.session
                            WHERE session BETWEEN $1::text::date AND $2::text::date ORDER BY session LIMIT $3::bigint + 1)
             SELECT s.session::text, s.n, d.revision_id, d.profile_revision, d.engine_build,
                    to_char(d.published_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    to_char(d.snapshot_cutoff AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), d.base_revision,
                    m.measurement_id, m.slot_state, m.science_version, m.method_version, m.body_sha256, st.root_sha256,
                    CASE WHEN $5 THEN m.body_text END,
                    CASE WHEN NOT $5 THEN (SELECT COALESCE(jsonb_object_agg(r.role_id, r.value), '{}'::jsonb)::text FROM vyreon.reader_scalar r
                                           WHERE r.revision_id = m.revision_id AND r.represented_date = m.represented_date
                                             AND r.object_id = m.object_id AND r.measurement_id = m.measurement_id) END
             FROM range s
             LEFT JOIN vyreon.day_revision d ON d.represented_date = s.session AND d.state = 'PUBLISHED'
             LEFT JOIN vyreon.measurement m ON m.revision_id = d.revision_id AND m.represented_date = d.represented_date AND m.object_id = $4
                                           AND ($6::text IS NULL OR m.measurement_id = $6)
             LEFT JOIN core.expected_slot e ON e.engine_build = d.engine_build AND e.profile_revision = d.profile_revision
                                           AND e.measurement_id = m.measurement_id AND e.object_id = m.object_id
             LEFT JOIN vyreon.measurement_stream st ON st.revision_id = m.revision_id AND st.represented_date = m.represented_date
                                                   AND st.measurement_id = m.measurement_id AND st.object_id = m.object_id
             ORDER BY s.session, d.base_revision NULLS FIRST, d.revision_id, e.slot_ordinal",
            &[from, to, &limit, &object, &full, &q.measurement],
        )
        .await
        .map_err(db)?;
    let raw = |text: Option<String>| text.map(RawValue::from_string).transpose().map_err(|e| failed(format!("a stored body is not JSON: {e}")));
    let (mut sessions, mut next, mut identity): (Vec<Session>, Option<String>, String) = (Vec::new(), None, format!("{object} {full} {:?}", q.measurement));
    for row in rows {
        let (date, n): (String, i64) = (row.get(0), row.get(1));
        if n > limit {
            next = Some(date);
            break;
        }
        if sessions.last().is_none_or(|s| s.date != date) {
            sessions.push(Session { date: date.clone(), state: "NOT_PUBLISHED", revisions: Vec::new() });
        }
        let session = sessions.last_mut().expect("just pushed");
        let Some(revision_id) = row.get::<_, Option<i64>>(2) else { continue };
        session.state = "PUBLISHED";
        if session.revisions.last().is_none_or(|r| r.revision_id != revision_id) {
            identity.push_str(&format!(" {date}:{revision_id}"));
            session.revisions.push(Revision {
                revision_id,
                profile_revision: row.get(3),
                engine_build: row.get(4),
                published_at: row.get(5),
                snapshot_cutoff: row.get(6),
                base_revision: row.get(7),
                measurements: Vec::new(),
            });
        }
        let Some(measurement_id) = row.get::<_, Option<String>>(8) else { continue };
        session.revisions.last_mut().expect("just pushed").measurements.push(Measurement {
            measurement_id,
            slot_state: row.get(9),
            science_version: row.get(10),
            method_version: row.get(11),
            body_sha256: row.get(12),
            stream_root_sha256: row.get(13),
            body: raw(row.get(14))?,
            scalars: raw(row.get(15))?,
        });
    }
    // published revisions never change: the ids served, with what was asked, identify the response
    let etag = format!("\"{}\"", &sha256_hex(format!("{identity} {next:?}").as_bytes())[..32]);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response());
    }
    #[derive(Serialize)]
    struct History<'a> {
        contract_version: &'a str,
        object_id: &'a str,
        detail: &'a str,
        sessions: Vec<Session>,
        next: Option<String>,
    }
    let reply = History { contract_version: CONTRACT, object_id: object, detail: if full { "full" } else { "headline" }, sessions, next };
    let text = serde_json::to_string(&reply).map_err(|e| failed(e.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/json".to_owned()), (header::ETAG, etag)], text).into_response())
}

type Slot = Path<(i64, String, String)>;

async fn body(State(app): Shared, headers: HeaderMap, Path((revision, measurement, object)): Slot) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "read").await?;
    let row = client
        .query_opt("SELECT body_text, body_sha256 FROM vyreon.accepted_measurement WHERE revision_id = $1 AND measurement_id = $2 AND object_id = $3", &[&revision, &measurement, &object])
        .await
        .map_err(db)?
        .ok_or_else(|| not_found("no accepted revision has this slot"))?;
    // the engine's bytes: the stored text and its final newline
    let (text, digest): (String, String) = (row.get(0), row.get(1));
    Ok(([(header::CONTENT_TYPE, "application/json".to_owned()), (header::ETAG, format!("\"{digest}\""))], format!("{text}\n")).into_response())
}

async fn stream_part(app: &App, headers: &HeaderMap, slot: (i64, String, String), column: &str) -> Result<Vec<u8>, ApiError> {
    let client = app.db().await?;
    allowed(&client, headers, "read").await?;
    let row = client
        .query_opt(&format!("SELECT {column} FROM vyreon.accepted_stream WHERE revision_id = $1 AND measurement_id = $2 AND object_id = $3"), &[&slot.0, &slot.1, &slot.2])
        .await
        .map_err(db)?
        .ok_or_else(|| not_found("this slot has no event stream in an accepted revision"))?;
    Ok(row.get(0))
}

async fn stream_root(State(app): Shared, headers: HeaderMap, Path(slot): Slot) -> Reply {
    let bytes = stream_part(&app, &headers, slot, "root_bytes").await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response())
}

async fn stream_events(State(app): Shared, headers: HeaderMap, Path(slot): Slot) -> Reply {
    // stored gzip-compressed and sent as stored
    let bytes = stream_part(&app, &headers, slot, "events_gzip").await?;
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson"), (header::CONTENT_ENCODING, "gzip")], bytes).into_response())
}

async fn status(State(app): Shared, headers: HeaderMap) -> Reply {
    let client = app.db().await?;
    allowed(&client, &headers, "admin").await?;
    let client: &Client = &client;
    let store: String = client
        .query_one(
            "SELECT jsonb_build_object(
                 'published_sessions', (SELECT count(DISTINCT represented_date) FROM vyreon.day_revision WHERE state = 'PUBLISHED'),
                 'first_session', (SELECT min(represented_date) FROM vyreon.day_revision WHERE state = 'PUBLISHED'),
                 'latest_session', (SELECT max(represented_date) FROM vyreon.day_revision WHERE state = 'PUBLISHED'),
                 'database_bytes', pg_database_size(current_database()),
                 'builds', (SELECT COALESCE(jsonb_agg(jsonb_build_object('engine_build', engine_build, 'admitted', admitted_at IS NOT NULL AND withdrawn_at IS NULL)
                                                      ORDER BY registered_at), '[]'::jsonb) FROM core.engine_build))::text",
            &[],
        )
        .await
        .map_err(db)?
        .get(0);
    Ok(Json(json!({
        "contract_version": CONTRACT,
        "queue": work::status(client).await.map_err(failed)?,
        "budget": {"committed": budget::committed(client).await.map_err(failed)?, "per_minute": app.config.budget.per_minute},
        "store": serde_json::from_str::<Value>(&store).map_err(|e| failed(e.to_string()))?,
    }))
    .into_response())
}
