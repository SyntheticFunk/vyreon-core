//! Notices (migrations 0008, 0018): written with the event, in the caller's transaction (the
//! transactional outbox, C-027). Since C-050 a notice is either posted to the owner's channel
//! (`alert`: a problem opened or resolved, the digest, the weekly check) or only kept as the log
//! (`log`: what happened, for `vyreon-core log`). The service posts only the first kind.
//!
//! C-065: a post is for one of two channels, each with its own sender and its own order: the
//! owner's alerts (every post before 0042), and the optional progress channel (`progress`).
use tokio_postgres::GenericClient;

/// Where a post goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// The owner's channel (C-050): problems, the digest, the weekly check.
    Alerts,
    /// The run's progress, batched (C-065); posted only when its webhook is set.
    Progress,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::Alerts => "alerts",
            Channel::Progress => "progress",
        }
    }
}

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// Records an event in the log, in the caller's transaction. Never posted.
pub async fn log(db_client: &impl GenericClient, message: &str) -> Result<(), String> {
    db_client.execute("INSERT INTO core.notice (message) VALUES ($1)", &[&message]).await.map_err(db)?;
    Ok(())
}

/// Queues a post for the owner's channel, in the caller's transaction.
pub async fn alert(db_client: &impl GenericClient, message: &str) -> Result<(), String> {
    // (owner, 2026-11-25) a post is in plain words: no internal codes; C-091: no command, file or secret (`plain::for_alert`)
    let message = crate::plain::for_alert(message);
    db_client.execute("INSERT INTO core.notice (message, post) VALUES ($1, true)", &[&message]).await.map_err(db)?;
    Ok(())
}

/// Queues a post unless one with the same key was queued before. True when queued.
pub async fn alert_once(db_client: &impl GenericClient, key: &str, message: &str) -> Result<bool, String> {
    let message = crate::plain::for_alert(message);
    let n = db_client
        .execute("INSERT INTO core.notice (message, once, post) VALUES ($1, $2, true) ON CONFLICT (once) DO NOTHING", &[&message, &key])
        .await
        .map_err(db)?;
    Ok(n == 1)
}

/// Queues a post for the progress channel under a key, in the caller's transaction (C-065).
pub async fn progress(db_client: &impl GenericClient, key: &str, message: &str) -> Result<bool, String> {
    let message = crate::plain::plain(message);
    let n = db_client
        .execute(
            "INSERT INTO core.notice (message, once, post, channel) VALUES ($1, $2, true, 'progress') ON CONFLICT (once) DO NOTHING",
            &[&message, &key],
        )
        .await
        .map_err(db)?;
    Ok(n == 1)
}

/// Progress posts not sent within `hours` are not sent at all (marked so, kept in the table): the
/// channel says what is happening, and a backlog after its webhook was down is history the log
/// keeps. Returns how many.
pub async fn drop_stale_progress(db_client: &impl GenericClient, hours: f64) -> Result<u64, String> {
    db_client
        .execute(
            "UPDATE core.notice SET sent_at = now(), last_error = 'not posted: older than ' || $1::float8 || ' h (progress)', sending_since = NULL
             WHERE channel = 'progress' AND post AND sent_at IS NULL AND created_at < now() - make_interval(secs => $1::float8 * 3600)",
            &[&hours],
        )
        .await
        .map_err(db)
}

/// Whether a notice with this key was queued.
pub async fn queued(db_client: &impl GenericClient, key: &str) -> Result<bool, String> {
    Ok(db_client.query_opt("SELECT FROM core.notice WHERE once = $1", &[&key]).await.map_err(db)?.is_some())
}

/// The oldest posts for the owner's channel not sent yet.
pub async fn unsent(db_client: &impl GenericClient, limit: i64) -> Result<Vec<(i64, String)>, String> {
    unsent_on(db_client, limit, Channel::Alerts).await
}

