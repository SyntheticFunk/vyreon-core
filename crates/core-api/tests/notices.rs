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
    let settings = watch::Settings { profile: "FREE_REPORT_64".into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None };

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
    let config = Config { worker_token_sha256: "0".repeat(64), spool: std::env::temp_dir(), lease_seconds: 300.0, budget: Budget::default(), max_bundle_bytes: 1, archive: None };
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

async fn ready_app(test: &str) -> (Scratch, tokio_postgres::Client, Arc<App>) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(CALENDAR).unwrap()).await.unwrap();
    let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(scratch.config.clone(), tokio_postgres::NoTls)).max_size(4).build().unwrap();
    let config = Config { worker_token_sha256: "0".repeat(64), spool: std::env::temp_dir(), lease_seconds: 300.0, budget: Budget::default(), max_bundle_bytes: 1, archive: None };
    (scratch, client, App::new(pool, config))
}

/// A webhook that takes its time (300 ms a post) and records what it was sent.
async fn slow_webhook(State(received): State<Received>, Json(body): Json<Value>) -> StatusCode {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    received.lock().unwrap().push(body);
    StatusCode::NO_CONTENT
}

/// Audit 2026-11-17 round v, 8: a post is claimed before it is sent. Two senders at once post each
/// notice once, in order; a post claimed by a sender that stopped before recording it waits while
/// the claim is live and goes once after it lapses (the stated at-least-once window, C-050).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn two_senders_post_each_notice_once_and_a_lapsed_claim_once_more() {
    let (scratch, client, app) = ready_app("notice_claims").await;
    for i in 0..6 {
        notice::alert(&client, &format!("post {i}")).await.unwrap();
    }
    let received: Received = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let stub = Router::new().route("/hook", post(slow_webhook)).with_state(received.clone());
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let http = reqwest::Client::new();
    let (a, b) = tokio::join!(send_unsent(&app, &http, &url), send_unsent(&app, &http, &url));
    // the second sender stops at a post the first holds; either may stop that way, nothing goes twice
    let total = a.unwrap_or(0) + b.unwrap_or(0);
    let mut sent = 0;
    while sent + total < 6 {
        sent += send_unsent(&app, &http, &url).await.unwrap();
    }
    let posts: Vec<String> = received.lock().unwrap().iter().map(|p| p["content"].as_str().unwrap().to_owned()).collect();
    assert_eq!(posts, (0..6).map(|i| format!("post {i}")).collect::<Vec<_>>(), "each once, in order");

    // a sender claimed a post and stopped before recording it: nothing goes while its claim is live
    notice::alert(&client, "after a crash").await.unwrap();
    let crashed = notice::claim_next(&client, core_api::notices::CLAIM_SECONDS).await.unwrap().unwrap();
    assert_eq!(crashed.message, "after a crash");
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 0);
    assert_eq!(received.lock().unwrap().len(), 6);
    // once the claim lapses it is sent, once; the stopped sender's late record changes nothing
    client.execute("UPDATE core.notice SET sending_since = sending_since - interval '121 seconds' WHERE notice_id = $1", &[&crashed.id]).await.unwrap();
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 1);
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 0);
    assert!(!notice::sent(&client, &crashed).await.unwrap());
    assert_eq!(received.lock().unwrap().len(), 7);
    let unsent: i64 = client.query_one("SELECT count(*) FROM core.notice WHERE post AND sent_at IS NULL", &[]).await.unwrap().get(0);
    assert_eq!(unsent, 0);
    scratch.drop().await;
}

/// Audit 2026-11-17 round v, 2: the watch runs in its own task; a webhook that never answers holds
/// the sender, not the watch (which also queues the daily runs, C-051).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_watch_runs_while_the_sender_is_stuck() {
    let (scratch, client, app) = ready_app("watch_apart").await;
    let settings = watch::Settings { profile: "FREE_REPORT_64".into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None };
    for i in 0..5 {
        notice::alert(&client, &format!("backlog {i}")).await.unwrap();
    }
    // a webhook that accepts connections and never answers
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let sender = core_api::notices::spawn_sender(app.clone(), url, std::time::Duration::from_millis(50));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let watcher = core_api::notices::spawn_watch(app.clone(), settings, std::time::Duration::from_millis(200));
    work::pause(&client, "checking the watch runs").await.unwrap();
    let mut opened = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let n: i64 = client.query_one("SELECT count(*) FROM core.problem WHERE kind = 'QUEUE_PAUSED' AND resolved_at IS NULL", &[]).await.unwrap().get(0);
        if n == 1 {
            opened = true;
            break;
        }
    }
    assert!(opened, "the watch saw the pause within 3 s while the sender waited on the webhook");
    let sent: i64 = client.query_one("SELECT count(*) FROM core.notice WHERE sent_at IS NOT NULL", &[]).await.unwrap().get(0);
    assert_eq!(sent, 0, "the webhook never answered");
    sender.abort();
    watcher.abort();
    scratch.drop().await;
}
