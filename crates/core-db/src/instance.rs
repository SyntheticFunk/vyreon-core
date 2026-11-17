//! One core at a time (C-053). `vyreon-core serve` runs as one replica (deploy/k8s/20-core.yaml,
//! strategy `Recreate`); this makes that an invariant the database enforces rather than a
//! deployment setting: at startup the service takes a session-level advisory lock on a connection
//! it keeps for its whole life, and refuses to serve if another session holds it. The watch, the
//! sender and every check written for one core (one tick at a time, one sender) rely on it.
//! The command-line tools (`vyreon-core publish`, `queue`, `digest` …) do not take it: they are
//! the operator's, and what they write is safe beside the service (each takes its own row or
//! advisory locks).
use tokio_postgres::GenericClient;

/// The lock's key, in the database's advisory-lock space (per database).
const KEY: &str = "vyreon-core serve";

/// Takes the single-instance lock for this session. False: another session holds it. The lock
/// lasts until this session ends (PostgreSQL releases a session-level advisory lock when its
/// connection closes).
pub async fn take(c: &impl GenericClient) -> Result<bool, String> {
    c.query_one("SELECT pg_try_advisory_lock(hashtextextended($1, 0))", &[&KEY])
        .await
        .map(|r| r.get(0))
        .map_err(|e| e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned()))
}

/// Whether this session still holds the lock (a ping of the connection that holds it).
pub async fn held(c: &impl GenericClient) -> Result<bool, String> {
    c.query_one(
        "SELECT EXISTS (SELECT FROM pg_locks WHERE locktype = 'advisory' AND pid = pg_backend_pid() AND granted
                          AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
                          AND ((classid::bigint << 32) | objid::bigint) = hashtextextended($1, 0))",
        &[&KEY],
    )
    .await
    .map(|r| r.get(0))
    .map_err(|e| e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned()))
}
