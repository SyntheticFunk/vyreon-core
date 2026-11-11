//! Notices (migration 0008): written with the event, sent later by the service.
use tokio_postgres::GenericClient;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// Queues a notice in the caller's transaction.
pub async fn post(db_client: &impl GenericClient, message: &str) -> Result<(), String> {
    db_client.execute("INSERT INTO core.notice (message) VALUES ($1)", &[&message]).await.map_err(db)?;
    Ok(())
}

/// Queues a notice unless one with the same key was queued before.
pub async fn post_once(db_client: &impl GenericClient, key: &str, message: &str) -> Result<bool, String> {
    let n = db_client.execute("INSERT INTO core.notice (message, once) VALUES ($1, $2) ON CONFLICT (once) DO NOTHING", &[&message, &key]).await.map_err(db)?;
    Ok(n == 1)
}

/// The oldest unsent notices.
pub async fn unsent(db_client: &impl GenericClient, limit: i64) -> Result<Vec<(i64, String)>, String> {
    let rows = db_client.query("SELECT notice_id, message FROM core.notice WHERE sent_at IS NULL ORDER BY notice_id LIMIT $1", &[&limit]).await.map_err(db)?;
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

/// Warns, once a week, when the engine's trading calendar has fewer than `days` days left: daily
/// runs stop at its last session (C-014f).
pub async fn calendar_check(db_client: &impl GenericClient, days: i32) -> Result<bool, String> {
    let row = db_client
        .query_one("SELECT max(session)::text, (max(session) - current_date)::int, to_char(current_date, 'IYYY\"-W\"IW') FROM core.session", &[])
        .await
        .map_err(db)?;
    let (last, left, week): (Option<String>, Option<i32>, String) = (row.get(0), row.get(1), row.get(2));
    match (last, left) {
        (Some(last), Some(left)) if left < days => {
            post_once(db_client, &format!("calendar {last} {week}"), &format!("The trading calendar ends on {last}, in {left} days. Daily runs stop there: the engine needs a later calendar.")).await
        }
        _ => Ok(false),
    }
}
