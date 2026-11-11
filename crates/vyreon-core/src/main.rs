//! vyreon-core: the authoritative database and orchestrator around the Vyreon engine.
//!
//!   vyreon-core migrate                               bring the database to this binary's migrations
//!   vyreon-core universe load <PROFILE.json> <dir>    store a profile's members and their ticker
//!                                                     history from saved Intrinio responses
//!   vyreon-core universe on <profile> <date>          the members and their tickers on a date
//!   vyreon-core build register <inventory.json>       register an engine build with its slot inventory
//!   vyreon-core months <first> <last>                 create the store's monthly partitions for a date range
//!   vyreon-core publish <bundle>...                   check, write, read back and publish day bundles, in order
//!   vyreon-core engine-store export <date> <dir>      bundles and inventories of a day from a store in
//!                                                     the engine's schema (VYREON_ENGINE_PG[_FILE]; read-only)
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
mod engine_store;
use core_db::{build, migrate, publish, universe};
use std::path::Path;
use tokio_postgres::Client;
use std::process::ExitCode;
use tokio_postgres::NoTls;

/// The connection string in `<name>`, or in the file named by `<name>_FILE`.
fn conninfo(name: &str) -> Result<String, String> {
    if let Ok(v) = std::env::var(name) {
        return Ok(v);
    }
    let file = std::env::var(format!("{name}_FILE")).map_err(|_| format!("set {name} or {name}_FILE"))?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
    Ok(text.trim().to_owned())
}

async fn connect_to(name: &str) -> Result<Client, String> {
    // TLS: the database is reached inside the cluster's private network; add TLS before any
    // connection leaves it.
    let (client, connection) = tokio_postgres::connect(&conninfo(name)?, NoTls).await.map_err(|e| format!("postgres connect: {e}"))?;
    tokio::spawn(connection);
    Ok(client)
}

async fn connect() -> Result<Client, String> {
    connect_to("VYREON_CORE_PG").await
}

async fn build_register(file: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let inventory = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
    let r = build::register(&mut connect().await?, &inventory).await?;
    println!("{} {}: {} slots", if r.new { "registered" } else { "already registered" }, r.engine_build, r.slots);
    Ok(())
}

async fn months(first: &str, last: &str) -> Result<(), String> {
    let client = connect().await?;
    let created: i32 = client
        .query_one("SELECT vyreon.ensure_months($1::text::date, $2::text::date)", &[&first, &last])
        .await
        .map_err(|e| e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned()))?
        .get(0);
    println!("{created} partitions created");
    Ok(())
}

async fn publish_bundles(bundles: &[&str]) -> Result<(), String> {
    let mut client = connect().await?;
    for bundle in bundles {
        let p = publish::publish_file(&mut client, Path::new(bundle)).await.map_err(|e| format!("{bundle}: {e}"))?;
        println!(
            "published {} {} as revision {}: {} slots, {} streams ({} bytes), {} reader scalars, {} unusual scores, {} superseded",
            p.represented_date, p.profile_revision, p.revision_id, p.slots, p.streams, p.stream_bytes, p.reader_scalars, p.unusual_scores, p.superseded
        );
    }
    Ok(())
}

async fn engine_store_export(date: &str, dir: &str) -> Result<(), String> {
    let store = connect_to("VYREON_ENGINE_PG").await?;
    for file in engine_store::export_day(&store, date, Path::new(dir)).await? {
        println!("{file}");
    }
    Ok(())
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
        ["build", "register", file] => build_register(file).await,
        ["months", first, last] => months(first, last).await,
        ["publish", ref bundles @ ..] if !bundles.is_empty() => publish_bundles(bundles).await,
        ["engine-store", "export", date, dir] => engine_store_export(date, dir).await,
        _ => Err("usage: vyreon-core migrate | universe load <PROFILE.json> <dir> | universe on <profile> <date> | build register <inventory.json> \
                  | months <first> <last> | publish <bundle>... | engine-store export <date> <dir>"
            .to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vyreon-core: {e}");
            ExitCode::FAILURE
        }
    }
}