/// The oldest posts for `channel` not sent yet.
pub async fn unsent_on(db_client: &impl GenericClient, limit: i64, channel: Channel) -> Result<Vec<(i64, String)>, String> {
    let rows = db_client
        .query(
            "SELECT notice_id, message FROM core.notice WHERE post AND sent_at IS NULL AND channel = $2 ORDER BY notice_id LIMIT $1",
            &[&limit, &channel.name()],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// A post taken for sending: its id and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: i64,
    pub message: String,
}

/// Takes the oldest post not sent yet for sending, and marks when it is being sent
/// (`sending_since`, migration 0021). Nothing while that post's last send has an outcome not known
/// (Discord may have saved it) younger than `unconfirmed_seconds`: it is not sent again before,
/// and no later post overtakes it. One sender at a time (one core, C-053), so no row lock or
/// claim is needed; the mark covers a restart between a send and its record.
pub async fn next(db_client: &impl GenericClient, unconfirmed_seconds: i32) -> Result<Option<Post>, String> {
    next_on(db_client, unconfirmed_seconds, Channel::Alerts).await
}

/// `next` for one channel: each channel's posts go in their own order, so a progress post not
/// delivered never holds back an alert, nor the reverse (C-065).
pub async fn next_on(db_client: &impl GenericClient, unconfirmed_seconds: i32, channel: Channel) -> Result<Option<Post>, String> {
    let row = db_client
        .query_opt(
            "UPDATE core.notice SET sending_since = clock_timestamp()
             WHERE notice_id = (SELECT notice_id FROM core.notice WHERE post AND sent_at IS NULL AND channel = $2 ORDER BY notice_id LIMIT 1)
               AND (sending_since IS NULL OR sending_since < clock_timestamp() - make_interval(secs => $1::int))
             RETURNING notice_id, message",
            &[&unconfirmed_seconds, &channel.name()],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|r| Post { id: r.get(0), message: r.get(1) }))
}

/// Records a post as sent (Discord returned the saved message).
pub async fn sent(db_client: &impl GenericClient, id: i64) -> Result<(), String> {
    db_client
        .execute("UPDATE core.notice SET sent_at = now(), attempts = attempts + 1, last_error = NULL, sending_since = NULL WHERE notice_id = $1", &[&id])
        .await
        .map_err(db)?;
    Ok(())
}

/// Records a post as not sent: Discord answered with an error (or was never reached), so it is
/// sent again at the next send.
pub async fn not_sent(db_client: &impl GenericClient, id: i64, error: &str) -> Result<(), String> {
    db_client
        .execute("UPDATE core.notice SET attempts = attempts + 1, last_error = $2, sending_since = NULL WHERE notice_id = $1", &[&id, &error])
        .await
        .map_err(db)?;
    Ok(())
}

/// Records a send whose outcome is not known (the request may have reached Discord, but no saved
/// message came back): the post keeps its `sending_since`, so it is sent again only after the
/// window `next` is given (at least once, never in a tight loop).
pub async fn unconfirmed(db_client: &impl GenericClient, id: i64, error: &str) -> Result<(), String> {
    db_client
        .execute("UPDATE core.notice SET attempts = attempts + 1, last_error = $2 WHERE notice_id = $1", &[&id, &error])
        .await
        .map_err(db)?;
    Ok(())
}

/// Marks every post not sent yet as sent, without posting it: the channel starts clean when the
/// sender is switched on (C-050). Logged notices are never posted. Returns how many were marked.
pub async fn mark_all_sent(db_client: &impl GenericClient) -> Result<u64, String> {
    db_client
        .execute("UPDATE core.notice SET sent_at = now(), last_error = 'marked sent without posting (C-050)' WHERE post AND sent_at IS NULL", &[])
        .await
        .map_err(db)
}

/// The newest `limit` logged events, newest first: (when, on the owner's clock (C-072's fixed
/// UTC−6, "MT"; C-073, audit c072 r4: it was UTC, a day off from the progress channel at night), and
/// what).
pub async fn recent_log(db_client: &impl GenericClient, limit: i64) -> Result<Vec<(String, String)>, String> {
    let rows = db_client
        .query(
            "SELECT to_char(created_at AT TIME ZONE $2, 'YYYY-MM-DD HH24:MI:SS \"MT\"'), message FROM core.notice WHERE NOT post ORDER BY notice_id DESC LIMIT $1",
            &[&limit, &crate::watch::ZONE],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}
