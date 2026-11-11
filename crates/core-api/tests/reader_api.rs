//! The reader API over real HTTP, on a database holding the real slice of 2025-12-10 (AAPL, six
//! sensors, base and derived revisions) published through core. Run with `cargo test -- --ignored`.
use core_api::{App, Config};
use core_contract::sha256_hex;
use core_db::budget::Budget;
use core_db::publish::publish_file;
use core_db::{api_key, build, migrate, work};
use reqwest::{Client as Http, StatusCode};
use serde_json::{Value, json};
use std::io::Read;
use test_support::slice::{BASE, DERIVED, bundle, inventories, parts, slice};
use test_support::{CALENDAR, Scratch};

struct Service {
    scratch: Scratch,
    url: String,
    http: Http,
    reader: String,
    admin: String,
    base_revision: i64,
    derived_revision: i64,
}

async fn service(test: &str) -> Service {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(CALENDAR).unwrap()).await.unwrap();
    for inventory in inventories() {
        build::register(&mut client, &inventory).await.unwrap();
    }
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    let mut revisions = Vec::new();
    for revision in [BASE, DERIVED] {
        let (header, scalars, files) = parts(revision);
        let path = bundle(&format!("reader-{test}-{revision}"), &header, &scalars, &files);
        revisions.push(publish_file(&mut client, &path).await.unwrap().revision_id);
        std::fs::remove_file(&path).unwrap();
    }
    let reader = api_key::create(&client, "reporter", &["read".to_owned()]).await.unwrap();
    let admin = api_key::create(&client, "owner", &["read".to_owned(), "admin".to_owned()]).await.unwrap();
    let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(scratch.config.clone(), tokio_postgres::NoTls)).max_size(4).build().unwrap();
    let config = Config { worker_token_sha256: sha256_hex(b"unused"), spool: std::env::temp_dir(), lease_seconds: 300.0, budget: Budget::default(), max_bundle_bytes: 1 << 20 };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = App::new(pool, config);
    tokio::spawn(async move { axum::serve(listener, core_api::router(app)).await.unwrap() });
    Service { scratch, url, http: Http::new(), reader, admin, base_revision: revisions[0], derived_revision: revisions[1] }
}

impl Service {
    async fn get_as(&self, key: &str, path: &str) -> reqwest::Response {
        self.http.get(format!("{}{path}", self.url)).bearer_auth(key).send().await.unwrap()
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let r = self.get_as(&self.reader, path).await;
        (r.status(), r.json().await.unwrap_or(Value::Null))
    }
}

