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

/// How long a sender's claim on a post lasts (migration 0021). Longer than one post can take
/// (`POST_TIMEOUT`, plus at most 30 s waiting out a rate limit), so a live sender never loses its
/// claim; a sender that stopped mid-post loses it after this, and the post is sent again.
pub const CLAIM_SECONDS: i32 = 120;
/// The longest one post may take.
pub const POST_TIMEOUT: Duration = Duration::from_secs(20);
/// At most this many posts per send.
const BATCH: usize = 20;

/// Sends what is unsent, oldest first, stopping at the first failure so notices stay in order.
/// Each post is claimed first (`notice::claim_next`): two senders never post the same one, and a
/// post another sender is sending stops this one. Returns how many were sent.
pub async fn send_unsent(app: &App, http: &reqwest::Client, webhook: &str) -> Result<usize, String> {
    let client = app.pool.get().await.map_err(|e| e.to_string())?;
    let client: &tokio_postgres::Client = &client;
    let mut sent = 0;
    for _ in 0..BATCH {
        let Some(claimed) = notice::claim_next(client, CLAIM_SECONDS).await? else {
            break;
        };
        let outcome = match http.post(webhook).timeout(POST_TIMEOUT).json(&payload(&claimed.message)).send().await {
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
                if !notice::sent(client, &claimed).await? {
                    return Err(format!("notice {}: posted after its claim had lapsed", claimed.id));
                }
                sent += 1;
            }
            Err(e) => {
                notice::not_sent(client, &claimed, &e).await?;
                return Err(format!("notice {}: {e}", claimed.id));
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
