//! The owner's Discord channel (C-014, C-050; AGENTS.md: the channel only, never a phone ping):
//! the watch (`core_db::watch`: problems opened and resolved, the 09:00 digest, the Saturday
//! check, the daily runs' queueing) once a minute, and the sender, which posts what the watch
//! queued, each in its own task, so delivery never delays the watch (audit 2026-11-17 round v, 2). Each post is silent
//! and names no one. The webhook URL is a secret: it is read from a file, never logged, and
//! stripped from any error text.
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

/// How long a post whose send has no known outcome waits before it is sent again (migration
/// 0021's `sending_since`): longer than one post can take (`POST_TIMEOUT`, plus at most 30 s
/// waiting out a rate limit).
pub const UNCONFIRMED_SECONDS: i32 = 120;
/// The longest one post may take.
pub const POST_TIMEOUT: Duration = Duration::from_secs(20);
/// At most this many posts per send.
const BATCH: usize = 20;

/// What became of one post.
enum Outcome {
    /// Discord returned the saved message.
    Delivered,
    /// Discord answered with an error, or was never reached: send it again.
    NotSent(String),
    /// The request may have reached Discord, but no saved message came back.
    Unknown(String),
}

/// Posts one message with `wait=true`: Discord's Execute Webhook then "waits for server
/// confirmation of message send before response, and returns the created message body"; with the
/// default `false`, "a message that is not saved does not return an error"
/// (https://docs.discord.com/developers/resources/webhook#execute-webhook, read 2026-11-17).
/// Delivered only when the saved message (its `id`) comes back.
async fn post(http: &reqwest::Client, webhook: &str, message: &str) -> Outcome {
    let response = match http.post(webhook).query(&[("wait", "true")]).timeout(POST_TIMEOUT).json(&payload(message)).send().await {
        Ok(r) => r,
        // never connected: nothing reached Discord
        Err(e) if e.is_connect() || e.is_builder() => return Outcome::NotSent(e.without_url().to_string()),
        Err(e) => return Outcome::Unknown(format!("no answer: {}", e.without_url())),
    };
    let status = response.status().as_u16();
    if status == 429 {
        // Discord says how long to wait
        let wait = response.json::<Value>().await.ok().and_then(|v| v["retry_after"].as_f64()).unwrap_or(2.0).clamp(0.5, 30.0);
        tokio::time::sleep(Duration::from_secs_f64(wait)).await;
        return Outcome::NotSent("rate limited".to_owned());
    }
    if !response.status().is_success() {
        return Outcome::NotSent(format!("status {status}"));
    }
    match response.json::<Value>().await {
        Ok(saved) if saved["id"].as_str().is_some_and(|id| !id.is_empty()) => Outcome::Delivered,
        Ok(_) => Outcome::Unknown(format!("status {status} without the saved message")),
        Err(e) => Outcome::Unknown(format!("status {status} without the saved message ({})", e.without_url())),
    }
}

/// Sends what is unsent, oldest first, stopping at the first post not delivered so notices stay
/// in order. One sender (one core, C-053). A post whose outcome is unknown is not sent again for
/// `UNCONFIRMED_SECONDS`: at least once, the residual is a duplicate after a lost answer or a
/// restart mid-send (Discord's webhook takes no idempotency key). Returns how many were sent.
pub async fn send_unsent(app: &App, http: &reqwest::Client, webhook: &str) -> Result<usize, String> {
    let client = app.pool.get().await.map_err(|e| e.to_string())?;
    let client: &tokio_postgres::Client = &client;
    let mut sent = 0;
    for _ in 0..BATCH {
        let Some(p) = notice::next(client, UNCONFIRMED_SECONDS).await? else {
            break;
        };
        match post(http, webhook, &p.message).await {
            Outcome::Delivered => {
                notice::sent(client, p.id).await?;
                sent += 1;
            }
            Outcome::NotSent(e) => {
                notice::not_sent(client, p.id, &e).await?;
                return Err(format!("notice {}: {e}", p.id));
            }
            Outcome::Unknown(e) => {
                notice::unconfirmed(client, p.id, &e).await?;
                return Err(format!("notice {}: {e}; not sent again for {UNCONFIRMED_SECONDS} s", p.id));
            }
        }
    }
    Ok(sent)
}

/// The watch (problems, the digest, the weekly check, and the daily runs' queueing, C-051) every
/// `every` (a minute in the service). Its own task: delivering posts never delays it.
pub fn spawn_watch(app: Arc<App>, settings: core_db::watch::Settings, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // `/live` (round x, 5): a tick that never ends leaves this time behind
            app.watching(std::time::Instant::now());
            let Ok(mut client) = app.pool.get().await else {
                eprintln!("vyreon-core: watch: no database connection");
                continue;
            };
            let client: &mut tokio_postgres::Client = &mut client;
            let watched = match core_db::watch::now(&*client).await {
                Ok(now) => core_db::watch::tick(client, &settings, &now).await,
                Err(e) => Err(e),
            };
            match watched {
                Ok(change) => {
                    for s in &change.opened {
                        println!("vyreon-core: problem opened: {s}");
                    }
                    for s in &change.resolved {
                        println!("vyreon-core: problem resolved: {s}");
                    }
                }
                Err(e) => eprintln!("vyreon-core: watch: {e}"),
            }
        }
    })
}

/// The sender: what the watch queued, posted every `every` (a quarter minute in the service), in
/// a task of its own. Without a webhook nothing is started and the posts wait unsent
/// (`vyreon-core notices mark-sent` clears them before the sender is switched on).
pub fn spawn_sender(app: Arc<App>, webhook: String, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let http = reqwest::Client::builder().timeout(POST_TIMEOUT).build().expect("http client");
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = send_unsent(&app, &http, &webhook).await {
                eprintln!("vyreon-core: notices: {e}");
            }
        }
    })
}
