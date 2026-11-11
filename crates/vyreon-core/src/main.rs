//! vyreon-core: the authoritative database and orchestrator around the Vyreon engine.
//!
//!   vyreon-core migrate                               bring the database to this binary's migrations
//!   vyreon-core universe load <PROFILE.json> <dir>    store a profile's members and their ticker
//!                                                     history from saved Intrinio responses
//!   vyreon-core universe on <profile> <date>          the members and their tickers on a date
//!   vyreon-core build register <inventory.json>       register an engine build with its slot inventory
//!   vyreon-core build admit <build> <evidence.json>   let the build's workers claim work (the checks it passed)
//!   vyreon-core build withdraw <build> <reason>       stop them claiming
//!   vyreon-core calendar load <sessions.json>         store the engine's trading sessions
//!   vyreon-core schedule <tape|day-block|ordered-pass> <profile> <build> <first> <last> [block sessions]
//!   vyreon-core queue [pause <reason> | resume | retry-failed]   the queue at a glance, or change it
//!   vyreon-core months <first> <last>                 create the store's monthly partitions for a date range
//!   vyreon-core publish <bundle>...                   check, write, read back and publish day bundles, in order
//!   vyreon-core api-key create <holder> <read[,admin]>   make a reader API key (printed once; only its digest is kept)
//!   vyreon-core api-key revoke <holder>
//!   vyreon-core serve                                 the HTTP service (the worker and reader APIs). Settings:
//!                                                     VYREON_CORE_WORKER_TOKEN_SHA256 (required: the SHA-256 of the
//!                                                     workers' shared token), VYREON_CORE_LISTEN (0.0.0.0:8080),
//!                                                     VYREON_CORE_SPOOL (the temp dir), VYREON_CORE_LEASE_SECONDS (300),
//!                                                     VYREON_CORE_INTRINIO_PER_MINUTE (1800), VYREON_CORE_WEBHOOK_FILE
//!                                                     (a file holding the Discord webhook URL; without it notices wait)
//!   vyreon-core engine-store export <date> <dir>      bundles and inventories of a day from a store in
//!                                                     the engine's schema (VYREON_ENGINE_PG[_FILE]; read-only)
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
mod engine_store;
use core_db::{build, migrate, publish, universe, work};
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

fn json_file(file: &str) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
}

async fn schedule(kind: &str, profile: &str, engine_build: &str, first: &str, last: &str, block: &str) -> Result<(), String> {
    let block: i32 = block.parse().map_err(|_| format!("block sessions: {block}"))?;
    let made = work::schedule(&mut connect().await?, work::Kind::parse(kind)?, profile, engine_build, first, last, block).await?;
    println!("{} jobs over {} sessions; {} partitions created", made.jobs, made.sessions, made.partitions);
    Ok(())
}

async fn queue(change: &[&str]) -> Result<(), String> {
    let client = connect().await?;
    match change {
        [] => {}
        ["pause", reason] => work::pause(&client, reason).await?,
        ["resume"] => println!("was paused for: {}", work::resume(&client).await?.as_deref().unwrap_or("(it was not paused)")),
        ["retry-failed"] => println!("{} failed jobs queued again", work::retry_failed(&client).await?),
        _ => return Err("queue [pause <reason> | resume | retry-failed]".into()),
    }
    work::sweep(&client).await?;
    println!("{}", serde_json::to_string_pretty(&work::status(&client).await?).map_err(|e| e.to_string())?);
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

fn setting<T: std::str::FromStr>(name: &str, default: T) -> Result<T, String> {
    match std::env::var(name) {
        Ok(v) => v.parse().map_err(|_| format!("{name}: cannot read {v}")),
        Err(_) => Ok(default),
    }
}

async fn serve() -> Result<(), String> {
    let digest = std::env::var("VYREON_CORE_WORKER_TOKEN_SHA256").map_err(|_| "set VYREON_CORE_WORKER_TOKEN_SHA256".to_string())?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("VYREON_CORE_WORKER_TOKEN_SHA256 is not a SHA-256 in hex".into());
    }
    let mut budget = core_db::budget::Budget::default();
    budget.per_minute = setting("VYREON_CORE_INTRINIO_PER_MINUTE", budget.per_minute)?;
    let config = core_api::Config {
        worker_token_sha256: digest.to_ascii_lowercase(),
        spool: std::env::var_os("VYREON_CORE_SPOOL").map_or_else(std::env::temp_dir, Into::into),
        lease_seconds: setting("VYREON_CORE_LEASE_SECONDS", 300.0)?,
        budget,
        max_bundle_bytes: 2 << 30,
    };
    let pg: tokio_postgres::Config = conninfo("VYREON_CORE_PG")?.parse().map_err(|_| "VYREON_CORE_PG: not a connection string".to_string())?;
    let manager = deadpool_postgres::Manager::new(pg, NoTls);
    let pool = deadpool_postgres::Pool::builder(manager).max_size(16).build().map_err(|e| e.to_string())?;
    let app = core_api::App::new(pool, config);
    let listen: String = setting("VYREON_CORE_LISTEN", "0.0.0.0:8080".to_string())?;
    let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| format!("{listen}: {e}"))?;
    core_api::spawn_sweeper(app.clone());
    match std::env::var("VYREON_CORE_WEBHOOK_FILE") {
        Ok(file) => {
            let webhook = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?.trim().to_owned();
            core_api::notices::spawn_sender(app.clone(), webhook);
        }
        Err(_) => println!("VYREON_CORE_WEBHOOK_FILE is not set: notices are kept unsent"),
    }
    println!("vyreon-core listening on {listen}");
    axum::serve(listener, core_api::router(app)).await.map_err(|e| e.to_string())
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
        ["build", "admit", engine_build, evidence] => match (connect().await, json_file(evidence)) {
            (Ok(client), Ok(evidence)) => build::admit(&client, engine_build, &evidence).await,
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        ["build", "withdraw", engine_build, reason] => match connect().await {
            Ok(client) => build::withdraw(&client, engine_build, reason).await,
            Err(e) => Err(e),
        },
        ["calendar", "load", file] => match (connect().await, json_file(file)) {
            (Ok(mut client), Ok(calendar)) => work::load_calendar(&mut client, &calendar).await.map(|n| println!("{n} sessions")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        ["schedule", kind, profile, engine_build, first, last] => schedule(kind, profile, engine_build, first, last, "30").await,
        ["schedule", kind, profile, engine_build, first, last, block] => schedule(kind, profile, engine_build, first, last, block).await,
        ["queue", ref change @ ..] => queue(change).await,
        ["api-key", "create", holder, scopes] => match connect().await {
            Ok(client) => core_db::api_key::create(&client, holder, &scopes.split(',').map(str::to_owned).collect::<Vec<_>>()).await.map(|key| println!("{key}")),
            Err(e) => Err(e),
        },
        ["api-key", "revoke", holder] => match connect().await {
            Ok(client) => core_db::api_key::revoke(&client, holder).await,
            Err(e) => Err(e),
        },
        ["serve"] => serve().await,
        ["months", first, last] => months(first, last).await,
        ["publish", ref bundles @ ..] if !bundles.is_empty() => publish_bundles(bundles).await,
        ["engine-store", "export", date, dir] => engine_store_export(date, dir).await,
        _ => Err("unknown command; the commands are listed at the top of crates/vyreon-core/src/main.rs".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vyreon-core: {e}");
            ExitCode::FAILURE
        }
    }
}
