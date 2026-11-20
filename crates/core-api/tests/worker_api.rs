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
        archive: None,
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
    // C-058: the tape reader's verdict on the cache it stored, kept once beside it (here the
    // reference suite's own answer on this cache, which holds no raw tape: NOT_RUN with why)
    let verdict = json!({"schema": "VYREON-TAPE-VERDICT@1", "asset": "AAPL", "session": "2025-12-10", "outcome": "NOT_RUN",
                         "why": "the reference reader failed: RawTapeAbsent: AAPL 2025-12-10: UNVERIFIABLE: raw tape absent",
                         "method_version": "engine-v1", "cache_sha256": sha256_hex(TAPE_AAPL), "engine_build": s.base_build, "versions": null});
    let put_verdict = |asset: &str, v: &Value| s.http.put(format!("{caches}/{asset}/verdict?method_version=engine-v1")).bearer_auth(token).json(v).send();
    for (status, new) in [(StatusCode::OK, true), (StatusCode::OK, false)] {
        let r = put_verdict("AAPL", &verdict).await.unwrap();
        assert_eq!(r.status(), status);
        assert_eq!(r.json::<Value>().await.unwrap()["new"], new);
    }
    let r = put_verdict("MSFT", &verdict).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY, "another asset's verdict");
    let r = s.http.put(format!("{caches}/AAPL/verdict?method_version=engine-v1")).bearer_auth("not-a-lease").json(&verdict).send().await.unwrap();
    assert_ne!(r.status(), StatusCode::OK, "only the session's tape job sends a verdict");
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
    assert_eq!(day["tape_history_from"], "2025-12-10", "the first scheduled tape session");
    assert_eq!(members.iter().find(|m| m["asset_key"] == "META").unwrap()["ticker"], "META");
    // and, since history windows cross ticker changes, every ticker each member has traded under
    let tickers = day["tickers"].as_object().unwrap();
    assert_eq!(tickers.len(), 64);
    assert_eq!(
        day["tickers"]["META"],
        json!([{"ticker": "FB", "from": "2012-05-18", "to": "2022-06-08"}, {"ticker": "META", "from": "2022-06-09", "to": null}])
    );
    assert_eq!(day["tickers"]["ETHA"], json!([{"ticker": "ETHA", "from": "2024-07-23", "to": null}]));
    // C-055: and the OCC adjustment ledger's entries of its members, as the engine reads them
    // (`VYREON-ADJUSTMENTS@1`'s `assets`): every ledger asset is one of the 64
    assert_eq!(day["adjustments"], core_api::adjustments::ledger().unwrap()["assets"]);
    let ewj = &day["adjustments"]["EWJ"][0];
    assert_eq!((ewj["ex_date"].as_str(), ewj["kind"].as_str(), ewj["strike_offset"].as_f64()), (Some("2025-12-16"), Some("other"), Some(0.16)));
    // engine D-120 as amended 2026-11-20: with OCC's special behind the offset (memo #57919)
    assert_eq!(ewj["special_dividend"].as_f64(), Some(0.157024));
    assert_eq!(day["adjustments"]["ETHA"][0]["root"], "ETHA1");
    assert!(day["adjustments"].get("AAPL").is_none(), "no OCC adjustment of AAPL since 2022-05-27");
    // C-057: and each member's first option session where core holds the provider's evidence
    // (ETHA's first chain 2025-04-11, IBIT's 2024-11-19); the other members have none
    assert_eq!(day["first_option_sessions"], json!({"ETHA": "2025-04-11", "IBIT": "2024-11-19"}));
    // the bundle states the day's listed members: the asset keys with a ticker that day
    let listed: Vec<&str> = members.iter().filter(|m| m["ticker"].is_string()).map(|m| m["asset_key"].as_str().unwrap()).collect();
    assert_eq!(listed, test_support::slice::members_except(&[]));
    let tar_bytes = s.http.get(format!("{caches}?method_version=engine-v1")).bearer_auth(token).send().await.unwrap().bytes().await.unwrap();
    let mut archive = tar::Archive::new(&tar_bytes[..]);
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.path().unwrap().to_string_lossy().into_owned(), bytes));
    }
    assert_eq!(entries.len(), 3, "AAPL's cache, the absences and the verdicts");
    assert_eq!((entries[0].0.as_str(), entries[0].1.as_slice()), ("AAPL", TAPE_AAPL));
    assert_eq!(serde_json::from_slice::<Value>(&entries[1].1).unwrap().as_object().unwrap().len(), 63);
    assert_eq!(entries[2].0, "verdicts.json");
    assert_eq!(serde_json::from_slice::<Value>(&entries[2].1).unwrap(), json!({"AAPL": verdict}));
    assert_eq!(s.post("/lease/heartbeat", token, json!({})).await.1["paused"], false);
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::UNPROCESSABLE_ENTITY, "its day is not published yet");
    let (header, scalars, files) = parts(BASE);
    let (status, published) = s.publish(token, &bundle("api-base", &header, &scalars, &files)).await;
    assert_eq!((status, published["slots"].as_i64(), published["streams"].as_i64()), (StatusCode::OK, Some(4), Some(2)), "{published}");
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::UNPROCESSABLE_ENTITY, "the worker has not said the day is done");
    // C-043: the published day's input captures (here T−5's real macro capture), written once;
    // one not kept is recorded with its reason
    let captures = format!("{}/worker/v1/input-captures", s.url);
    let put_capture = |body: Vec<u8>, captured_at: &str| {
        s.http
            .put(format!("{captures}/2025-12-03/macro/{}", test_support::MACRO_CAPTURE_IDENTITY))
            .query(&[("captured_at", captured_at)])
            .bearer_auth(token)
            .body(body)
            .send()
    };
    let kept = put_capture(test_support::MACRO_CAPTURE.to_vec(), "2026-11-16T04:49:17.420677Z").await.unwrap();
    assert_eq!((kept.status(), kept.json::<Value>().await.unwrap()["new"].clone()), (StatusCode::OK, json!(true)));
    let again = put_capture(test_support::MACRO_CAPTURE.to_vec(), "2026-11-16T04:49:17.420677Z").await.unwrap();
    assert_eq!(again.json::<Value>().await.unwrap()["new"], false);
    let damaged = put_capture(test_support::MACRO_CAPTURE[..1000].to_vec(), "2026-11-16T04:49:17Z").await.unwrap();
    assert_eq!(damaged.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let too_large = put_capture(vec![0; core_db::capture::MAX_ARCHIVE_BYTES + 1], "2026-11-16T04:49:17Z").await.unwrap();
    assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(s.post("/input-captures/2025-12-10/company/absence", token, json!({"reason": "not packed: no manifest.json"})).await, (StatusCode::OK, json!({"new": true})));
    let listed: Value = s.http.get(format!("{captures}/2025-12-03")).bearer_auth(token).send().await.unwrap().json().await.unwrap();
    assert_eq!(listed["captures"].as_array().unwrap().iter().map(|c| c["identity"].as_str().unwrap()).collect::<Vec<_>>(), [test_support::MACRO_CAPTURE_IDENTITY]);
    let fetched = s.http.get(format!("{captures}/2025-12-03/macro/{}", test_support::MACRO_CAPTURE_IDENTITY)).bearer_auth(token).send().await.unwrap();
    assert_eq!(fetched.headers()["content-type"], "application/gzip");
    assert_eq!(fetched.bytes().await.unwrap().as_ref(), test_support::MACRO_CAPTURE);
    // and for the owner, with an admin key (a read key is not enough: these are the provider's
    // raw responses)
    let owner = core_db::api_key::create(&admin, "owner", &["read".to_owned(), "admin".to_owned()]).await.unwrap();
    let reporter = core_db::api_key::create(&admin, "reporter", &["read".to_owned()]).await.unwrap();
    let as_key = |key: &str, path: &str| s.http.get(format!("{}/v1/input-captures/{path}", s.url)).bearer_auth(key.to_owned()).send();
    assert_eq!(as_key(&reporter, "2025-12-03").await.unwrap().status(), StatusCode::FORBIDDEN);
    let listed: Value = as_key(&owner, "2025-12-10").await.unwrap().json().await.unwrap();
    assert_eq!(listed["captures"][0]["absence"], "not packed: no manifest.json");
    let fetched = as_key(&owner, &format!("2025-12-03/macro/{}", test_support::MACRO_CAPTURE_IDENTITY)).await.unwrap().bytes().await.unwrap();
    assert_eq!(fetched.as_ref(), test_support::MACRO_CAPTURE);
    assert_eq!(as_key(&owner, &format!("2025-12-04/macro/{}", test_support::MACRO_CAPTURE_IDENTITY)).await.unwrap().status(), StatusCode::NOT_FOUND);
    // C-054: before its day's check is kept, the day must be done; then the worker's document
    // (the real B0-census report as this day's) is kept once; a document carrying a credential,
    // or one sent for another day, is refused
    let refcheck = |session: &str, body: Vec<u8>| s.http.put(format!("{}/worker/v1/refcheck/{session}", s.url)).bearer_auth(token).body(body).send();
    let report = {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(test_support::REFCHECK_REPORT_2025_12_16), &mut text).unwrap();
        text
    };
    let check_document = |report: &str| {
        let r: Value = serde_json::from_str(report).unwrap();
        let summary = r["summary"].as_object().unwrap();
        let sum = |k: &str| summary.values().map(|m| m[k].as_i64().unwrap_or(0)).sum::<i64>();
        let account = json!({
            "schema": "VYREON-REFCHECK@1", "session": "2025-12-10", "outcome": "COMPLETED", "why": null, "wall_seconds": 2460.0,
            "suite_seconds": 2400.0, "cpu_seconds": 16800.0, "cpu_complete": true, "jobs": 8, "deadline_seconds": 7200,
            "counts": {"checked": sum("checked"), "passed": sum("passed"), "failed": sum("failed"), "skipped": sum("skipped"),
                       "reference_errors": sum("reference_errors"), "failure_classes": r["failure_classes"].as_array().unwrap().len(),
                       "measurements_failing": summary.values().filter(|m| m["failed"].as_i64().unwrap_or(0) > 0).count()},
            "summary": r["summary"],
        });
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, format!("{account}\n{report}").as_bytes()).unwrap();
        gz.finish().unwrap()
    };
    let early = refcheck("2025-12-10", check_document(&report)).await.unwrap();
    assert_eq!(early.status(), StatusCode::UNPROCESSABLE_ENTITY, "not done yet");
    assert_eq!(s.post("/days/2025-12-10/done", token, json!({})).await.0, StatusCode::OK);
    let kept: Value = refcheck("2025-12-10", check_document(&report)).await.unwrap().json().await.unwrap();
    assert_eq!(kept["new"], true, "{kept}");
    let again: Value = refcheck("2025-12-10", check_document(&report)).await.unwrap().json().await.unwrap();
    assert_eq!((again["new"].clone(), again["report_id"].clone()), (json!(false), kept["report_id"].clone()));
    let leaked = report.replacen("\"dump\": \"", "\"dump\": \"https://api-v2.intrinio.com/x?api_key=OmRlYWRiZWVm ", 1);
    assert_eq!(refcheck("2025-12-10", check_document(&leaked)).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refcheck("2025-12-09", check_document(&report)).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY, "the account is of another day");
    assert_eq!(refcheck("yesterday", check_document(&report)).await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
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
    assert!(day["tape_history_from"].is_null(), "no tape job is scheduled: the engine keeps its own setting");
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
    // the pause and its notice were written together
    let notice: String = admin.query_one("SELECT message FROM core.notice WHERE NOT post ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(notice.starts_with("Queue paused: job ") && notice.contains("bundle refused: unreadable bundle"), "{notice}");
    assert_eq!(s.claim("day-block", &s.base_build, "day-2").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(s.post("/lease/heartbeat", token, json!({})).await.1["paused"], true, "the worker learns of the pause");
    assert_eq!(admin.query_one("SELECT count(*) FROM vyreon.day_revision", &[]).await.unwrap().get::<_, i64>(0), 0);

    // an upload over the size limit is turned away
    let big = vec![0u8; (1 << 20) + 1];
    let r = s.http.post(format!("{}/worker/v1/days", s.url)).bearer_auth(token).body(big).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // C-049: before its fail, the worker leaves its evidence (a real worker's log of a refused
    // day as the excerpt); a document with a credential left in it is refused and nothing stored
    let evidence = |line: &str| {
        let account = json!({"schema": "VYREON-JOB-EVIDENCE@1", "stage": "surfaces", "message": "no chain for XLB", "session": "2025-12-10", "refusals": null});
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, format!("{account}\n{line}{}", test_support::WORKER_LOG_REFUSED_DAY).as_bytes()).unwrap();
        gz.finish().unwrap()
    };
    let put = |body: Vec<u8>| s.http.put(format!("{}/worker/v1/evidence", s.url)).bearer_auth(token).body(body).send();
    let r = put(evidence("GET https://api-v2.intrinio.com/x?api_key=OmRlYWRiZWVm failed\n")).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.json::<Value>().await.unwrap()["error"].as_str().unwrap().contains("unredacted api_key="));
    let r = put(evidence("")).await.unwrap();
    assert_eq!((r.status(), r.json::<Value>().await.unwrap()), (StatusCode::OK, json!({"new": true})));
    let r = s.http.put(format!("{}/worker/v1/evidence", s.url)).body(evidence("")).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // a failing worker says where it failed and sends its log
    let (status, failed) = s.post("/lease/fail", token, json!({"stage": "surfaces", "message": "no chain for XLB", "log": "the day's log"})).await;
    assert_eq!((status, &failed["requeued"]), (StatusCode::OK, &json!(true)));
    let kept = admin.query_one("SELECT failed_stage, failure, failure_log, worker FROM core.lease WHERE outcome = 'FAILED'", &[]).await.unwrap();
    assert_eq!((kept.get::<_, &str>(0), kept.get::<_, &str>(1), kept.get::<_, &str>(2), kept.get::<_, &str>(3)), ("surfaces", "no chain for XLB", "the day's log", "day-1"));
    let (record, excerpt) = core_db::evidence::show(&admin, day["job_id"].as_i64().unwrap(), None).await.unwrap().unwrap();
    assert_eq!((record["attempt"].as_i64(), record["worker"].as_str(), excerpt.as_str()), (Some(1), Some("day-1"), test_support::WORKER_LOG_REFUSED_DAY));
    s.scratch.drop().await;
}

