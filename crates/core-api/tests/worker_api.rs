//! A worker's whole life against the service over real HTTP: the tape job, the day block and the
//! ordered pass of 2025-12-10, with the real slice of that day, a real tape cache, the engine's
//! calendar and the real universe. Run with `cargo test -- --ignored`.
use core_api::{App, Config};
use core_contract::sha256_hex;
use core_db::budget::Budget;
use core_db::work::{self, Kind};
use core_db::{build, migrate, universe};
use reqwest::{Client as Http, StatusCode};
use serde_json::{Value, json};
use std::io::Read;
use test_support::slice::{BASE, DERIVED, bundle, inventories, parts};
use test_support::{CALENDAR, Scratch, TAPE_AAPL, fixtures};

const PROFILE: &str = "FREE_REPORT_64";
const WORKERS_TOKEN: &str = "the workers' shared token of this test";

struct Service {
    scratch: Scratch,
    url: String,
    http: Http,
    base_build: String,
    derived_build: String,
}

/// A database with the calendar, the universe and the slice's builds admitted, and the service
/// listening on a free local port.
async fn service(test: &str) -> Service {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(CALENDAR).unwrap()).await.unwrap();
    let doc = universe::read_responses(&fixtures().join("engine-v1-PROFILE.json"), &fixtures().join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for inventory in inventories() {
        let b = build::register(&mut client, &inventory).await.unwrap().engine_build;
        build::admit(&client, &b, &json!({"checklist": "worker API test"})).await.unwrap();
        builds.push(b);
    }
    let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(scratch.config.clone(), tokio_postgres::NoTls)).max_size(4).build().unwrap();
    let config = Config {
        worker_token_sha256: sha256_hex(WORKERS_TOKEN.as_bytes()),
        spool: std::env::temp_dir(),
        lease_seconds: 300.0,
        budget: Budget::default(),
        max_bundle_bytes: 1 << 20,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = App::new(pool, config);
    tokio::spawn(async move { axum::serve(listener, core_api::router(app)).await.unwrap() });
    Service { scratch, url, http: Http::new(), derived_build: builds.pop().unwrap(), base_build: builds.pop().unwrap() }
}

impl Service {
    async fn claim(&self, kind: &str, build: &str, worker: &str) -> reqwest::Response {
        self.http
            .post(format!("{}/worker/v1/claims", self.url))
            .bearer_auth(WORKERS_TOKEN)
            .json(&json!({"kind": kind, "engine_build": build, "worker": worker}))
            .send()
            .await
            .unwrap()
    }

    /// POSTs JSON under a lease token; returns the status and the reply.
    async fn post(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let r = self.http.post(format!("{}/worker/v1{path}", self.url)).bearer_auth(token).json(&body).send().await.unwrap();
        (r.status(), r.json().await.unwrap_or(Value::Null))
    }

