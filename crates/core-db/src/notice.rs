//! Notices (migrations 0008, 0018): written with the event, in the caller's transaction (the
//! transactional outbox, C-027). Since C-050 a notice is either posted to the owner's channel
//! (`alert`: a problem opened or resolved, the digest, the weekly check) or only kept as the log
//! (`log`: what happened, for `vyreon-core log`). The service posts only the first kind.
use tokio_postgres::GenericClient;

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
    db_client.execute("INSERT INTO core.notice (message, post) VALUES ($1, true)", &[&message]).await.map_err(db)?;
    Ok(())
}

/// Queues a post unless one with the same key was queued before. True when queued.
pub async fn alert_once(db_client: &impl GenericClient, key: &str, message: &str) -> Result<bool, String> {
    let n = db_client
        .execute("INSERT INTO core.notice (message, once, post) VALUES ($1, $2, true) ON CONFLICT (once) DO NOTHING", &[&message, &key])
        .await
        .map_err(db)?;
    Ok(n == 1)
}

/// Whether a notice with this key was queued.
pub async fn queued(db_client: &impl GenericClient, key: &str) -> Result<bool, String> {
    Ok(db_client.query_opt("SELECT FROM core.notice WHERE once = $1", &[&key]).await.map_err(db)?.is_some())
}

/// The oldest posts not sent yet.
pub async fn unsent(db_client: &impl GenericClient, limit: i64) -> Result<Vec<(i64, String)>, String> {
    let rows = db_client
        .query("SELECT notice_id, message FROM core.notice WHERE post AND sent_at IS NULL ORDER BY notice_id LIMIT $1", &[&limit])
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

pub async fn sent(db_client: &impl GenericClient, id: i64) -> Result<(), String> {
    db_client.execute("UPDATE core.notice SET sent_at = now(), attempts = attempts + 1, last_error = NULL WHERE notice_id = $1", &[&id]).await.map_err(db)?;
    Ok(())
}

pub async fn not_sent(db_client: &impl GenericClient, id: i64, error: &str) -> Result<(), String> {
    db_client.execute("UPDATE core.notice SET attempts = attempts + 1, last_error = $2 WHERE notice_id = $1", &[&id, &error]).await.map_err(db)?;
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

/// The newest `limit` logged events, newest first: (when, in UTC, and what).
pub async fn recent_log(db_client: &impl GenericClient, limit: i64) -> Result<Vec<(String, String)>, String> {
    let rows = db_client
        .query(
            "SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), message FROM core.notice WHERE NOT post ORDER BY notice_id DESC LIMIT $1",
            &[&limit],
        )
        .await
        .map_err(db)?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}
