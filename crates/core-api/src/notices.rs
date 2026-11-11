//! Sends the queued notices to the owner's Discord channel (C-014; AGENTS.md: the channel only,
//! never a phone ping). Each post is silent and names no one. The webhook URL is a secret: it is
//! read from a file, never logged, and stripped from any error text.
use crate::App;
use core_db::notice;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// Discord's `SUPPRESS_NOTIFICATIONS` message flag: the post makes no sound and no push.
const SILENT: u32 = 1 << 12;
/// Discord's content limit is 2,000 characters.
const LIMIT: usize = 1900;

pub fn payload(message: &str) -> Value {
    let content: String = if message.chars().count() > LIMIT { message.chars().take(LIMIT).chain("…".chars()).collect() } else { message.to_owned() };
    json!({"content": content, "flags": SILENT, "allowed_mentions": {"parse": []}, "username": "Vyreon core"})
}

/// Sends what is unsent, oldest first, stopping at the first failure so notices stay in order.
/// Returns how many were sent.
pub async fn send_unsent(app: &App, http: &reqwest::Client, webhook: &str) -> Result<usize, String> {
    let client = app.pool.get().await.map_err(|e| e.to_string())?;
    let client: &tokio_postgres::Client = &client;
    let mut sent = 0;
    for (id, message) in notice::unsent(client, 20).await? {
        let outcome = match http.post(webhook).json(&payload(&message)).send().await {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) if r.status().as_u16() == 429 => {
                // Discord says how long to wait
                let wait = r.json::<Value>().await.ok().and_then(|v| v["retry_after"].as_f64()).unwrap_or(2.0).clamp(0.5, 30.0);
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                Err("rate limited".to_owned())
            }
            Ok(r) => Err(format!("status {}", r.status().as_u16())),
            Err(e) => Err(e.without_url().to_string()),
        };
        match outcome {
            Ok(()) => {
                notice::sent(client, id).await?;
                sent += 1;
            }
            Err(e) => {
                notice::not_sent(client, id, &e).await?;
                return Err(format!("notice {id}: {e}"));
            }
        }
    }
    Ok(sent)
}

/// Sends notices every quarter minute and checks the calendar's end once an hour.
pub fn spawn_sender(app: Arc<App>, webhook: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("http client");
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        let mut ticks = 0u64;
        loop {
            tick.tick().await;
            if ticks.is_multiple_of(240)
                && let Ok(client) = app.pool.get().await
            {
                let client: &tokio_postgres::Client = &client;
                let _ = notice::calendar_check(client, 45).await;
            }
            ticks += 1;
            if let Err(e) = send_unsent(&app, &http, &webhook).await {
                eprintln!("vyreon-core: notices: {e}");
            }
        }
    })
}
