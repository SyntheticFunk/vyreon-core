//! vyreon-core: the authoritative database and orchestrator around the Vyreon engine.
//!
//!   vyreon-core migrate     bring the database to this binary's migrations
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
use core_db::migrate;
use std::process::ExitCode;
use tokio_postgres::NoTls;

fn conninfo() -> Result<String, String> {
    if let Ok(v) = std::env::var("VYREON_CORE_PG") {
        return Ok(v);
    }
    let file = std::env::var("VYREON_CORE_PG_FILE").map_err(|_| "set VYREON_CORE_PG or VYREON_CORE_PG_FILE".to_string())?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
    Ok(text.trim().to_owned())
}

async fn run_migrate() -> Result<(), String> {
    // TLS: the database is reached inside the cluster's private network; add TLS before any
    // connection leaves it.
    let (mut client, connection) = tokio_postgres::connect(&conninfo()?, NoTls).await.map_err(|e| format!("postgres connect: {e}"))?;
    tokio::spawn(connection);
    let set = migrate::embedded().map_err(|e| e.to_string())?;
    let report = migrate::apply(&mut client, &set).await.map_err(|e| e.to_string())?;
    for (version, name) in &report.applied {
        println!("applied {version:04}_{name}");
    }
    println!("{} applied, {} unchanged", report.applied.len(), report.unchanged);
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        Some("migrate") => run_migrate().await,
        _ => Err("usage: vyreon-core migrate".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vyreon-core: {e}");
            ExitCode::FAILURE
        }
    }
}