/// The slice's rows of one table for a measurement.
fn of(table: &str, measurement: &str) -> Vec<Value> {
    slice()[table].as_array().unwrap().iter().filter(|r| r["measurement_id"] == measurement).cloned().collect()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_key_is_needed_and_its_scopes_are_kept() {
    let s = service("reader_keys").await;
    let anonymous = s.http.get(format!("{}/contract", s.url)).send().await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(s.get_as("vyk_not_a_key", "/contract").await.status(), StatusCode::UNAUTHORIZED);
    assert!(s.reader.starts_with("vyk_") && s.reader.len() == 68);
    let (status, contract) = s.get("/contract").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((contract["contract_version"].as_str(), contract["latest_session"].as_str()), (Some("VYREON-CORE-API@1"), Some("2025-12-10")));
    assert_eq!(contract["calendar"], json!({"first": "2020-01-02", "last": "2028-12-29"}));
    assert_eq!(contract["profiles"].as_array().unwrap().len(), 2);

    // the queue and budget are for the admin scope
    assert_eq!(s.get("/status").await.0, StatusCode::FORBIDDEN);
    let status: Value = s.get_as(&s.admin, "/status").await.json().await.unwrap();
    assert_eq!((status["store"]["published_sessions"].as_i64(), status["queue"]["paused"].as_bool(), status["budget"]["per_minute"].as_i64()), (Some(1), Some(false), Some(1800)));
    // a revoked key stops working; the key itself is nowhere in the database
    let admin = s.scratch.client().await;
    assert_eq!(admin.query_one("SELECT count(*) FROM core.api_key WHERE key_sha256 = $1", &[&sha256_hex(s.reader.as_bytes())]).await.unwrap().get::<_, i64>(0), 1);
    api_key::revoke(&admin, "reporter").await.unwrap();
    assert_eq!(s.get("/contract").await.0, StatusCode::UNAUTHORIZED);
    s.scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn every_session_is_listed_and_an_unpublished_one_says_so() {
    let s = service("reader_sessions").await;
    let (_, listed) = s.get("/sessions?from=2025-12-06&to=2025-12-11").await;
    let sessions = listed["sessions"].as_array().unwrap();
    let states: Vec<(&str, &str)> = sessions.iter().map(|x| (x["date"].as_str().unwrap(), x["state"].as_str().unwrap())).collect();
    assert_eq!(states, [("2025-12-08", "NOT_PUBLISHED"), ("2025-12-09", "NOT_PUBLISHED"), ("2025-12-10", "PUBLISHED"), ("2025-12-11", "NOT_PUBLISHED")]);
    let revisions = sessions[2]["revisions"].as_array().unwrap();
    assert_eq!(revisions.iter().map(|r| (r["revision_id"].as_i64().unwrap(), r["slot_count"].as_i64().unwrap(), r["base_revision"].as_i64())).collect::<Vec<_>>(), [(s.base_revision, 4, None), (s.derived_revision, 2, Some(s.base_revision))]);
    assert_eq!(revisions[0]["engine_build"], slice()["day_revision"][0]["engine_build"]);
    assert!(revisions[0].get("slots").is_none() && sessions[0]["revisions"].as_array().unwrap().is_empty());

    // one session, with its slots in the profile's order
    for path in ["/sessions/2025-12-10", "/sessions/latest"] {
        let (_, one) = s.get(path).await;
        let slots = one["session"]["revisions"][0]["slots"].as_array().unwrap();
        let all = slice();
        let mut expected: Vec<&Value> = all["expected_slot"].as_array().unwrap().iter().filter(|e| e["layer"] != "DEMAND_LED").collect();
        expected.sort_by_key(|e| e["slot_ordinal"].as_i64());
        assert_eq!(slots.iter().map(|x| &x["measurement_id"]).collect::<Vec<_>>(), expected.iter().map(|e| &e["measurement_id"]).collect::<Vec<_>>(), "{path}");
        let q01 = slots.iter().find(|x| x["measurement_id"] == "VS-Q01").unwrap();
        let fixture = &of("measurement", "VS-Q01")[0];
        assert_eq!((&q01["body_sha256"], q01["body_bytes"].as_u64().unwrap() as usize), (&fixture["body_sha256"], fixture["body_text"].as_str().unwrap().len() + 1));
        assert_eq!(slots.iter().filter(|x| !x["stream_root_sha256"].is_null()).count(), 2);
    }
    for (path, status) in [("/sessions/2025-12-13", StatusCode::NOT_FOUND), ("/sessions/yesterday", StatusCode::BAD_REQUEST), ("/sessions?from=2025-12-45&to=2025-12-11", StatusCode::BAD_REQUEST), ("/sessions?from=2020-01-01&to=2028-12-31", StatusCode::BAD_REQUEST), ("/sessions", StatusCode::BAD_REQUEST)] {
        assert_eq!(s.get(path).await.0, status, "{path}");
    }
    assert_eq!(s.get("/sessions?from=2025-12-13&to=2025-12-14").await.1["sessions"], json!([]), "a weekend holds no session");
    s.scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_assets_history_carries_what_the_engine_published() {
    let s = service("reader_history").await;
    // headline: the reader scalars of each measurement, as the droplet's store held them
    let (status, headline) = s.get("/assets/AAPL/history?from=2025-12-09&to=2025-12-11&detail=headline").await;
    assert_eq!(status, StatusCode::OK, "{headline}");
    let sessions = headline["sessions"].as_array().unwrap();
    assert_eq!(sessions.iter().map(|x| x["state"].as_str().unwrap()).collect::<Vec<_>>(), ["NOT_PUBLISHED", "PUBLISHED", "NOT_PUBLISHED"]);
    assert_eq!((headline["detail"].as_str(), headline["next"].is_null()), (Some("headline"), true));
    let measurements: Vec<&Value> = sessions[1]["revisions"].as_array().unwrap().iter().flat_map(|r| r["measurements"].as_array().unwrap()).collect();
    assert_eq!(measurements.len(), 6);
    for m in &measurements {
        let id = m["measurement_id"].as_str().unwrap();
        let expected: serde_json::Map<String, Value> = of("reader_scalar", id).iter().map(|r| (r["role_id"].as_str().unwrap().to_owned(), r["value"].clone())).collect();
        assert_eq!(m["scalars"], Value::Object(expected), "{id}");
        assert!(m.get("body").is_none());
        assert_eq!(m["science_version"], of("measurement", id)[0]["science_version"]);
    }
    assert_eq!(measurements.iter().filter(|m| !m["scalars"].as_object().unwrap().is_empty()).count(), 3, "VS-IVS01, DLM-IV30 and DLM-MAX-PAIN have reader scalars");

    // full (the default): every body, equal to the engine's
    let (_, full) = s.get("/assets/AAPL/history?from=2025-12-10&to=2025-12-10").await;
    let measurements: Vec<&Value> = full["sessions"][0]["revisions"].as_array().unwrap().iter().flat_map(|r| r["measurements"].as_array().unwrap()).collect();
    assert_eq!(measurements.len(), 6);
    for m in &measurements {
        let fixture = &of("measurement", m["measurement_id"].as_str().unwrap())[0];
        assert_eq!(m["body"], serde_json::from_str::<Value>(fixture["body_text"].as_str().unwrap()).unwrap());
        assert_eq!((&m["body_sha256"], &m["slot_state"]), (&fixture["body_sha256"], &fixture["slot_state"]));
    }
    // one measurement only; the latest session; an object no profile has
    let (_, one) = s.get("/assets/AAPL/history?from=2025-12-10&to=2025-12-10&detail=headline&measurement=DLM-IV30").await;
    let revisions = one["sessions"][0]["revisions"].as_array().unwrap();
    assert_eq!((revisions[0]["measurements"].as_array().unwrap().len(), revisions[1]["measurements"][0]["scalars"]["iv30"].as_f64()), (0, of("reader_scalar", "DLM-IV30")[0]["value"].as_f64()));
    assert_eq!(s.get("/assets/AAPL/latest?detail=headline").await.1["sessions"][0]["date"], "2025-12-10");
    for (path, status) in [("/assets/NOPE/history?from=2025-12-10&to=2025-12-10", StatusCode::NOT_FOUND), ("/assets/AAPL/history?from=2025-12-10", StatusCode::BAD_REQUEST), ("/assets/AAPL/history?from=2025-12-10&to=2025-12-10&detail=everything", StatusCode::BAD_REQUEST)] {
        assert_eq!(s.get(path).await.0, status, "{path}");
    }

    // a long range is cut and says where to continue: December 2025 has 22 sessions, a full page holds 20
    let (_, page) = s.get("/assets/AAPL/history?from=2025-12-01&to=2025-12-31").await;
    assert_eq!((page["sessions"].as_array().unwrap().len(), page["next"].as_str()), (20, Some("2025-12-30")));
    let (_, rest) = s.get("/assets/AAPL/history?from=2025-12-30&to=2025-12-31").await;
    assert_eq!((rest["sessions"].as_array().unwrap().len(), rest["next"].is_null()), (2, true));

    // an unchanged answer is not sent again
    let first = s.get_as(&s.reader, "/assets/AAPL/history?from=2025-12-09&to=2025-12-11&detail=headline").await;
    let etag = first.headers()["etag"].to_str().unwrap().to_owned();
    let again = s.http.get(format!("{}/assets/AAPL/history?from=2025-12-09&to=2025-12-11&detail=headline", s.url)).bearer_auth(&s.reader).header("if-none-match", &etag).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    s.scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_body_and_its_stream_are_served_byte_for_byte() {
    let s = service("reader_bodies").await;
    let fixture = &of("measurement", "M0-ANALYTICS")[0];
    let slot = format!("/revisions/{}/slots/M0-ANALYTICS/AAPL", s.base_revision);
    let body = s.get_as(&s.reader, &slot).await;
    assert_eq!(body.headers()["etag"].to_str().unwrap(), format!("\"{}\"", fixture["body_sha256"].as_str().unwrap()));
    let bytes = body.bytes().await.unwrap();
    assert_eq!(sha256_hex(&bytes), fixture["body_sha256"].as_str().unwrap());
    assert_eq!(bytes, format!("{}\n", fixture["body_text"].as_str().unwrap()).as_bytes());

    // the stream: the root hashes to the body's commitment, the events to the root's digest
    let committed = serde_json::from_slice::<Value>(&bytes).unwrap()["product_stream_root_sha256"].as_str().unwrap().to_owned();
    let root = s.get_as(&s.reader, &format!("{slot}/stream/root")).await.bytes().await.unwrap();
    assert_eq!(sha256_hex(&root), committed);
    let root: Value = serde_json::from_slice(&root).unwrap();
    let events = s.get_as(&s.reader, &format!("{slot}/stream/events")).await;
    assert_eq!(events.headers()["content-encoding"], "gzip");
    let mut plain = Vec::new();
    flate2::read::GzDecoder::new(&events.bytes().await.unwrap()[..]).read_to_end(&mut plain).unwrap();
    assert_eq!((sha256_hex(&plain), plain.len() as i64), (root["events_sha256"].as_str().unwrap().to_owned(), root["byte_length"].as_i64().unwrap()));

    for path in [format!("/revisions/{}/slots/VS-Q01/AAPL/stream/root", s.base_revision), format!("/revisions/{}/slots/VS-Q01/MSFT", s.base_revision), "/revisions/999/slots/VS-Q01/AAPL".to_owned()] {
        assert_eq!(s.get(&path).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    // a staged or rejected revision is never served; a superseded one still is, by its id
    let admin = s.scratch.client().await;
    admin.execute("UPDATE vyreon.day_revision SET state = 'SUPERSEDED' WHERE revision_id = $1", &[&s.derived_revision]).await.unwrap();
    assert_eq!(s.get_as(&s.reader, &format!("/revisions/{}/slots/DLM-IV30/AAPL", s.derived_revision)).await.status(), StatusCode::OK);
    assert_eq!(s.get("/assets/AAPL/history?from=2025-12-10&to=2025-12-10&detail=headline").await.1["sessions"][0]["revisions"].as_array().unwrap().len(), 1);
    admin.execute("UPDATE vyreon.day_revision SET state = 'REJECTED' WHERE revision_id = $1", &[&s.derived_revision]).await.unwrap();
    assert_eq!(s.get_as(&s.reader, &format!("/revisions/{}/slots/DLM-IV30/AAPL", s.derived_revision)).await.status(), StatusCode::NOT_FOUND);
    s.scratch.drop().await;
}
