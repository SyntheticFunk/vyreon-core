//! Notices: events logged with their transactions, problems posted once each, and the posts sent
//! in order, silently and naming no one, to a local stand-in for the Discord webhook. Run with
//! `cargo test -- --ignored`.
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing::post};
use core_api::notices::{payload, send_unsent};
use core_api::{App, Config};
use core_db::budget::Budget;
use core_db::work::{self, Kind};
use core_db::{build, migrate, notice, watch};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use test_support::slice::inventories;
use test_support::{CALENDAR, Scratch};

type Received = Arc<Mutex<Vec<Value>>>;

/// A webhook that refuses its first post as Discord does when rate limiting, then accepts,
/// returning the saved message as Discord's Execute Webhook does with `wait=true` (only then).
async fn webhook(State(received): State<Received>, Query(q): Query<HashMap<String, String>>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut received = received.lock().unwrap();
    if received.is_empty() && body["content"] != "again" {
        received.push(json!("refused once"));
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"retry_after": 0.5})));
    }
    received.push(body.clone());
    match q.get("wait").map(String::as_str) {
        Some("true") => (StatusCode::OK, Json(json!({"id": format!("{}", 1_000 + received.len()), "content": body["content"]}))),
        _ => (StatusCode::NO_CONTENT, Json(Value::Null)),
    }
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

/// A webhook that answers 204 without the saved message (Discord's answer without `wait=true`).
async fn unconfirming_webhook(State(received): State<Received>, Json(body): Json<Value>) -> StatusCode {
    received.lock().unwrap().push(body);
    StatusCode::NO_CONTENT
}

/// Moves a post's send mark 121 s back: its window has passed.
async fn back(c: &tokio_postgres::Client, id: i64) {
    c.execute("UPDATE core.notice SET sending_since = sending_since - interval '121 seconds' WHERE notice_id = $1", &[&id]).await.unwrap();
}

/// A post's (sent, attempts, last error).
async fn state(c: &tokio_postgres::Client, id: i64) -> (bool, i32, Option<String>) {
    let r = c.query_one("SELECT sent_at IS NOT NULL, attempts, last_error FROM core.notice WHERE notice_id = $1", &[&id]).await.unwrap();
    (r.get(0), r.get(1), r.get(2))
}

/// Audit 2026-11-17 round w, 6 and 11: a post is delivered only when Discord returns the saved
/// message (`wait=true`); a send whose outcome is unknown (a 2xx without the message, or the
/// answer lost after the request went) is not sent again for `UNCONFIRMED_SECONDS`, then once; a
/// restart between a send and its record is the same window. One sender (one core, C-053).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_post_is_sent_when_saved_and_an_unknown_outcome_waits_its_window() {
    let (scratch, client, app) = ready_app("notice_outcomes").await;
    let http = reqwest::Client::new();
    // 204 without the saved message: not counted as delivered, and not sent again at once
    notice::alert(&client, "first").await.unwrap();
    notice::alert(&client, "second").await.unwrap();
    let first: i64 = client.query_one("SELECT min(notice_id) FROM core.notice", &[]).await.unwrap().get(0);
    let received: Received = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let stub = Router::new().route("/hook", post(unconfirming_webhook)).with_state(received.clone());
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    let why = send_unsent(&app, &http, &url).await.unwrap_err();
    assert!(why.contains("without the saved message") && why.contains("not sent again for 120 s"), "{why}");
    assert_eq!(state(&client, first).await.0, false);
    assert_eq!(send_unsent(&app, &http, &url).await.unwrap(), 0, "inside the window: nothing, and nothing overtakes it");
    assert_eq!(received.lock().unwrap().len(), 1);

    // the answer lost after the request went (the connection dropped): the same window
    let dropping = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lost_url = format!("http://{}/hook", dropping.local_addr().unwrap());
    let reached = Arc::new(Mutex::new(0usize));
    let counted = reached.clone();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        while let Ok((mut socket, _)) = dropping.accept().await {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            *counted.lock().unwrap() += 1;
            drop(socket);
        }
    });
    back(&client, first).await;
    let why = send_unsent(&app, &http, &lost_url).await.unwrap_err();
    assert!(why.contains("no answer") && why.contains("not sent again"), "{why}");
    assert_eq!(send_unsent(&app, &http, &lost_url).await.unwrap(), 0, "a lost answer is not followed by an immediate repost");
    assert_eq!(*reached.lock().unwrap(), 1);

    // never connected: nothing reached Discord, so it goes again at the next send
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    back(&client, first).await;
    assert!(send_unsent(&app, &http, &format!("http://{closed}/hook")).await.is_err());
    let (_, attempts, _) = state(&client, first).await;
    let saved: Received = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let good = format!("http://{}/webhooks/1/a-secret-token", listener.local_addr().unwrap());
    let stub = Router::new().route("/webhooks/1/a-secret-token", post(webhook)).with_state(saved.clone());
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });
    saved.lock().unwrap().push(json!("no rate limit here"));
    assert_eq!(send_unsent(&app, &http, &good).await.unwrap(), 2, "a refused connection is retried at once; then both, in order");
    assert_eq!(state(&client, first).await, (true, attempts + 1, None));
    let posts: Vec<String> = saved.lock().unwrap()[1..].iter().map(|p| p["content"].as_str().unwrap().to_owned()).collect();
    assert_eq!(posts, ["first", "second"]);

    // a restart between a send and its record: the mark stays, so it waits the window, then once
    notice::alert(&client, "after a restart").await.unwrap();
    let crashed = notice::next(&client, core_api::notices::UNCONFIRMED_SECONDS).await.unwrap().unwrap();
    assert_eq!(crashed.message, "after a restart");
    assert_eq!(send_unsent(&app, &http, &good).await.unwrap(), 0);
    back(&client, crashed.id).await;
    assert_eq!(send_unsent(&app, &http, &good).await.unwrap(), 1);
    assert_eq!(send_unsent(&app, &http, &good).await.unwrap(), 0);
    assert_eq!(saved.lock().unwrap().len(), 4);
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

/// Audit round x, 5: `/live`, the liveness probe's, answers while the watch ticks and fails once
/// it has not started a tick within `LIVE_WITHIN` (a frozen core is restarted by Kubernetes and its
/// single-instance lock freed); a watch ticking again makes it live again.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_core_whose_watch_stalled_is_not_live() {
    let (scratch, _client, app) = ready_app("liveness").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/live", listener.local_addr().unwrap());
    let router = core_api::router(app.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    async fn status(url: &str) -> u16 {
        reqwest::get(url).await.unwrap().status().as_u16()
    }
    assert_eq!(status(&url).await, 200, "just started");
    app.watching(std::time::Instant::now().checked_sub(core_api::LIVE_WITHIN + std::time::Duration::from_secs(1)).unwrap());
    assert_eq!(status(&url).await, 503, "the watch stalled");
    let settings = watch::Settings { profile: "FREE_REPORT_64".into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None };
    let watcher = core_api::notices::spawn_watch(app.clone(), settings, std::time::Duration::from_millis(100));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(status(&url).await, 200, "the watch ticks again");
    watcher.abort();
    scratch.drop().await;
}
