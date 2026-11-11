//! The migration runner. Migrations are SQL files `NNNN_name.sql`, numbered from 0001 without
//! gaps and embedded in the binary. A run takes a PostgreSQL advisory lock, checks the SHA-256 of
//! every migration already applied against the ledger `core.schema_migration`, and applies each
//! new one in its own transaction with its ledger row. It is safe to run repeatedly and from
//! several processes at once. An applied migration is never edited: a changed file is refused.
//!
//! Each migration runs inside a transaction, so it cannot hold statements PostgreSQL refuses
//! there (`CREATE INDEX CONCURRENTLY`, `VACUUM`).
use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};
use std::fmt;
use tokio_postgres::Client;

static EMBEDDED: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// The advisory lock every runner takes ("vycore" in ASCII).
const LOCK_KEY: i64 = 0x7679_636f_7265;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: i32,
    pub name: String,
    pub sql: String,
}

impl Migration {
    pub fn sha256(&self) -> String {
        Sha256::digest(self.sql.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Debug)]
pub enum Error {
    /// A file name is not `NNNN_name.sql`, or the versions are not 1, 2, 3, …
    Set(String),
    /// An applied migration's file differs from what the ledger recorded.
    Changed { version: i32, name: String, recorded: String, found: String },
    /// The database holds a migration this binary does not have (an older binary on a newer database).
    Unknown { version: i32, name: String },
    /// A migration failed; its transaction was rolled back and nothing after it ran.
    Failed { version: i32, name: String, source: tokio_postgres::Error },
    Db(tokio_postgres::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Set(e) => write!(f, "migration set: {e}"),
            Error::Changed { version, name, recorded, found } => {
                write!(f, "migration {version:04}_{name} was applied as {recorded} but the file is now {found}")
            }
            Error::Unknown { version, name } => write!(f, "the database holds migration {version:04}_{name}, which this binary does not have"),
            Error::Failed { version, name, source } => write!(f, "migration {version:04}_{name} failed: {}", message(source)),
            Error::Db(e) => write!(f, "postgres: {}", message(e)),
        }
    }
}

impl std::error::Error for Error {}

impl From<tokio_postgres::Error> for Error {
    fn from(e: tokio_postgres::Error) -> Error {
        Error::Db(e)
    }
}

fn message(e: &tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// What a run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Migrations this run applied, in order.
    pub applied: Vec<(i32, String)>,
    /// Migrations already in the ledger and unchanged.
    pub unchanged: usize,
}

/// Parses `NNNN_name.sql` files into a checked set: versions 1, 2, 3, … in order.
pub fn parse(files: impl IntoIterator<Item = (String, String)>) -> Result<Vec<Migration>, Error> {
    let mut out = Vec::new();
    for (file, sql) in files {
        let stem = file.strip_suffix(".sql").ok_or_else(|| Error::Set(format!("{file}: not a .sql file")))?;
        let (number, name) = stem.split_once('_').ok_or_else(|| Error::Set(format!("{file}: expected NNNN_name.sql")))?;
        if number.len() != 4 || name.is_empty() {
            return Err(Error::Set(format!("{file}: expected NNNN_name.sql")));
        }
        let version: i32 = number.parse().map_err(|_| Error::Set(format!("{file}: expected NNNN_name.sql")))?;
        out.push(Migration { version, name: name.to_owned(), sql });
    }
    out.sort_by_key(|m| m.version);
    for (i, m) in out.iter().enumerate() {
        if m.version != i as i32 + 1 {
            return Err(Error::Set(format!("expected version {:04}, found {:04}_{}", i + 1, m.version, m.name)));
        }
    }
    Ok(out)
}

/// The migrations compiled into this binary.
pub fn embedded() -> Result<Vec<Migration>, Error> {
    parse(EMBEDDED.files().map(|f| {
        let name = f.path().file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        (name, String::from_utf8_lossy(f.contents()).into_owned())
    }))
}

/// Brings the database to `migrations`, under the advisory lock.
pub async fn apply(client: &mut Client, migrations: &[Migration]) -> Result<Report, Error> {
    client.execute("SELECT pg_advisory_lock($1)", &[&LOCK_KEY]).await?;
    let result = apply_locked(client, migrations).await;
    // the lock also ends with the session; a failed unlock must not hide the run's result
    let _ = client.execute("SELECT pg_advisory_unlock($1)", &[&LOCK_KEY]).await;
    result
}

async fn apply_locked(client: &mut Client, migrations: &[Migration]) -> Result<Report, Error> {
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS core;
             CREATE TABLE IF NOT EXISTS core.schema_migration (
                 version    integer     PRIMARY KEY,
                 name       text        NOT NULL,
                 sha256     text        NOT NULL,
                 applied_at timestamptz NOT NULL DEFAULT now()
             );",
        )
        .await?;
    let ledger = client.query("SELECT version, name, sha256 FROM core.schema_migration ORDER BY version", &[]).await?;
    for row in &ledger {
        let (version, name, recorded): (i32, String, String) = (row.get(0), row.get(1), row.get(2));
        match migrations.iter().find(|m| m.version == version) {
            None => return Err(Error::Unknown { version, name }),
            Some(m) if m.sha256() != recorded || m.name != name => {
                return Err(Error::Changed { version, name, recorded, found: m.sha256() });
            }
            Some(_) => {}
        }
    }
    let mut report = Report { applied: Vec::new(), unchanged: ledger.len() };
    for m in migrations.iter().filter(|m| m.version as usize > ledger.len()) {
        let failed = |source| Error::Failed { version: m.version, name: m.name.clone(), source };
        let tx = client.transaction().await?;
        tx.batch_execute(&m.sql).await.map_err(failed)?;
        tx.execute("INSERT INTO core.schema_migration (version, name, sha256) VALUES ($1, $2, $3)", &[&m.version, &m.name, &m.sha256()])
            .await
            .map_err(failed)?;
        tx.commit().await.map_err(failed)?;
        report.applied.push((m.version, m.name.clone()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_set_is_well_formed() {
        let set = embedded().unwrap();
        assert!(!set.is_empty());
        assert_eq!(set[0].version, 1);
    }

    #[test]
    fn file_names_and_numbering_are_checked() {
        let f = |n: &str| (n.to_owned(), "SELECT 1".to_owned());
        assert_eq!(parse([f("0002_b.sql"), f("0001_a.sql")]).unwrap().iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        for bad in [vec![f("0001_a.sql"), f("0003_c.sql")], vec![f("0002_b.sql")], vec![f("0001_a.sql"), f("0001_b.sql")], vec![f("1_a.sql")], vec![f("0001_a.txt")], vec![f("0001.sql")]] {
            assert!(matches!(parse(bad), Err(Error::Set(_))));
        }
    }
}
