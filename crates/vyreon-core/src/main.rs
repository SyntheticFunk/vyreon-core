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
//!   vyreon-core queue [pause <reason> | resume | retry-failed | cancel <job>...]   the queue at a glance, or change it
//!   vyreon-core months <first> <last>                 create the store's monthly partitions for a date range
//!   vyreon-core publish [--vouch] <bundle>...         check, write, read back and publish day bundles, in order.
//!                                                     A revision computed from a base must name the base revision
//!                                                     it read and read its day only; --vouch is the operator's word
//!                                                     that the bundles were computed from what is published now
//!                                                     (C-042: a bundle that names no base revision, or reads history)
//!   vyreon-core evidence list [<job>]                 the failure evidence workers left (C-049), newest first
//!   vyreon-core evidence show <job> [--attempt N]     one attempt's evidence in full (the latest by default):
//!                                                     its stop message, the input plan's refusals and its log excerpt
//!   vyreon-core archive list [<date>]                 the perishable archive (C-052): each object requested, kept or
//!                                                     failed (the newest sessions, or one session), and what is missing
//!   vyreon-core problems                              the open problems (C-050), oldest first, and the last resolved
//!   vyreon-core digest [weekly]                       print the 09:00 digest (or the Saturday check) as it would be posted now
//!   vyreon-core log [n]                               the newest n (20) logged events, which are not posted
//!   vyreon-core daily [queue]                         the daily runs (C-051): their settings, the sessions
//!                                                     handled and the next due; `queue` queues what is due now
//!                                                     (the service does it every minute by itself)
//!   vyreon-core notices mark-sent                     mark every post not sent yet as sent, without posting it (run once
//!                                                     before the webhook is first switched on, so the channel starts clean)
//!   vyreon-core api-key create <holder> <read[,admin]>   make a reader API key (printed once; only its digest is kept)
//!   vyreon-core api-key revoke <holder>
//!   vyreon-core serve                                 the HTTP service (the worker and reader APIs). Settings:
//!                                                     VYREON_CORE_WORKER_TOKEN_SHA256 (required: the SHA-256 of the
//!                                                     workers' shared token), VYREON_CORE_LISTEN (0.0.0.0:8080),
//!                                                     VYREON_CORE_SPOOL (the temp dir), VYREON_CORE_LEASE_SECONDS (300),
//!                                                     VYREON_CORE_INTRINIO_PER_MINUTE (1800), VYREON_CORE_WEBHOOK_FILE
//!                                                     (a file holding the Discord webhook URL; without it the posts wait).
//!                                                     The watch (C-050), also read by `problems` and `digest`:
//!                                                     VYREON_CORE_WATCH_PROFILE (FREE_REPORT_64), VYREON_CORE_DEADLINES_FROM
//!                                                     (the first session with a publishing deadline; unset: the daily
//!                                                     runs' first session, else none), VYREON_CORE_DEADLINE_HOUR (20:
//!                                                     the deadline's hour, MT, the next calendar day), VYREON_CORE_VOLUME_GIB
//!                                                     (the database volume's size; unset: not checked).
//!                                                     Daily runs (C-051), both or neither: VYREON_CORE_DAILY_FROM (the first
//!                                                     session core queues by itself; unset: none) and VYREON_CORE_DAILY_BUILD
//!                                                     (the admitted engine build they are queued for).
//!                                                     The perishable archive (C-052), all five or none (off):
//!                                                     VYREON_CORE_SPACES_ENDPOINT (https://nyc3.digitaloceanspaces.com),
//!                                                     VYREON_CORE_SPACES_REGION (nyc3), VYREON_CORE_SPACES_BUCKET,
//!                                                     VYREON_CORE_SPACES_KEY_ID_FILE and VYREON_CORE_SPACES_SECRET_FILE
//!                                                     (files holding the bucket's access key; read, never printed; absent:
//!                                                     the archive is off). VYREON_CORE_ARCHIVE_FROM (the first session whose
//!                                                     published day must have its archives; unset: the daily runs' first
//!                                                     session) and VYREON_CORE_ARCHIVE_ALERT_DAYS (a missing archive older
//!                                                     than this is a problem; unset: only counted in the digest)
//!   vyreon-core engine-store export <date> <dir>      bundles and inventories of a day from a store in
//!                                                     the engine's schema (VYREON_ENGINE_PG[_FILE]; read-only)
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
mod engine_store;
use core_db::{archive, build, daily, evidence, migrate, notice, publish, universe, watch, work};
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
    let mut client = connect().await?;
    match change {
        [] => {}
        // the queue's change and its notice are one transaction (C-027's outbox)
        ["pause", reason] => {
            let tx = client.transaction().await.map_err(|e| e.to_string())?;
            work::pause(&tx, reason).await?;
            tx.commit().await.map_err(|e| e.to_string())?
        }
        ["resume"] => {
            let tx = client.transaction().await.map_err(|e| e.to_string())?;
            let was = work::resume(&tx).await?;
            tx.commit().await.map_err(|e| e.to_string())?;
            println!("was paused for: {}", was.as_deref().unwrap_or("(it was not paused)"))
        }
        ["retry-failed"] => println!("{} failed jobs queued again", work::retry_failed(&client).await?),
        ["cancel", jobs @ ..] if !jobs.is_empty() => {
            let jobs: Vec<i64> = jobs.iter().map(|j| j.parse().map_err(|_| format!("job id: {j}"))).collect::<Result<_, _>>()?;
            println!("{} jobs cancelled", work::cancel(&mut client, &jobs).await?)
        }
        _ => return Err("queue [pause <reason> | resume | retry-failed | cancel <job>...]".into()),
    }
    work::sweep(&mut client).await?;
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