/// C-059: a verify job over HTTP. Its claim says it verifies and leaves the tape history where the
/// tape jobs put it; it may store no cache and no verdict of its own; its check of AAPL (recomputed
/// equal to the real cache core holds, with a PASS standing for the tape reader's on a raw tape)
/// keeps the verdict beside the stored cache, which the day workers then read with it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_verify_job_checks_the_stored_caches_over_http_and_stores_none() {
    let s = service("api_verify").await;
    let mut admin = s.scratch.client().await;
    const S: &str = "2025-12-10";
    assert!(core_db::tape::put(&admin, "AAPL", S, "engine-v1", &core_db::tape::Cache::Body(TAPE_AAPL.to_vec())).await.unwrap());
    work::schedule(&mut admin, Kind::Tape, PROFILE, &s.base_build, "2025-12-11", "2025-12-11", 1).await.unwrap();
    let verify = work::schedule_verify(&mut admin, PROFILE, &s.base_build, &[S]).await.unwrap();

    let claim: Value = s.claim("tape", &s.base_build, "verify-1").await.json().await.unwrap();
    let token = claim["lease_token"].as_str().unwrap();
    assert_eq!((claim["job_id"].as_i64(), claim["scope"].clone()), (Some(verify[0]), json!({"verify": true})));
    assert_eq!(claim["tape_history_from"], "2025-12-11", "a verify job does not move the tape history");
    let caches = format!("{}/worker/v1/tape-caches/{S}", s.url);
    let r = s.http.put(format!("{caches}/AAPL?method_version=engine-v1")).bearer_auth(token).body(TAPE_AAPL).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY, "a verify job stores no cache");
    let whole = sha256_hex(TAPE_AAPL);
    let verdict = json!({"schema": "VYREON-TAPE-VERDICT@1", "asset": "AAPL", "session": S, "outcome": "PASS", "why": null, "differences": [],
                         "reader_bins_sha256": "a".repeat(64), "method_version": "engine-v1", "cache_sha256": whole, "engine_build": s.base_build, "versions": null});
    let r = s.http.put(format!("{caches}/AAPL/verdict?method_version=engine-v1")).bearer_auth(token).json(&verdict).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY, "nor a verdict of its own");
    let check = |asset: &str, recomputed: Option<&str>, verdict: Option<&Value>| {
        json!({"schema": "VYREON-TAPE-CHECK@1", "asset": asset, "session": S, "method_version": "engine-v1", "engine_build": s.base_build,
               "recomputed_sha256": recomputed, "why": recomputed.is_none().then_some("only AAPL's tape is on this machine"), "tape_bytes": 30_056_637_434_i64,
               "stamped": [], "differences": [], "verdict": verdict})
    };
    let r = s.http.put(format!("{caches}/AAPL/check?method_version=engine-v1")).bearer_auth(token).json(&check("AAPL", Some(&whole), Some(&verdict))).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.json::<Value>().await.unwrap(), json!({"outcome": "MATCH", "verdict_kept": true, "why": null, "new": true}));
    let r = s.http.put(format!("{caches}/MSFT/check?method_version=engine-v1")).bearer_auth(token).json(&check("AAPL", None, None)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY, "a check names the asset it is sent for");
    for member in claim["days"][0]["members"].as_array().unwrap().iter().filter(|m| m["ticker"].is_string()).map(|m| m["asset_key"].as_str().unwrap()).filter(|k| *k != "AAPL") {
        let r = s.http.put(format!("{caches}/{member}/check?method_version=engine-v1")).bearer_auth(token).json(&check(member, None, None)).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{member}");
    }
    assert_eq!(s.post(&format!("/tape-caches/{S}/done?method_version=engine-v1"), token, json!({})).await.0, StatusCode::OK);
    assert_eq!(s.post("/lease/finish", token, json!({})).await.0, StatusCode::OK);
    // the stored cache, and the verdict beside it, as a day worker reads them
    let day = s.claim("tape", &s.base_build, "tape-1").await.json::<Value>().await.unwrap();
    let tar_bytes = s.http.get(format!("{caches}?method_version=engine-v1")).bearer_auth(day["lease_token"].as_str().unwrap()).send().await.unwrap().bytes().await.unwrap();
    let mut archive = tar::Archive::new(&tar_bytes[..]);
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.path().unwrap().to_string_lossy().into_owned(), bytes));
    }
    assert_eq!((entries[0].0.as_str(), entries[0].1.as_slice()), ("AAPL", TAPE_AAPL));
    assert_eq!(serde_json::from_slice::<Value>(&entries[2].1).unwrap(), json!({"AAPL": verdict}));
    s.scratch.drop().await;
}