    async fn publish(&self, token: &str, path: &std::path::Path) -> (StatusCode, Value) {
        let bytes = std::fs::read(path).unwrap();
        std::fs::remove_file(path).unwrap();
        let r = self.http.post(format!("{}/worker/v1/days", self.url)).bearer_auth(token).body(bytes).send().await.unwrap();
        (r.status(), r.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_tape_worker_a_day_worker_and_the_ordered_pass_do_a_day_over_http() {
    let s = service("api").await;
    let mut admin = s.scratch.client().await;
    for (kind, build) in [(Kind::Tape, &s.base_build), (Kind::DayBlock, &s.base_build), (Kind::OrderedPass, &s.derived_build)] {
        work::schedule(&mut admin, kind, PROFILE, build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    }
    assert_eq!(s.http.get(format!("{}/ready", s.url)).send().await.unwrap().text().await.unwrap(), "ready");

    // no token, the wrong token, a lease token where the workers' token belongs
    let anonymous = s.http.post(format!("{}/worker/v1/claims", s.url)).json(&json!({"kind": "tape", "engine_build": s.base_build, "worker": "x"})).send().await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(s.post("/lease/heartbeat", "not-a-lease", json!({})).await.0, StatusCode::GONE);
    // the day block waits for the tape
    assert_eq!(s.claim("day-block", &s.base_build, "day-1").await.status(), StatusCode::NO_CONTENT);

    // the tape worker: claim, draw budget, store each member's cache or absence, done, finish
    let tape: Value = s.claim("tape", &s.base_build, "tape-1").await.json().await.unwrap();
    let token = tape["lease_token"].as_str().unwrap();
    assert_eq!(tape["sessions"], json!(["2025-12-10"]));
    assert_eq!(tape["sources"]["replay_tapes"], "2023-05-24");
    let (_, grant) = s.post("/budget", token, json!({"requests": 200})).await;
    assert_eq!((grant["requests"].as_i64(), grant["valid_seconds"].as_f64()), (Some(200), Some(30.0)));
    let (status, released) = s.post(&format!("/budget/{}/release", grant["budget_lease_id"]), token, json!({"used": {"consolidated": 137}})).await;
    assert_eq!((status, released["counted"].as_i64()), (StatusCode::OK, Some(137)));
    let caches = format!("{}/worker/v1/tape-caches/2025-12-10", s.url);
    let put = s.http.put(format!("{caches}/AAPL?method_version=engine-v1")).bearer_auth(token).body(TAPE_AAPL).send().await.unwrap();
    assert_eq!(put.json::<Value>().await.unwrap()["new"], true);
    let (status, early) = s.post("/tape-caches/2025-12-10/done", token, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(early["error"].as_str().unwrap().starts_with("63 of 64 members have no tape cache"), "{early}");
    for member in tape["days"][0]["members"].as_array().unwrap().iter().map(|m| m["asset_key"].as_str().unwrap()).filter(|k| *k != "AAPL") {
        let absent = s
            .http
            .put(format!("{caches}/{member}"))
            .query(&[("method_version", "engine-v1"), ("absence", "NOT_CAPTURED: only AAPL's tape was fetched for this test")])
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(absent.status(), StatusCode::OK, "{member}");
    }
    assert_eq!(s.post("/tape-caches/2025-12-10/done", token, json!({})).await.0, StatusCode::OK);
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::OK);
    assert_eq!(s.post("/lease/heartbeat", token, json!({})).await.0, StatusCode::GONE, "a finished lease is over");

    // the day worker: its claim tells it who is listed under which ticker; it reads the session's
    // tape caches, publishes the day and finishes
    let day: Value = s.claim("day-block", &s.base_build, "day-1").await.json().await.unwrap();
    let token = day["lease_token"].as_str().unwrap();
    let members = day["days"][0]["members"].as_array().unwrap();
    assert_eq!((day["days"][0]["session"].as_str(), members.len()), (Some("2025-12-10"), 64));
    assert_eq!(members.iter().find(|m| m["asset_key"] == "META").unwrap()["ticker"], "META");
    let tar_bytes = s.http.get(format!("{caches}?method_version=engine-v1")).bearer_auth(token).send().await.unwrap().bytes().await.unwrap();
    let mut archive = tar::Archive::new(&tar_bytes[..]);
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.path().unwrap().to_string_lossy().into_owned(), bytes));
    }
    assert_eq!(entries.len(), 2, "AAPL's cache and the absences");
    assert_eq!((entries[0].0.as_str(), entries[0].1.as_slice()), ("AAPL", TAPE_AAPL));
    assert_eq!(serde_json::from_slice::<Value>(&entries[1].1).unwrap().as_object().unwrap().len(), 63);
    assert_eq!(s.post("/lease/heartbeat", token, json!({})).await.1["paused"], false);
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::UNPROCESSABLE_ENTITY, "its day is not published yet");
    let (header, scalars, files) = parts(BASE);
    let (status, published) = s.publish(token, &bundle("api-base", &header, &scalars, &files)).await;
    assert_eq!((status, published["slots"].as_i64(), published["streams"].as_i64()), (StatusCode::OK, Some(4), Some(2)), "{published}");
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::OK);

    // the ordered pass: the day's history sensors, on the base it read
    let pass: Value = s.claim("ordered-pass", &s.derived_build, "pass-1").await.json().await.unwrap();
    let token = pass["lease_token"].as_str().unwrap();
    let (mut dh, ds, df) = parts(DERIVED);
    dh.base_revision = published["revision_id"].as_i64();
    let (status, derived) = s.publish(token, &bundle("api-derived", &dh, &ds, &df)).await;
    assert_eq!((status, derived["slots"].as_i64()), (StatusCode::OK, Some(2)), "{derived}");
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::OK);

    let status = work::status(&admin).await.unwrap();
    assert_eq!(status["jobs"].as_array().unwrap().iter().map(|j| (j["kind"].as_str().unwrap(), j["state"].as_str().unwrap())).collect::<Vec<_>>(), [("DAY_BLOCK", "DONE"), ("ORDERED_PASS", "DONE"), ("TAPE", "DONE")]);
    assert_eq!(admin.query_one("SELECT count(*) FROM vyreon.published_measurement", &[]).await.unwrap().get::<_, i64>(0), 6);
    assert_eq!(admin.query_one("SELECT sum(requests)::bigint FROM core.request_usage WHERE worker = 'tape-1'", &[]).await.unwrap().get::<_, i64>(0), 137);
    s.scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refused_bundle_pauses_the_queue_and_a_failing_worker_sends_its_log() {
    let s = service("api_refusal").await;
    let mut admin = s.scratch.client().await;
    work::schedule(&mut admin, Kind::DayBlock, PROFILE, &s.base_build, "2025-12-10", "2025-12-11", 1).await.unwrap();
    let day: Value = s.claim("day-block", &s.base_build, "day-1").await.json().await.unwrap();
    let token = day["lease_token"].as_str().unwrap();

    // a body changed on the way: refused, nothing stored, and no more work is handed out
    let (header, scalars, files) = parts(BASE);
    let path = bundle("api-good", &header, &scalars, &files);
    let mut bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let cut = bytes.len() / 2;
    bytes.truncate(cut);
    let r = s.http.post(format!("{}/worker/v1/days", s.url)).bearer_auth(token).body(bytes).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.json::<Value>().await.unwrap()["error"].as_str().unwrap().contains("unreadable bundle"));
    let status = work::status(&admin).await.unwrap();
    assert_eq!(status["paused"], true);
    assert!(status["pause_reason"].as_str().unwrap().contains("(day-1): bundle refused: unreadable bundle"), "{status}");
    assert_eq!(s.claim("day-block", &s.base_build, "day-2").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(s.post("/lease/heartbeat", token, json!({})).await.1["paused"], true, "the worker learns of the pause");
    assert_eq!(admin.query_one("SELECT count(*) FROM vyreon.day_revision", &[]).await.unwrap().get::<_, i64>(0), 0);

    // an upload over the size limit is turned away
    let big = vec![0u8; (1 << 20) + 1];
    let r = s.http.post(format!("{}/worker/v1/days", s.url)).bearer_auth(token).body(big).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // a failing worker says where it failed and sends its log
    let (status, failed) = s.post("/lease/fail", token, json!({"stage": "surfaces", "message": "no chain for XLB", "log": "the day's log"})).await;
    assert_eq!((status, &failed["requeued"]), (StatusCode::OK, &json!(true)));
    let kept = admin.query_one("SELECT failed_stage, failure, failure_log, worker FROM core.lease WHERE outcome = 'FAILED'", &[]).await.unwrap();
    assert_eq!((kept.get::<_, &str>(0), kept.get::<_, &str>(1), kept.get::<_, &str>(2), kept.get::<_, &str>(3)), ("surfaces", "no chain for XLB", "the day's log", "day-1"));
    s.scratch.drop().await;
}
