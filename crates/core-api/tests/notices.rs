//! Notices: events logged with their transactions, problems posted once each, and the posts sent
//! in order, silently and naming no one, to a local stand-in for the Discord webhook. Run with
//! `cargo test -- --ignored`.
use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router, routing::post};
use core_api::notices::{payload, send_unsent};
use core_api::{App, Config};
use core_db::budget::Budget;
use core_db::work::{self, Kind};
use core_db::{build, migrate, notice, watch};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use test_support::slice::inventories;
use test_support::{CALENDAR, Scratch};

type Received = Arc<Mutex<Vec<Value>>>;

/// A webhook that refuses its first post as Discord does when rate limiting, then accepts.
async fn webhook(State(received): State<Received>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut received = received.lock().unwrap();
    if received.is_empty() && body["content"] != "again" {
        received.push(json!("refused once"));
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"retry_after": 0.5})));
    }
    received.push(body);
    (StatusCode::NO_CONTENT, Json(Value::Null))
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn problems_are_posted_once_in_order_silently_and_events_only_logged() {
    let scratch = Scratch::new("notices").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(CALENDAR).unwrap()).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    let settings = watch::Settings { profile: "FREE_REPORT_64".into(), deadlines_from: None, volume_bytes: None };

    // the events: a build admitted, a block done, the queue paused and resumed: logged, not posted (C-050)
    build::admit(&client, &b, &json!({"checklist": "notice test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, "FREE_REPORT_64", &b, "2025-12-10", "2025-12-11", 1).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "day-1", 3600.0).await.unwrap().unwrap();
    let tx = client.transaction().await.unwrap();
    let held = work::held(&tx, &claim.token).await.unwrap();
    work::session_done(&tx, &held, "2025-12-10").await.unwrap();
    tx.commit().await.unwrap();
    work::finish(&mut client, &claim.token).await.unwrap();
    work::pause(&client, "a check failed on 2025-12-11").await.unwrap();
    // the watch sees the pause: one alert however often it looks
    for _ in 0..3 {
        let now = watch::now(&client).await.unwrap();
        watch::check(&mut client, &settings, &now).await.unwrap();
    }
    work::resume(&client).await.unwrap();
    let now = watch::now(&client).await.unwrap();
    assert_eq!(watch::check(&mut client, &settings, &now).await.unwrap().resolved.len(), 1);
    // an event that is rolled back is not recorded
    let tx = client.transaction().await.unwrap();
    work::pause(&tx, "rolled back").await.unwrap();
    drop(tx);
    let logged: Vec<String> = notice::recent_log(&client, 20).await.unwrap().into_iter().rev().map(|(_, m)| m).collect();
    assert_eq!(logged.len(), 4, "{logged:?}");
    assert_eq!(logged[0], format!("Engine build {b} admitted: its workers may claim work."));
    assert_eq!(logged[1], "Day block 2025-12-10 to 2025-12-10 done by day-1 (1 blocks to go).");
    assert_eq!(logged[2], "Queue paused: a check failed on 2025-12-11. No work is handed out until `vyreon-core queue resume`.");
    assert_eq!(logged[3], "Queue resumed.");
    let queued: Vec<String> = notice::unsent(&client, 20).await.unwrap().into_iter().map(|(_, m)| m).collect();
    assert_eq!(queued.len(), 2, "{queued:?}");
    assert_eq!(queued[0], "🔴 Work queue paused, nothing is handed out: a check failed on 2025-12-11\nTo see more: `vyreon-core queue`");
    assert!(queued[1].starts_with("✅ Resolved: Work queue paused, nothing is handed out: a check failed on 2025-12-11 (it was open "), "{}", queued[1]);

    // the sender
    let received: Received = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/webhooks/1/a-secret-token", listener.local_addr().unwrap());
    let stub = Router::new().route("/webhooks/1/a-secret-token", post(webhook)).with_state(received.clone());
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(scratch.config.clone(), tokio_postgres::NoTls)).max_size(2).build().unwrap();
    let config = Config { worker_token_sha256: "0".repeat(64), spool: std::env::temp_dir(), lease_seconds: 300.0, budget: Budget::default(), max_bundle_bytes: 1 };
    let app = App::new(pool, config);
    let http = reqwest::Client::new();

    // an unreachable webhook: nothing is lost, and the error kept does not hold the secret URL
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let unreachable = send_unsent(&app, &http, &format!("http://{closed}/webhooks/1/a-secret-token")).await.unwrap_err();
    let kept: String = client.query_one("SELECT last_error FROM core.notice WHERE post ORDER BY notice_id LIMIT 1", &[]).await.unwrap().get(0);
    assert!(!unreachable.contains("a-secret-token") && !kept.contains("a-secret-token") && !kept.contains("127.0.0.1"), "{unreachable} / {kept}");
    // rate limited once: the notice waits and nothing overtakes it; then all go, in order
    assert!(send_unsent(&app, &http, &url).await.unwrap_err().contains("rate limited"));
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 2);
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 0);
    let posts = received.lock().unwrap().clone();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts[1..].iter().map(|p| p["content"].as_str().unwrap().to_owned()).collect::<Vec<_>>(), queued);
    for post in &posts[1..] {
        assert_eq!(post["flags"], 4096, "silent: no sound, no push");
        assert_eq!(post["allowed_mentions"], json!({"parse": []}), "names no one");
    }
    let state = client.query_one("SELECT count(*) FILTER (WHERE sent_at IS NOT NULL), max(attempts) FROM core.notice", &[]).await.unwrap();
    assert_eq!((state.get::<_, i64>(0), state.get::<_, i32>(1)), (2, 3), "the first was tried three times; the log is never sent");

    // the switch-on: whatever waits is marked sent, never posted
    let now = watch::now(&client).await.unwrap();
    work::pause(&client, "waiting while the webhook is off").await.unwrap();
    watch::check(&mut client, &settings, &now).await.unwrap();
    assert_eq!(notice::unsent(&client, 20).await.unwrap().len(), 1);
    assert_eq!(notice::mark_all_sent(&client).await.unwrap(), 1);
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 0);
    assert_eq!(received.lock().unwrap().len(), 3);

    // a long message is cut to Discord's limit
    let long = payload(&"x".repeat(5000));
    assert_eq!(long["content"].as_str().unwrap().chars().count(), 1901);
    scratch.drop().await;
}