/// The bundles and whether they are vouched for, from what follows `publish`. The whole command
/// line is read before anything is published: `--vouch` counts wherever it stands, and any other
/// word that starts with `--` is refused, not taken as a bundle after the ones before it were
/// published (a file so named is written `./--name`).
fn publish_args<'a>(rest: &[&'a str]) -> Result<(Vec<&'a str>, bool), String> {
    if let Some(flag) = rest.iter().find(|a| a.starts_with("--") && **a != "--vouch") {
        return Err(format!("publish: unknown option {flag}; nothing was published (publish [--vouch] <bundle>...)"));
    }
    let bundles: Vec<&str> = rest.iter().copied().filter(|a| *a != "--vouch").collect();
    if bundles.is_empty() {
        return Err("publish: no bundle named; nothing was published (publish [--vouch] <bundle>...)".to_string());
    }
    let vouched = bundles.len() != rest.len();
    Ok((bundles, vouched))
}

async fn publish_bundles(bundles: &[&str], vouched: bool) -> Result<(), String> {
    let mut client = connect().await?;
    for bundle in bundles {
        let path = Path::new(bundle);
        let p = if vouched { publish::publish_file_vouched(&mut client, path).await } else { publish::publish_file(&mut client, path).await };
        let p = p.map_err(|e| format!("{bundle}: {e}"))?;
        if p.already_published {
            println!("{} {} was already published as revision {}", p.represented_date, p.profile_revision, p.revision_id);
            continue;
        }
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

/// A setting that is set and not blank.
fn set(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn date_setting(name: &str) -> Result<Option<String>, String> {
    match set(name) {
        Some(d) if !core_contract::is_date(&d) => Err(format!("{name}: {d} is not a date written YYYY-MM-DD")),
        d => Ok(d),
    }
}

/// The watch's settings (C-050) and the daily runs' (C-051).
fn watch_settings() -> Result<watch::Settings, String> {
    let profile = setting("VYREON_CORE_WATCH_PROFILE", "FREE_REPORT_64".to_string())?;
    let daily = match (date_setting("VYREON_CORE_DAILY_FROM")?, set("VYREON_CORE_DAILY_BUILD")) {
        (Some(from), Some(build)) => Some(daily::Daily { profile: profile.clone(), build, from }),
        (None, None) => None,
        _ => return Err("VYREON_CORE_DAILY_FROM and VYREON_CORE_DAILY_BUILD are set together or not at all".into()),
    };
    // a deadline from the first session core queues by itself, unless one is set
    let deadlines_from = date_setting("VYREON_CORE_DEADLINES_FROM")?.or_else(|| daily.as_ref().map(|d| d.from.clone()));
    let deadline_hour: i32 = setting("VYREON_CORE_DEADLINE_HOUR", watch::DEADLINE_HOUR)?;
    if !(0..=21).contains(&deadline_hour) {
        return Err(format!("VYREON_CORE_DEADLINE_HOUR: {deadline_hour} is not an hour from 0 to 21"));
    }
    let volume: Option<f64> = match std::env::var("VYREON_CORE_VOLUME_GIB") {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().parse().map_err(|_| format!("VYREON_CORE_VOLUME_GIB: cannot read {v}"))?),
        _ => None,
    };
    // C-052: the archive is watched when its bucket is set
    let archive = match set("VYREON_CORE_SPACES_BUCKET") {
        None => None,
        Some(_) => {
            let alert_days = match set("VYREON_CORE_ARCHIVE_ALERT_DAYS") {
                None => None,
                Some(v) => Some(v.parse::<i32>().ok().filter(|d| *d >= 1).ok_or_else(|| format!("VYREON_CORE_ARCHIVE_ALERT_DAYS: {v} is not a whole number of days from 1"))?),
            };
            let expected_from = date_setting("VYREON_CORE_ARCHIVE_FROM")?.or_else(|| daily.as_ref().map(|d| d.from.clone()));
            Some(watch::ArchiveWatch { expected_from, alert_days })
        }
    };
    Ok(watch::Settings { profile, deadlines_from, volume_bytes: volume.map(|g| (g * (1u64 << 30) as f64) as i64), deadline_hour, daily, archive })
}

/// The perishable archive's bucket and key (C-052): all five settings or none. The key's files
/// may be absent (the Secret not created yet): the archive is then off. Never printed.
fn spaces_settings() -> Result<Option<archive::Spaces>, String> {
    let names = ["VYREON_CORE_SPACES_ENDPOINT", "VYREON_CORE_SPACES_REGION", "VYREON_CORE_SPACES_BUCKET", "VYREON_CORE_SPACES_KEY_ID_FILE", "VYREON_CORE_SPACES_SECRET_FILE"];
    let values: Vec<Option<String>> = names.iter().map(|n| set(n)).collect();
    if values.iter().all(Option::is_none) {
        println!("the perishable archive is off: VYREON_CORE_SPACES_* are not set");
        return Ok(None);
    }
    if values.iter().any(Option::is_none) {
        return Err(format!("set all or none of {}", names.join(", ")));
    }
    let v: Vec<String> = values.into_iter().flatten().collect();
    let read = |file: &str| std::fs::read_to_string(file).ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let (Some(key_id), Some(secret)) = (read(&v[3]), read(&v[4])) else {
        println!("the Spaces key's files are absent or empty: the perishable archive is off");
        return Ok(None);
    };
    let spaces = archive::Spaces::new(&v[0], &v[1], &v[2], &key_id, &secret)?;
    println!("the perishable archive goes to {spaces:?}");
    Ok(Some(spaces))
}

/// `archive list [<date>]`: one line per archive object, newest session first, then what is missing.
async fn archive_list(session: Option<&str>) -> Result<(), String> {
    let client = connect().await?;
    let rows = archive::list(&client, session, 200).await?;
    if rows.is_empty() {
        println!("no perishable archive{}", session.map_or(String::new(), |s| format!(" for {s}")));
    }
    for r in &rows {
        let text = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
        let state = match text("state").as_str() {
            "KEPT" => format!("KEPT {}", text("kept_at")),
            "FAILED" => format!("FAILED: {}", text("failure")),
            other => format!("{other} {} (not kept yet)", text("requested_at")),
        };
        println!("{} {:<12} {} bytes {}/{} job {} by {}: {state}", text("session"), text("category"), r["bytes"], text("bucket"), text("key"), r["job_id"], text("worker"));
    }
    let s = watch_settings()?;
    let now = watch::now(&client).await?;
    let expected = s.archive.as_ref().and_then(|a| a.expected_from.clone());
    let missing = archive::missing(&client, &s.profile, expected.as_deref(), &now).await?;
    println!(
        "missing: {}{}",
        missing.len(),
        expected.map_or(" (only requests never kept are counted: VYREON_CORE_ARCHIVE_FROM is not set)".into(), |f| format!(" (every published day from {f} is expected to have its archives)"))
    );
    for m in missing.iter().filter(|m| session.is_none_or(|s| s == m.session)) {
        println!("  {} {} for {}", m.session, m.category, watch::age(m.age));
    }
    Ok(())
}

/// `daily [queue]`: the daily runs' settings, the sessions handled, and (with `queue`) what is due
/// now queued, as the service does every minute.
async fn daily_runs(queue: bool) -> Result<(), String> {
    let s = watch_settings()?;
    let Some(d) = &s.daily else {
        println!("daily runs are off: VYREON_CORE_DAILY_FROM and VYREON_CORE_DAILY_BUILD are not set");
        return Ok(());
    };
    let mut client = connect().await?;
    let now = watch::now(&client).await?;
    println!(
        "daily runs: profile {}, build {}, from {}; a session is due at {} {} the next calendar day; deadline {:02}:00 MT the next calendar day",
        d.profile,
        d.build,
        d.from,
        daily::READY_AT,
        daily::READY_ZONE,
        s.deadline_hour
    );
    if let Some(why) = daily::build_stopped(&client, d).await? {
        println!("the build {why}: nothing is queued until it is admitted");
    }
    if queue {
        for q in daily::queue_due(&mut client, d, &now).await? {
            println!("queued {}: jobs {:?}", q.session, q.jobs);
        }
    } else {
        let due = daily::due(&client, d, &now).await?;
        println!("due now and not queued yet: {}", if due.is_empty() { "none".to_string() } else { due.join(" ") });
    }
    let rows = client
        .query(
            "SELECT session::text, engine_build, to_char(queued_at AT TIME ZONE 'America/Edmonton', 'Dy Mon FMDD HH24:MI'), jobs::text
             FROM core.daily_session WHERE profile_id = $1 ORDER BY session DESC LIMIT 10",
            &[&d.profile],
        )
        .await
        .map_err(|e| e.to_string())?;
    for r in rows.iter().rev() {
        println!("{} queued {} MT for {}: jobs {}", r.get::<_, String>(0), r.get::<_, String>(2), r.get::<_, String>(1), r.get::<_, String>(3));
    }
    println!("{}", daily::digest_line(&client, Some(d), &now, watch::ZONE).await?);
    Ok(())
}

/// `problems`: what is open, oldest first, and the ten last resolved.
async fn problems() -> Result<(), String> {
    let client = connect().await?;
    let now = watch::now(&client).await?;
    let open = watch::open(&client, &now).await?;
    if open.is_empty() {
        println!("no open problem");
    }
    for o in &open {
        println!("open {}: {} · `{}`", watch::age(o.age), o.summary, o.command);
    }
    let rows = client
        .query(
            "SELECT to_char(resolved_at AT TIME ZONE 'America/Edmonton', 'Dy Mon FMDD HH24:MI'), summary FROM core.problem
             WHERE resolved_at IS NOT NULL ORDER BY resolved_at DESC, problem_id DESC LIMIT 10",
            &[],
        )
        .await
        .map_err(|e| e.to_string())?;
    for r in &rows {
        println!("resolved {} MT: {}", r.get::<_, String>(0), r.get::<_, String>(1));
    }
    Ok(())
}

async fn digest(weekly: bool) -> Result<(), String> {
    let client = connect().await?;
    let now = watch::now(&client).await?;
    let text = if weekly { watch::weekly(&client, &watch_settings()?, &now).await? } else { watch::digest(&client, &watch_settings()?, &now).await? };
    println!("{text}");
    Ok(())
}

async fn log(n: &str) -> Result<(), String> {
    let n: i64 = n.parse().map_err(|_| format!("how many: {n}"))?;
    for (at, message) in notice::recent_log(&connect().await?, n).await?.iter().rev() {
        println!("{at} {message}");
    }
    Ok(())
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
        archive: spaces_settings()?,
    };
    let pg: tokio_postgres::Config = conninfo("VYREON_CORE_PG")?.parse().map_err(|_| "VYREON_CORE_PG: not a connection string".to_string())?;
    let manager = deadpool_postgres::Manager::new(pg, NoTls);
    let pool = deadpool_postgres::Pool::builder(manager).max_size(16).build().map_err(|e| e.to_string())?;
    let app = core_api::App::new(pool, config);
    let listen: String = setting("VYREON_CORE_LISTEN", "0.0.0.0:8080".to_string())?;
    let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| format!("{listen}: {e}"))?;
    core_api::spawn_sweeper(app.clone());
    // the webhook's file may be absent (the Secret not created yet): the posts then wait unsent
    let webhook = match std::env::var("VYREON_CORE_WEBHOOK_FILE") {
        Ok(file) => match std::fs::read_to_string(&file) {
            Ok(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
            _ => {
                println!("{file} is absent or empty: the channel's posts are kept unsent");
                None
            }
        },
        Err(_) => {
            println!("VYREON_CORE_WEBHOOK_FILE is not set: the channel's posts are kept unsent");
            None
        }
    };
    core_api::notices::spawn_watch(app.clone(), watch_settings()?, webhook);
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

/// `evidence list [<job>]`: one line per attempt, newest first; the message's first line only.
async fn evidence_list(job: Option<&str>) -> Result<(), String> {
    let job = job.map(|j| j.parse::<i64>().map_err(|_| format!("job id: {j}"))).transpose()?;
    let rows = evidence::list(&connect().await?, job, 200).await?;
    if rows.is_empty() {
        println!("no failure evidence{}", job.map_or(String::new(), |j| format!(" for job {j}")));
    }
    for r in &rows {
        let message = r["message"].as_str().unwrap_or_default();
        let refusals = r["refusals"].as_i64().map_or(String::new(), |n| format!(", {n} refusals"));
        println!(
            "{} job {} attempt {} {} {} by {}: {}: {} ({} bytes of log{refusals})",
            r["stored_at"].as_str().unwrap_or_default(),
            r["job_id"],
            r["attempt"],
            r["kind"].as_str().unwrap_or_default(),
            r["session"].as_str().unwrap_or("before its first session"),
            r["worker"].as_str().unwrap_or_default(),
            r["stage"].as_str().unwrap_or_default(),
            message.lines().next().unwrap_or_default(),
            r["excerpt_bytes"],
        );
    }
    Ok(())
}

/// `evidence show <job> [--attempt N]`: the record, the refusals, then the excerpt as sent.
async fn evidence_show(job: &str, attempt: Option<&str>) -> Result<(), String> {
    let job: i64 = job.parse().map_err(|_| format!("job id: {job}"))?;
    let attempt = attempt.map(|a| a.parse::<i32>().map_err(|_| format!("attempt: {a}"))).transpose()?;
    let Some((record, excerpt)) = evidence::show(&connect().await?, job, attempt).await? else {
        return Err(format!("no failure evidence for job {job}{}", attempt.map_or(String::new(), |a| format!(" attempt {a}"))));
    };
    let mut head = record.clone();
    if let Some(o) = head.as_object_mut() {
        o.remove("message");
        o.remove("refusals");
    }
    println!("{}", serde_json::to_string_pretty(&head).map_err(|e| e.to_string())?);
    println!("== message\n{}", record["message"].as_str().unwrap_or_default());
    if !record["refusals"].is_null() {
        println!("== refusals\n{}", serde_json::to_string_pretty(&record["refusals"]).map_err(|e| e.to_string())?);
    }
    print!("== log excerpt\n{excerpt}");
    if !excerpt.ends_with('\n') {
        println!();
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
        ["evidence", "list"] => evidence_list(None).await,
        ["evidence", "list", job] => evidence_list(Some(job)).await,
        ["evidence", "show", job] => evidence_show(job, None).await,
        ["evidence", "show", job, "--attempt", attempt] => evidence_show(job, Some(attempt)).await,
        ["archive", "list"] => archive_list(None).await,
        ["archive", "list", date] => archive_list(Some(date)).await,
        ["problems"] => problems().await,
        ["digest"] => digest(false).await,
        ["digest", "weekly"] => digest(true).await,
        ["daily"] => daily_runs(false).await,
        ["daily", "queue"] => daily_runs(true).await,
        ["log"] => log("20").await,
        ["log", n] => log(n).await,
        ["notices", "mark-sent"] => match connect().await {
            Ok(client) => notice::mark_all_sent(&client).await.map(|n| println!("{n} notices marked sent without posting")),
            Err(e) => Err(e),
        },
        ["serve"] => serve().await,
        ["months", first, last] => months(first, last).await,
        ["publish", ref rest @ ..] => match publish_args(rest) {
            Ok((bundles, vouched)) => publish_bundles(&bundles, vouched).await,
            Err(e) => Err(e),
        },
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

#[cfg(test)]
mod tests {
    use super::publish_args;

    /// Fix round C (corepublish.md, noticed by the verifier): `publish a --vouch b` published `a`
    /// unvouched and then failed opening a file named `--vouch`. The command line is now read
    /// whole first: the flag counts wherever it stands, and a bad line names no bundle to publish.
    #[test]
    fn the_publish_command_line_is_read_whole_before_anything_is_published() {
        assert_eq!(publish_args(&["a", "b"]), Ok((vec!["a", "b"], false)));
        assert_eq!(publish_args(&["--vouch", "a", "b"]), Ok((vec!["a", "b"], true)));
        assert_eq!(publish_args(&["a", "--vouch", "b"]), Ok((vec!["a", "b"], true)));
        assert_eq!(publish_args(&["a", "b", "--vouch"]), Ok((vec!["a", "b"], true)));
        assert_eq!(publish_args(&["./--vouch"]), Ok((vec!["./--vouch"], false)));
        for bad in [&[][..], &["--vouch"], &["a", "--vuoch", "b"], &["a", "--vouch=yes"]] {
            let refused = publish_args(bad).unwrap_err();
            assert!(refused.contains("nothing was published"), "{refused}");
        }
    }
}
