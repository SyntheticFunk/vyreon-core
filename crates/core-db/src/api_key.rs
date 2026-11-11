//! Keys for the reader API (migration 0007).
use tokio_postgres::Client;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// Makes a key for `name` with `scopes` and returns it. This is the only time it is known:
/// core keeps its SHA-256.
pub async fn create(client: &Client, name: &str, scopes: &[String]) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("a key names its holder".into());
    }
    let row = client
        .query_one(
            "WITH k AS (SELECT 'vyk_' || replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '') AS key)
             INSERT INTO core.api_key (name, key_sha256, scopes)
             SELECT $1, encode(sha256(convert_to(key, 'UTF8')), 'hex'), $2 FROM k RETURNING (SELECT key FROM k)",
            &[&name, &scopes],
        )
        .await
        .map_err(db)?;
    Ok(row.get(0))
}

/// Revokes the key held by `name`.
pub async fn revoke(client: &Client, name: &str) -> Result<(), String> {
    let n = client.execute("UPDATE core.api_key SET revoked_at = now() WHERE name = $1 AND revoked_at IS NULL", &[&name]).await.map_err(db)?;
    if n == 0 { Err(format!("no live key is held by {name}")) } else { Ok(()) }
}

/// The holder and scopes of a live key, or None.
pub async fn holder(client: &Client, key: &str) -> Result<Option<(String, Vec<String>)>, String> {
    let row = client
        .query_opt(
            "SELECT name, scopes FROM core.api_key WHERE key_sha256 = encode(sha256(convert_to($1, 'UTF8')), 'hex') AND revoked_at IS NULL",
            &[&key],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|r| (r.get(0), r.get(1))))
}
