//! The owner's Discord channel (C-014, C-050; AGENTS.md: the channel only, never a phone ping):
//! the watch (`core_db::watch`: problems opened and resolved, the 09:00 digest, the Saturday
//! check, the daily runs' queueing) once a minute, and the sender, which posts what the watch
//! queued, each in its own task, so delivery never delays the watch (audit 2026-11-17 round v, 2). Each post is silent
//! and names no one. The webhook URL is a secret: it is read from a file, never logged, and
//! stripped from any error text.
use crate::App;
use core_db::notice::{self, Channel};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// Discord's `SUPPRESS_NOTIFICATIONS` message flag: the post makes no sound and no push.
const SILENT: u32 = 1 << 12;
/// Discord's content limit is 2,000 characters.
const LIMIT: usize = 1900;

/// The post's body: the message, cut to `LIMIT` with the characters left out counted, never cut
/// silently (audit 2026-11-22 prebuild round 2, core-watch.md, 14).
pub fn payload(message: &str) -> Value {
    let content = core_db::progress::fit(message, LIMIT);
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
    /// Discord refused it (a 4xx), or was never reached: send it again.
    NotSent(String),
    /// The request may have reached Discord, but no saved message came back (no answer, a 5xx, or
    /// a 2xx without the message).
    Unknown(String),
}

/// The webhook URL to post to: the configured one with its own `wait` parameters removed and
/// `wait=true` added, so that only one `wait` is sent (audit round x, 19: a configured
/// `wait=false` beside it would leave Discord to choose). Err never holds the URL (a secret).
pub fn waiting_url(webhook: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(webhook).map_err(|e| format!("the webhook is not a URL ({e})"))?;
    let kept: Vec<(String, String)> = url.query_pairs().filter(|(k, _)| k != "wait").map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    url.query_pairs_mut().clear().extend_pairs(kept).append_pair("wait", "true");
    Ok(url)
}

/// Posts one message with `wait=true`: Discord's Execute Webhook then "waits for server
/// confirmation of message send before response, and returns the created message body"; with the
/// default `false`, "a message that is not saved does not return an error"
/// (https://docs.discord.com/developers/resources/webhook#execute-webhook, read 2026-11-17).
/// Delivered only when the saved message (its `id`) comes back.
async fn post(http: &reqwest::Client, webhook: &str, message: &str) -> Outcome {
    let url = match waiting_url(webhook) {
        Ok(url) => url,
        Err(e) => return Outcome::NotSent(e),
    };
    let response = match http.post(url).timeout(POST_TIMEOUT).json(&payload(message)).send().await {
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
    // a 5xx (a gateway's 502, 503 or 504 among them) may come after Discord saved the post: its
    // outcome is unknown (audit round x, 12; RFC 9110, 15.6); a 4xx is a definite refusal
    if response.status().is_server_error() {
        return Outcome::Unknown(format!("status {status}"));
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
    send_unsent_on(app, http, webhook, Channel::Alerts).await
}

/// `send_unsent` for one channel (C-065): each channel's posts in their own order, to its own
/// webhook, so a progress post not delivered never holds back or repeats an alert. Progress posts
/// older than `progress::STALE_HOURS` are dropped first (a backlog is history the log keeps).
pub async fn send_unsent_on(app: &App, http: &reqwest::Client, webhook: &str, channel: Channel) -> Result<usize, String> {
    let mut sent = 0;
    for _ in 0..BATCH {
        // audit 2026-11-22 prebuild round 2 (core-watch.md) 13: a pooled connection only for each
        // step in the database, never held across a post or a rate limit's wait, so one channel's
        // slow webhook never keeps a connection another sender or the watch needs
        let p = {
            let client = app.pool.get().await.map_err(|e| e.to_string())?;
            let client: &tokio_postgres::Client = &client;
            // 15: the six-hour cutoff before each post, not once a batch
            if channel == Channel::Progress {
                notice::drop_stale_progress(client, core_db::progress::STALE_HOURS).await?;
            }
            notice::next_on(client, UNCONFIRMED_SECONDS, channel).await?
        };
        let Some(p) = p else {
            break;
        };
        let outcome = post(http, webhook, &p.message).await;
        let client = app.pool.get().await.map_err(|e| e.to_string())?;
        let client: &tokio_postgres::Client = &client;
        match outcome {
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
/// `every` (a minute in the service). Its own task: delivering posts never delays it. C-069: `/live`
/// is told when a tick ran its checks and recorded what failed (`watch::tick` Ok), never before:
/// a watch that cannot run, hangs or has ended (a panic) leaves it behind.
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
                    // `/live` (round x, 5; C-069): only a tick that ran its checks and recorded them
                    app.watching(std::time::Instant::now());
                    for f in &change.failing {
                        eprintln!("vyreon-core: watch: part failing: {f}");
                    }
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
    spawn_sender_on(app, webhook, every, Channel::Alerts)
}

/// The sender of one channel, in a task of its own (C-065: the progress channel's beside the
/// alerts', each with its own webhook, order and retries).
pub fn spawn_sender_on(app: Arc<App>, webhook: String, every: Duration, channel: Channel) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let http = reqwest::Client::builder().timeout(POST_TIMEOUT).build().expect("http client");
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = send_unsent_on(&app, &http, &webhook, channel).await {
                eprintln!("vyreon-core: notices ({}): {e}", channel.name());
            }
        }
    })
}

/// The progress channel's windows (C-065): every `every` (a minute in the service), a window of
/// `progress::WINDOW_MINUTES` composed and queued when due. Its own task, so counting a window's
/// days never delays the watch; started only when the progress webhook is set.
pub fn spawn_progress(app: Arc<App>, profile: String, label: String, phase: Option<(String, String)>, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Ok(mut client) = app.pool.get().await else {
                eprintln!("vyreon-core: progress: no database connection");
                continue;
            };
            let client: &mut tokio_postgres::Client = &mut client;
            let done = match core_db::watch::now(&*client).await {
                Ok(now) => core_db::progress::tick_phase(client, &profile, &label, phase.as_ref(), &now).await,
                Err(e) => Err(e),
            };
            match done {
                Ok(Some(w)) if w.posts > 0 => println!("vyreon-core: progress: {} lines queued", w.lines),
                Ok(_) => {}
                Err(e) => eprintln!("vyreon-core: progress: {e}"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit round x, 19: one `wait`, `true`, whatever the configured URL carries; its other
    /// parameters kept.
    #[test]
    fn the_webhook_url_carries_one_wait_true() {
        for (configured, sent) in [
            ("https://discord.com/api/webhooks/1/a-token", "https://discord.com/api/webhooks/1/a-token?wait=true"),
            ("https://discord.com/api/webhooks/1/a-token?wait=false", "https://discord.com/api/webhooks/1/a-token?wait=true"),
            ("https://discord.com/api/webhooks/1/a-token?wait=false&thread_id=7&wait=0", "https://discord.com/api/webhooks/1/a-token?thread_id=7&wait=true"),
        ] {
            assert_eq!(waiting_url(configured).unwrap().as_str(), sent);
        }
        let why = waiting_url("not a url with a-token").unwrap_err();
        assert!(!why.contains("a-token"), "{why}");
    }
}
