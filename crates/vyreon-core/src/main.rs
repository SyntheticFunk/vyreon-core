//! vyreon-core: the authoritative database and orchestrator around the Vyreon engine.
//!
//!   vyreon-core migrate                               bring the database to this binary's migrations
//!   vyreon-core universe load <PROFILE.json> <dir>    store a profile's members and their ticker
//!                                                     history from saved Intrinio responses
//!   vyreon-core universe on <profile> <date>          the members and their tickers on a date
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
use core_db::{migrate, universe};
use std::path::Path;
use tokio_postgres::Client;
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

async fn connect() -> Result<Client, String> {
    // TLS: the database is reached inside the cluster's private network; add TLS before any
    // connection leaves it.
    let (client, connection) = tokio_postgres::connect(&conninfo()?, NoTls).await.map_err(|e| format!("postgres connect: {e}"))?;
    tokio::spawn(connection);
    Ok(client)
}

async fn universe_load(profile: &str, dir: &str) -> Result<(), String> {
    let doc = universe::read_responses(Path::new(profile), Path::new(dir))?;
    let loaded = universe::store(&mut connect().await?, &doc).await?;
    println!("{} members, {} provider records, {} ticker spans", loaded.members, loaded.securities, loaded.spans);
    Ok(())
}

async fn universe_on(profile: &str, date: &str) -> Result<(), String> {
    let client = connect().await?;
    let rows = client
        .query("SELECT ordinal, asset_key, security_id, ticker, group_name, company FROM core.members_on($1, $2::text::date)", &[&profile, &date])
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err(format!("no members for profile {profile}"));
    }
    for r in &rows {
        let (ordinal, key, id, ticker, group, company): (i32, String, String, Option<String>, String, bool) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        println!("{ordinal:>3} {key:<6} {:<10} {id:<11} {group}{}", ticker.as_deref().unwrap_or("not listed"), if company { " company" } else { "" });
    }
    Ok(())
}

async fn run_migrate() -> Result<(), String> {
    let mut client = connect().await?;
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args[..] {
        ["migrate"] => run_migrate().await,
        ["universe", "load", profile, dir] => universe_load(profile, dir).await,
        ["universe", "on", profile, date] => universe_on(profile, date).await,
        _ => Err("usage: vyreon-core migrate | universe load <PROFILE.json> <dir> | universe on <profile> <date>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vyreon-core: {e}");
            ExitCode::FAILURE
        }
    }
}
