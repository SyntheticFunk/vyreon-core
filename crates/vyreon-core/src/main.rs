//! vyreon-core: the authoritative database and orchestrator around the Vyreon engine.
//!
//!   vyreon-core migrate                               bring the database to this binary's migrations
//!   vyreon-core universe load <PROFILE.json> <dir>    store a profile's members and their ticker
//!                                                     history from saved Intrinio responses
//!   vyreon-core universe on <profile> <date>          the members and their tickers on a date
//!   vyreon-core build register <inventory.json>       register an engine build with its slot inventory (`-`: stdin)
//!   vyreon-core build admit <build> <evidence.json>   let the build's workers claim work (the checks it passed; `-`: stdin)
//!   vyreon-core build withdraw <build> <reason>       stop them claiming, and stop its running jobs (their
//!                                                     leases end; the jobs are queued again under it)
//!   vyreon-core build hand-over (--preview | --expect <token>) <from> <to> <reason...>   C-068: hand a build's
//!                                                     unfinished tapes, day blocks and ordered passes to its successor,
//!                                                     registered and NOT yet admitted (admit it after), once; <from> is
//!                                                     withdrawn by it if it still runs: each job is cancelled and the same work
//!                                                     queued under <to> (a block from its next session; its done days
//!                                                     a DONE job of <from>). --preview does it in a transaction rolled
//!                                                     back and prints each job with a token; the hand-over itself
//!                                                     takes --expect <that token>
//!   vyreon-core calendar load <sessions.json>         store the engine's trading sessions
//!   vyreon-core schedule <tape|day-block|ordered-pass> <profile> <build> <first> <last> [block sessions]
//!                                                     ordered passes (C-062): a session keeps its unfinished pass of
//!                                                     this build; one of a build that does not run is cancelled and
//!                                                     replaced; one of another build that runs refuses the range
//!   vyreon-core republish <profile> <build> <scope> <first> <last> "<reason>" [--plan] [--scoped|--full] [--block N]
//!                                                     C-074 (engine D-104, D-462): recompute the families or measurements of
//!                                                     <scope> (comma-separated; each a family, else a measurement) and every
//!                                                     same-day dependant on each published day of the range. --plan prints
//!                                                     the plan and schedules nothing: the recompute set, its share of the
//!                                                     day's slots, the inputs and stages a day needs, the measured cost
//!                                                     scoped and full, and the choice (SCOPED when its measured cost for
//!                                                     the range is below FULL's). Without it the choice is scheduled:
//!                                                     REPUBLISH jobs (one a day, copy-forward revisions), day blocks of
//!                                                     --block days (default 3) for FULL, or ordered passes when the set
//!                                                     holds history sensors only; --scoped or --full overrides the choice
//!   vyreon-core compare-day <profile> <build> <date> C-074: a compare-only day block of the date: computed whole, its
//!                                                     revisions staged and never published (the scoped check's comparison)
//!   vyreon-core tape verify <profile> <build> <session>...  schedule a verify job per session (C-059): a tape worker
//!                                                     downloads the session's replay files again, recomputes each member's
//!                                                     cache and runs the tape reader on it; core keeps the verdict beside
//!                                                     the stored cache only when the recomputed one equals it byte for byte
//!   vyreon-core tape checks                           the sample (C-059): one line, then each session's checks and
//!                                                     every member not passed
//!   vyreon-core queue [pause <reason> | resume | retry-failed | cancel <job>...]   the queue at a glance, or change it
//!   vyreon-core omit <profile> <session> <reason...>  omit a date whose day failed for the provider (C-061): only while
//!                                                     its day job stands FAILED, never for an engine fault (fixed and
//!                                                     recomputed instead). Records the reason, cancels the failed day job
//!                                                     and the session's queued ordered pass; the reader API then states
//!                                                     the session NOT_PUBLISHED with the cause OMITTED and the reason
//!   vyreon-core days retire (--preview | --expect <token>) <reason> <date>...   retire published days of earlier builds (C-062): every
//!                                                     PUBLISHED revision of each date (base, day-local, history) becomes
//!                                                     SUPERSEDED, kept, never deleted; the unfinished jobs of the builds
//!                                                     that published those days are cancelled; later days' history
//!                                                     revisions that read them fall and their passes are queued again
//!                                                     (C-037). The reason is recorded (`core.retirement`, the log), and
//!                                                     the reader states each date NOT_PUBLISHED with the cause RETIRED.
//!                                                     --preview does it all in a transaction rolled back and prints
//!                                                     what it would do, with a token; the retirement itself takes
//!                                                     --expect <that token> and is refused when what the dates hold
//!                                                     has changed since (C-063)
//!   vyreon-core months <first> <last>                 create the store's monthly partitions for a date range
//!   vyreon-core publish [--vouch] <bundle>...         check, write, read back and publish day bundles, in order.
//!                                                     A revision computed from a base must name the base revision
//!                                                     it read and read its day only; --vouch is the operator's word
//!                                                     that the bundles were computed from what is published now
//!                                                     (C-042: a bundle that names no base revision, or reads history)
//!   vyreon-core evidence list [<job>]                 the failure evidence workers left (C-049), newest first
//!   vyreon-core evidence show <job> [--attempt N]     one attempt's evidence in full (the latest by default):
//!                                                     its stop message, the input plan's refusals and its log excerpt
//!   vyreon-core refcheck list [<first> [<last>]]    the reference checks (C-054) of the sessions, newest first: each
//!                                                     day's outcome, counts and cost; "superseded" when the day has been
//!                                                     published again since
//!   vyreon-core refcheck show <date> [--report]       a day's latest check in full: its record, the measurements with a
//!                                                     failure or a reference error, and each failure class with an example
//!                                                     (with --report, the suite's report as JSON instead); and the session's
//!                                                     tape verdicts (C-058): each outcome's count, every one not passed
//!   vyreon-core archive list [<date>]                 the perishable archive (C-052): each object requested, kept or
//!                                                     failed with its endpoint, bucket and key (the newest sessions, or
//!                                                     one session), each category a worker could not pack, and what is missing
//!   vyreon-core problems                              the open problems (C-050), oldest first, and the last resolved
//!   vyreon-core budget [hours]                        the request ledger (C-068): what is committed against the ceiling
//!                                                     now, the busiest minutes of the last hours (6) with the requests
//!                                                     reported and the grants never released, and how many were not
//!   vyreon-core digest [weekly]                       print the 09:00 digest (or the Saturday check) as it would be posted now
//!   vyreon-core log [n]                               the newest n (20) logged events, which are not posted
//!   vyreon-core progress                              the last 30 minutes as the progress channel (C-065) would post them
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
//!                                                     VYREON_CORE_PROGRESS_WEBHOOK_FILE (C-065, optional: a file holding a
//!                                                     second channel's webhook URL; core then posts the run's progress
//!                                                     there every 30 minutes, silently: days published with their counts
//!                                                     and reference checks, blocks started, the logged events, the queue's
//!                                                     counts; unset or absent: nothing is composed or posted for it);
//!                                                     VYREON_CORE_PROGRESS_LABEL (C-073: its headline's name, "Phase 1").
//!                                                     The watch (C-050), also read by `problems` and `digest`:
//!                                                     VYREON_CORE_WATCH_PROFILE (FREE_REPORT_64), VYREON_CORE_DEADLINES_FROM
//!                                                     (the first session with a publishing deadline; unset: the daily
//!                                                     runs' first session, else none), VYREON_CORE_DEADLINE_HOUR (20:
//!                                                     the deadline's hour, MT, the next calendar day), VYREON_CORE_VOLUME_GIB
//!                                                     (the database volume's size; unset: not checked), VYREON_CORE_BACKUP_MAX_DAYS
//!                                                     (C-068: the newest recoverable base backup's largest age in days, from
//!                                                     the backup plugin's record; unset: not checked), VYREON_CORE_PG_CLUSTER
//!                                                     (the CloudNativePG Cluster whose plugin record core reads every 5 min
//!                                                     from the Kubernetes API with its service account; unset: not read).
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
//!                                                     than this is a problem; unset: only counted in the digest).
//!                                                     The reference check (C-054): VYREON_CORE_REFCHECK_FROM (the first
//!                                                     session whose published day must have a report; unset: only the
//!                                                     reports workers send are judged); VYREON_CORE_REFCHECK_RERUN_HOURS
//!                                                     (C-073: a published day from VYREON_CORE_REFCHECK_FROM with no
//!                                                     completed check, its worker gone, gets a re-run (a REFCHECK job)
//!                                                     this many hours after publishing; default 6, `off`: none)
//!   vyreon-core engine-store export <date> <dir>      bundles and inventories of a day from a store in
//!                                                     the engine's schema (VYREON_ENGINE_PG[_FILE]; read-only)
//!
//! The database connection string comes from `VYREON_CORE_PG`, or from the file named by
//! `VYREON_CORE_PG_FILE`. It is a secret and is never printed.
mod engine_store;
use core_db::{archive, build, daily, evidence, migrate, notice, publish, refcheck, universe, watch, work};
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
    secret_file(&format!("{name}_FILE"), &file)
}

/// The trimmed text of the file the setting `name` names. A read failure names the setting, never
/// its value: a secret put in the setting itself by mistake (a connection string, a webhook URL)
/// would otherwise be printed (audit 2026-11-22 prebuild round 2, core-watch.md, 4).
fn secret_file(name: &str, file: &str) -> Result<String, String> {
    std::fs::read_to_string(file).map(|t| t.trim().to_owned()).map_err(|e| format!("{name}: the file it names cannot be read ({})", e.kind()))
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

/// A file's text, or with `-` the standard input (C-073, release audit, Gemini's suspicion 2: `kubectl
/// exec -i` gives the pod's process a pipe it may not reopen as `/dev/stdin` under its own user; fd 0
/// is read directly).
fn read_input(file: &str) -> Result<String, String> {
    if file == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).map_err(|e| format!("standard input: {e}"))?;
        return Ok(text);
    }
    std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))
}

async fn build_register(file: &str) -> Result<(), String> {
    let text = read_input(file)?;
    let inventory = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
    let r = build::register(&mut connect().await?, &inventory).await?;
    println!("{} {}: {} slots", if r.new { "registered" } else { "already registered" }, r.engine_build, r.slots);
    Ok(())
}

fn json_file(file: &str) -> Result<serde_json::Value, String> {
    let text = read_input(file)?;
    serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
}

async fn schedule(kind: &str, profile: &str, engine_build: &str, first: &str, last: &str, block: &str) -> Result<(), String> {
    let block: i32 = block.parse().map_err(|_| format!("block sessions: {block}"))?;
    let made = work::schedule(&mut connect().await?, work::Kind::parse(kind)?, profile, engine_build, first, last, block).await?;
    println!("{} jobs over {} sessions; {} partitions created", made.jobs, made.sessions, made.partitions);
    if made.kept > 0 || !made.replaced.is_empty() {
        // C-062: an ordered pass already unfinished under this build is kept; one under a build that
        // does not run is replaced
        let replaced: Vec<String> = made.replaced.iter().map(i64::to_string).collect();
        println!("{} sessions kept their unfinished pass of {engine_build}; {} passes of builds that do not run replaced ({})", made.kept, replaced.len(), replaced.join(" "));
    }
    Ok(())
}

/// C-074: `republish <profile> <build> <scope> <first> <last> <reason> [--plan] [--scoped|--full] [--block N]`.
async fn republish(profile: &str, build: &str, scope: &str, first: &str, last: &str, reason: &str, flags: &[&str]) -> Result<(), String> {
    use core_db::republish::{self as rp, Choice};
    let (mut only_plan, mut force, mut block) = (false, None, 3i64);
    let mut it = flags.iter();
    while let Some(f) = it.next() {
        match *f {
            "--plan" => only_plan = true,
            "--scoped" => force = Some(Choice::Scoped),
            "--full" => force = Some(Choice::Full),
            "--block" => block = it.next().and_then(|b| b.parse().ok()).filter(|b| *b >= 1).ok_or("--block takes a number of days, at least 1")?,
            other => return Err(format!("republish: unknown option {other}")),
        }
    }
    if reason.trim().is_empty() {
        return Err("a republish states its reason".into());
    }
    // (release-2 audit, Codex 20) a reason is stored, logged and posted: never a credential
    if let Some(found) = core_db::evidence::unredacted(reason) {
        return Err(format!("{found}: a republish's reason carries no credential; nothing was scheduled"));
    }
    let mut client = connect().await?;
    let today: String = client.query_one("SELECT ((now() AT TIME ZONE 'UTC')::date)::text", &[]).await.map_err(|e| e.to_string())?.get(0);
    let names: Vec<&str> = scope.split(',').collect();
    let plan = rp::plan(&client, profile, build, &names, first, last, block, &today).await?;
    // (release-2 audit, Codex 14) the choice the run would make, overrides applied, is what the plan says
    let choice = match (force, plan.choice) {
        (Some(Choice::Scoped), Choice::Passes) => return Err("--scoped: the scope holds history sensors only; no worker has anything to recompute".into()),
        (Some(Choice::Scoped), _) if !plan.stale.is_empty() => return Err("--scoped: some days are published in other profile revisions; a copy-forward cannot cross them".into()),
        (Some(f), _) => f,
        (None, c) => c,
    };
    for line in rp::describe(&plan) {
        println!("{line}");
    }
    if force.is_some() && force != Some(plan.choice) {
        println!("overridden: {} (as asked)", choice.name());
    }
    if only_plan {
        return Ok(());
    }
    // no faithful recompute (the scope reads inputs the provider no longer serves): only on the word
    if plan.choice == Choice::Full && plan.why.starts_with("no faithful recompute") && force != Some(Choice::Full) {
        return Err("nothing was scheduled: pass --full to recompute those days whole anyway".into());
    }
    // (release-2 audit, Codex 6, Gemini 5) only the published days the plan names, in runs of consecutive
    // sessions: an unpublished or omitted day of the range is left as it is
    let runs = plan.runs();
    if runs.is_empty() {
        return Err("no session of the range has a published day: nothing to recompute".into());
    }
    match choice {
        Choice::Scoped => {
            let made = rp::schedule(&mut client, profile, build, &plan.work, &plan.sessions, reason).await?;
            println!("{} republish jobs: {}", made.jobs.len(), made.jobs.iter().map(i64::to_string).collect::<Vec<_>>().join(" "));
        }
        Choice::Full => {
            let mut jobs = 0;
            for (a, b) in &runs {
                jobs += work::schedule(&mut client, work::Kind::DayBlock, profile, build, a, b, block as i32).await?.jobs;
            }
            notice::log(&client, &format!("Full recompute of {} sessions, {} to {}, under build {build} queued as {jobs} day blocks ({}).", plan.sessions.len(), runs[0].0, runs[runs.len() - 1].1, watch::one_line(reason, 200))).await?;
            println!("{jobs} day blocks over {} sessions", plan.sessions.len());
        }
        Choice::Passes => {
            let mut jobs = 0;
            for (a, b) in &runs {
                jobs += work::schedule(&mut client, work::Kind::OrderedPass, profile, build, a, b, 1).await?.jobs;
            }
            println!("{jobs} ordered passes over {} sessions", plan.sessions.len());
        }
    }
    Ok(())
}

/// C-059: `tape verify <profile> <build> <session>...`.
async fn tape_verify(profile: &str, engine_build: &str, sessions: &[&str]) -> Result<(), String> {
    let jobs = work::schedule_verify(&mut connect().await?, profile, engine_build, sessions).await?;
    println!("{} verify jobs: {}", jobs.len(), jobs.iter().map(i64::to_string).collect::<Vec<_>>().join(" "));
    Ok(())
}

/// C-059: a session's line in `tape checks`.
fn sample_session_line(s: &serde_json::Value) -> String {
    let n = |k: &str| s[k].as_i64().unwrap_or(0);
    let gb = s["tape_bytes"].as_i64().map_or_else(|| "replay bytes not stated".to_owned(), |b| format!("{:.1} GB of replay files", b as f64 / 1e9));
    let jobs: Vec<String> = s["jobs"].as_array().into_iter().flatten().map(|j| j.to_string()).collect();
    format!(
        "{} {}: {} of {} listed members checked, {} match ({} PASS, {} WITHHELD, {} FAIL, {} NOT_RUN, {} without a verdict), {} differ, {} not compared · {gb} · job {} · {}",
        s["session"].as_str().unwrap_or("?"),
        if s["passed"] == true { "passed" } else { "NOT passed" },
        n("checked"),
        n("listed"),
        n("MATCH"),
        n("PASS"),
        n("WITHHELD"),
        n("FAIL"),
        n("NOT_RUN"),
        n("no_verdict"),
        n("DIFFER"),
        n("NOT_COMPARED"),
        jobs.join(" "),
        s["last_checked_at"].as_str().unwrap_or("")
    )
}

/// C-059: `tape checks`.
async fn tape_checks() -> Result<(), String> {
    let sessions = core_db::tape::sample(&connect().await?).await?;
    let Some(line) = core_db::tape::sample_line(&sessions) else {
        println!("no tape cache has been verified by sample");
        return Ok(());
    };
    println!("{line}");
    for s in &sessions {
        println!("{}", sample_session_line(s));
        for m in s["not_passed"].as_array().into_iter().flatten() {
            let why: String = m["why"].as_str().unwrap_or("").chars().take(300).collect();
            println!("    {} {} verdict {}: {why}", m["asset"].as_str().unwrap_or("?"), m["outcome"].as_str().unwrap_or("?"), m["verdict"].as_str().unwrap_or("none"));
        }
    }
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

/// C-061: `omit <profile> <session> <reason...>`.
async fn omit(profile: &str, session: &str, reason: &[&str]) -> Result<(), String> {
    let done = work::omit(&mut connect().await?, profile, session, &reason.join(" ")).await?;
    let passes = match done.passes.as_slice() {
        [] => "no queued ordered pass".to_owned(),
        jobs => format!("ordered pass {} cancelled", jobs.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")),
    };
    println!("{session} omitted (omission {}): day job {} cancelled ({}: {}); {passes}", done.omission_id, done.day_job, done.cause.0, done.cause.1);
    Ok(())
}

/// C-062: `days retire (--preview | --expect <token>) <reason> <date>...`; the flags may stand
/// anywhere, exactly one of them (C-063: a retirement takes the token its preview printed), and any
/// other `--` word is refused before anything is done.
fn retire_args<'a>(rest: &[&'a str]) -> Result<(&'a str, Vec<&'a str>, bool, Option<&'a str>), String> {
    let usage = "days retire (--preview | --expect <token>) <reason> <date>...";
    let (mut words, mut preview, mut expect) = (Vec::new(), false, None);
    let mut args = rest.iter().copied();
    while let Some(a) = args.next() {
        match a {
            "--preview" => preview = true,
            "--expect" => match args.next() {
                Some(token) if !token.starts_with("--") && expect.is_none() => expect = Some(token),
                _ => return Err(format!("days retire: --expect takes one token; nothing was retired ({usage})")),
            },
            flag if flag.starts_with("--") => return Err(format!("days retire: unknown option {flag}; nothing was retired ({usage})")),
            word => words.push(word),
        }
    }
    if preview == expect.is_some() {
        return Err(format!("days retire: either --preview, or --expect with the token the preview printed; nothing was retired ({usage})"));
    }
    match words.split_first() {
        Some((reason, dates)) if !dates.is_empty() => Ok((reason, dates.to_vec(), preview, expect)),
        _ => Err(format!("days retire: a reason and at least one date; nothing was retired ({usage})")),
    }
}

/// C-068: `build hand-over (--preview | --expect <token>) <from> <to> <reason...>`; the flags may
/// stand anywhere, exactly one of them, as `days retire`'s.
fn hand_over_args<'a>(rest: &[&'a str]) -> Result<(&'a str, &'a str, String, bool, Option<&'a str>), String> {
    let usage = "build hand-over (--preview | --expect <token>) <from> <to> <reason...>";
    let (mut words, mut preview, mut expect) = (Vec::new(), false, None);
    let mut args = rest.iter().copied();
    while let Some(a) = args.next() {
        match a {
            "--preview" => preview = true,
            "--expect" => match args.next() {
                Some(token) if !token.starts_with("--") && expect.is_none() => expect = Some(token),
                _ => return Err(format!("build hand-over: --expect takes one token; nothing was moved ({usage})")),
            },
            flag if flag.starts_with("--") => return Err(format!("build hand-over: unknown option {flag}; nothing was moved ({usage})")),
            word => words.push(word),
        }
    }
    if preview == expect.is_some() {
        return Err(format!("build hand-over: either --preview, or --expect with the token the preview printed; nothing was moved ({usage})"));
    }
    match words.as_slice() {
        [from, to, reason @ ..] if !reason.is_empty() => Ok((from, to, reason.join(" "), preview, expect)),
        _ => Err(format!("build hand-over: the two builds and a reason; nothing was moved ({usage})")),
    }
}

async fn build_hand_over(rest: &[&str]) -> Result<(), String> {
    let (from, to, reason, preview, expect) = hand_over_args(rest)?;
    let h = build::hand_over(&mut connect().await?, from, to, &reason, preview, expect).await?;
    if h.withdrew {
        println!("build {from} still runs: it is withdrawn by this hand-over (its live leases end, their jobs carried)");
    }
    for c in &h.carried {
        let new = c.new_job.map_or_else(|| "a new job".to_owned(), |j| format!("job {j}"));
        println!("{} job {} ({}) {} to {}: cancelled; {new} of build {to}", c.kind, c.old_job, c.state, c.first, c.last);
    }
    for j in &h.verify_cancelled {
        println!("TAPE verify job {j}: cancelled, not carried (a sample of build {from}'s caches)");
    }
    match h.hand_over_id {
        Some(id) => println!("hand-over {id}: {} jobs of build {from} are build {to}'s; now admit build {to} (`vyreon-core build admit`)", h.carried.len()),
        None => println!(
            "preview: nothing was changed; to hand over exactly this: vyreon-core build hand-over --expect {} {from} {to} {reason:?}",
            h.token
        ),
    }
    Ok(())
}

async fn days_retire(rest: &[&str]) -> Result<(), String> {
    let (reason, dates, preview, expect) = retire_args(rest)?;
    let r = publish::retire(&mut connect().await?, reason, &dates, preview, expect).await?;
    for v in &r.revisions {
        println!("{} revision {} {} {} of build {} ({})", v.date, v.revision_id, v.profile_id, v.profile_revision, v.engine_build, v.kind);
    }
    let list = |v: &[String]| if v.is_empty() { "none".to_owned() } else { v.join(" ") };
    let jobs: Vec<String> = r.cancelled.iter().map(i64::to_string).collect();
    println!("unfinished jobs cancelled: {}", list(&jobs));
    println!("later history revisions that read them, superseded: {}", r.fallen);
    println!("days the ordered pass derives again: {}", list(&r.rederived));
    println!("days no job derives again: {}", list(&r.unowned));
    match r.retirement_id {
        Some(id) => println!("retirement {id}: {} revisions superseded (kept)", r.revisions.len()),
        None => println!("preview: nothing was changed; to retire exactly this: vyreon-core days retire --expect {} {reason:?} {}", r.token, dates.join(" ")),
    }
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
        Ok(v) => v.parse().map_err(|_| format!("{name}: cannot read its value")),
        Err(_) => Ok(default),
    }
}

/// C-073: the progress channel's headline label (`VYREON_CORE_PROGRESS_LABEL`, e.g. "Phase 1").
fn progress_label() -> String {
    set("VYREON_CORE_PROGRESS_LABEL").unwrap_or_else(|| core_db::progress::DEFAULT_LABEL.to_owned())
}

/// A setting that is set and not blank.
fn set(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn date_setting(name: &str) -> Result<Option<String>, String> {
    match set(name) {
        Some(d) if !core_contract::is_date(&d) => Err(format!("{name} is not a date written YYYY-MM-DD")),
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
                Some(v) => Some(v.parse::<i32>().ok().filter(|d| *d >= 1).ok_or_else(|| "VYREON_CORE_ARCHIVE_ALERT_DAYS is not a whole number of days from 1".to_string())?),
            };
            let expected_from = date_setting("VYREON_CORE_ARCHIVE_FROM")?.or_else(|| daily.as_ref().map(|d| d.from.clone()));
            Some(watch::ArchiveWatch { expected_from, alert_days })
        }
    };
    // C-054: every published day from this session is expected to have its reference check
    let refcheck_from = date_setting("VYREON_CORE_REFCHECK_FROM")?;
    // C-073: a day with no completed check gets a re-run this many hours after publishing (default
    // `refcheck::RERUN_AFTER_HOURS`; `off`: none; only with VYREON_CORE_REFCHECK_FROM)
    let refcheck_rerun_hours = match set("VYREON_CORE_REFCHECK_RERUN_HOURS").as_deref() {
        None => Some(core_db::refcheck::RERUN_AFTER_HOURS),
        Some("off") => None,
        Some(v) => Some(v.parse::<f64>().ok().filter(|h| h.is_finite() && *h >= 1.0).ok_or_else(|| "VYREON_CORE_REFCHECK_RERUN_HOURS is not a number of hours from 1, or off".to_string())?),
    };
    // C-068: the newest base backup's age is watched when this is set (with the volume's size)
    let backup_days = match set("VYREON_CORE_BACKUP_MAX_DAYS") {
        None => None,
        Some(v) => Some(v.parse::<i32>().ok().filter(|d| *d >= 1).ok_or_else(|| "VYREON_CORE_BACKUP_MAX_DAYS is not a whole number of days from 1".to_string())?),
    };
    Ok(watch::Settings { profile, deadlines_from, volume_bytes: volume.map(|g| (g * (1u64 << 30) as f64) as i64), deadline_hour, daily, archive, refcheck_from, backup_days, refcheck_rerun_hours })
}

/// A Discord webhook's URL from the file a setting names; None (said, never the URL, nor the
/// setting's value, which may be the URL put there by mistake) when the setting or the file is
/// absent or empty.
fn webhook_setting(name: &str, without: &str) -> Option<String> {
    match webhook_from(name, std::env::var(name).ok().as_deref(), without) {
        Ok(url) => Some(url),
        Err(said) => {
            println!("{said}");
            None
        }
    }
}

/// `webhook_setting` for the setting's value `file`: the URL, or what is said instead.
fn webhook_from(name: &str, file: Option<&str>, without: &str) -> Result<String, String> {
    let Some(file) = file else {
        return Err(format!("{name} is not set: {without}"));
    };
    match secret_file(name, file) {
        Ok(url) if !url.is_empty() => Ok(url),
        _ => Err(format!("the file {name} names is absent or empty: {without}")),
    }
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

/// One archive object as `archive list` prints it: where it is (endpoint, bucket, key: round w,
/// 18, C-052) and its state.
fn archive_line(r: &serde_json::Value) -> String {
    let text = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
    let state = match text("state").as_str() {
        "KEPT" => format!("KEPT {}", text("kept_at")),
        "FAILED" => format!("FAILED: {}", text("failure")),
        other => format!("{other} {} (not kept yet)", text("requested_at")),
    };
    let endpoint = r["endpoint"].as_str().unwrap_or("(endpoint not recorded)");
    format!("{} {:<12} {} bytes {endpoint}/{}/{} job {} by {}: {state}", text("session"), text("category"), r["bytes"], text("bucket"), text("key"), r["job_id"], text("worker"))
}

/// `archive list [<date>]`: one line per archive object, newest session first, then what is missing.
async fn archive_list(session: Option<&str>) -> Result<(), String> {
    let client = connect().await?;
    let rows = archive::list(&client, session, 200).await?;
    if rows.is_empty() {
        println!("no perishable archive{}", session.map_or(String::new(), |s| format!(" for {s}")));
    }
    for r in &rows {
        println!("{}", archive_line(r));
    }
    // round w, 7: what a worker could not pack is never silent
    for (session_, category, why, job, worker, at) in archive::unpacked_list(&client, session, 200).await? {
        println!("{session_} {category:<12} not packed at {at}, job {job} by {worker}: {why}");
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
            "SELECT session::text, engine_build, to_char(queued_at AT TIME ZONE $2, 'Dy Mon FMDD HH24:MI'), jobs::text
             FROM core.daily_session WHERE profile_id = $1 ORDER BY session DESC LIMIT 10",
            &[&d.profile, &watch::ZONE],
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
            "SELECT to_char(resolved_at AT TIME ZONE $1, 'Dy Mon FMDD HH24:MI'), summary FROM core.problem
             WHERE resolved_at IS NOT NULL ORDER BY resolved_at DESC, problem_id DESC LIMIT 10",
            &[&watch::ZONE],
        )
        .await
        .map_err(|e| e.to_string())?;
    for r in &rows {
        println!("resolved {} MT: {}", r.get::<_, String>(0), r.get::<_, String>(1));
    }
    Ok(())
}

async fn digest(weekly: bool) -> Result<(), String> {
    let mut client = connect().await?;
    let now = watch::now(&client).await?;
    let text = if weekly { watch::weekly(&mut client, &watch_settings()?, &now).await? } else { watch::digest(&mut client, &watch_settings()?, &now).await? };
    println!("{text}");
    Ok(())
}

async fn progress_preview() -> Result<(), String> {
    let mut client = connect().await?;
    let now = watch::now(&client).await?;
    let posts = core_db::progress::preview_as(&mut client, &watch_settings()?.profile, &progress_label(), &now).await?;
    if posts.is_empty() {
        println!("nothing happened in the last 30 minutes: no progress post");
    }
    for p in posts {
        println!("{p}\n");
    }
    Ok(())
}

/// C-068 (audit opsurface ops.md 12): the request ledger with its limits stated.
async fn budget_ledger(hours: &str) -> Result<(), String> {
    let hours: f64 = hours.parse().ok().filter(|h: &f64| *h > 0.0).ok_or_else(|| format!("hours: {hours}"))?;
    let client = connect().await?;
    let ceiling: i32 = setting("VYREON_CORE_INTRINIO_PER_MINUTE", core_db::budget::Budget::default().per_minute)?;
    println!("committed now: {} of {ceiling} a minute", core_db::budget::committed(&client).await?);
    // C-073: the scaler's view of the budget
    let now = watch::now(&client).await?;
    match core_db::budget::pressed(&client, &now).await? {
        Some((at, n)) => println!(
            "pressed: at least {n} draws waited in the last {:.0} min, the last at {at} UTC: KEDA starts no tape, backfill block or re-run pod until it is quiet that long",
            core_db::budget::PRESSURE_MINUTES
        ),
        None => println!("not pressed: no draw waited in the last {:.0} min (KEDA starts pods for all claimable work)", core_db::budget::PRESSURE_MINUTES),
    }
    let (n, requests) = core_db::budget::unreleased(&client, hours).await?;
    println!("leases never released in the last {hours} h: {n} ({requests} requests granted; what they used is unknown)");
    println!("busiest minutes (UTC), by the leases granted in them: used as reported + granted and never released; at most this many requests were made in the minute (its and the previous minute's leases); workers");
    for m in core_db::budget::ledger(&client, hours, 10).await? {
        println!("{} {:>5} + {:>4}  at most {:>5}  {} workers", m.minute, m.released, m.unreleased, m.bound, m.workers);
    }
    println!("a lease's requests fall in its grant minute or the next, so the bound is an upper bound per minute; Intrinio's own account usage is the measure of what was spent");
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
    // C-053: one core at a time. The lock is held by this connection for the service's life; a
    // second service refuses to start, and this one stops when the connection is lost
    let (holder, connection) = pg.connect(NoTls).await.map_err(|e| format!("postgres connect: {e}"))?;
    let mut lost = tokio::spawn(connection);
    if !core_db::instance::take(&holder).await? {
        return Err("another vyreon-core serve holds the database's single-instance lock (C-053): this one does not serve".into());
    }
    let manager = deadpool_postgres::Manager::new(pg, NoTls);
    // C-068: up to 24 tape pods, 13 day blocks and a pass at once (C-066's caps) each heartbeat and
    // draw from the budget on a connection held for one statement or two; an upload holds none
    // while it spools or waits for the bundle before it. PostgreSQL allows 100 (CloudNativePG's
    // default); the watch, the scaler's four counts and the pass's login are beside these
    let pool = deadpool_postgres::Pool::builder(manager).max_size(32).build().map_err(|e| e.to_string())?;
    let app = core_api::App::new(pool, config);
    let listen: String = setting("VYREON_CORE_LISTEN", "0.0.0.0:8080".to_string())?;
    let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| format!("{listen}: {e}"))?;
    core_api::spawn_sweeper(app.clone());
    // the webhook's file may be absent (the Secret not created yet): the posts then wait unsent
    let webhook = webhook_setting("VYREON_CORE_WEBHOOK_FILE", "the channel's posts are kept unsent");
    // C-065: the progress channel, optional; absent, nothing is composed or posted for it
    let progress = webhook_setting("VYREON_CORE_PROGRESS_WEBHOOK_FILE", "no progress is posted");
    // the watch and the sender run apart: delivering posts never delays the watch's queueing
    let settings = watch_settings()?;
    let profile = settings.profile.clone();
    // C-068 round 2: the backup plugin's record, read from the Kubernetes API, is the backups' evidence
    if let Some(cluster) = set("VYREON_CORE_PG_CLUSTER") {
        core_api::backups::spawn(app.clone(), core_api::backups::Kube::in_cluster(), cluster, std::time::Duration::from_secs(300));
    }
    core_api::notices::spawn_watch(app.clone(), settings, std::time::Duration::from_secs(60));
    if let Some(webhook) = webhook {
        core_api::notices::spawn_sender(app.clone(), webhook, std::time::Duration::from_secs(15));
    }
    if let Some(webhook) = progress {
        core_api::notices::spawn_progress(app.clone(), profile, progress_label(), std::time::Duration::from_secs(60));
        core_api::notices::spawn_sender_on(app.clone(), webhook, std::time::Duration::from_secs(15), notice::Channel::Progress);
    }
    println!("vyreon-core listening on {listen} (the single-instance lock is held, C-053)");
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(15));
    let held = async {
        loop {
            ping.tick().await;
            match tokio::time::timeout(std::time::Duration::from_secs(10), core_db::instance::held(&holder)).await {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => return "the single-instance lock is no longer held".to_string(),
                Ok(Err(e)) => return format!("the connection holding the single-instance lock failed: {e}"),
                Err(_) => return "the connection holding the single-instance lock did not answer in 10 s".to_string(),
            }
        }
    };
    tokio::select! {
        served = axum::serve(listener, core_api::router(app)) => served.map_err(|e| e.to_string()),
        why = held => Err(format!("{why}: stopping, so that no second core serves beside this one (C-053)")),
        _ = &mut lost => Err("the connection holding the single-instance lock ended: stopping (C-053)".into()),
    }
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
            r["stored_mt"].as_str().unwrap_or_default(),
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

/// One report as `refcheck list` prints it.
fn refcheck_line(r: &serde_json::Value) -> String {
    let n = |k: &str| r[k].as_i64().unwrap_or(0);
    let min = |k: &str| r[k].as_f64().map(|s| format!("{:.0}", s / 60.0));
    let what = if r["outcome"] == "COMPLETED" {
        format!(
            "{} checked, {} failed{} ({} classes, {} measurements), {} skipped, {} reference errors",
            n("checked"),
            n("failed"),
            // C-056: the failures the worker's rulings table does not explain
            // C-054 amended 2026-11-20: an account from a suite without rulings is not triaged
            match r["failed_unexplained"].as_i64() {
                Some(u) => format!(" ({u} unexplained)"),
                None if n("failed") > 0 => " (not triaged against rulings)".into(),
                None => String::new(),
            },
            n("failure_classes"),
            n("measurements_failing"),
            n("skipped"),
            n("reference_errors")
        )
    } else {
        r["why"].as_str().and_then(|w| w.lines().next()).unwrap_or_default().chars().take(120).collect()
    };
    let cost = match (min("suite_seconds"), min("cpu_seconds")) {
        (Some(w), Some(c)) => format!("suite {w} min, {c} CPU min{}", if r["cpu_complete"] == true { "" } else { " or more" }),
        (Some(w), None) => format!("suite {w} min"),
        _ => format!("{} min", min("wall_seconds").unwrap_or_default()),
    };
    format!(
        "{} {:<9} {what} · {cost}, {} jobs · job {} attempt {} by {} · report {} {}{}",
        r["session"].as_str().unwrap_or_default(),
        r["outcome"].as_str().unwrap_or_default(),
        r["jobs"],
        r["job_id"],
        r["attempt"],
        r["worker"].as_str().unwrap_or_default(),
        r["report_id"],
        r["stored_at"].as_str().unwrap_or_default(),
        if r["current"] == true { "" } else { " (superseded)" },
    )
}

/// `refcheck list [<first> [<last>]]`: one line per report, newest session first.
async fn refcheck_list(from: Option<&str>, to: Option<&str>) -> Result<(), String> {
    for d in [from, to].into_iter().flatten() {
        if !core_contract::is_date(d) {
            return Err(format!("{d} is not a date written YYYY-MM-DD"));
        }
    }
    let rows = refcheck::list(&connect().await?, from, to, 500).await?;
    if rows.is_empty() {
        println!("no reference check reports");
    }
    for r in &rows {
        println!("{}", refcheck_line(r));
    }
    Ok(())
}

/// C-058: a session's tape verdicts as `refcheck show` prints them.
fn tape_lines(tape: &serde_json::Value) -> Vec<String> {
    let counts: Vec<String> = tape["counts"].as_object().into_iter().flatten().map(|(k, n)| format!("{n} {k}")).collect();
    let mut out = vec![format!("== tape verdicts of {} caches: {}", tape["caches"], counts.join(", "))];
    for v in tape["not_passed"].as_array().into_iter().flatten() {
        out.push(format!("{} {}: {}", v["asset"].as_str().unwrap_or("?"), v["outcome"].as_str().unwrap_or("none"), v["why"].as_str().unwrap_or("no verdict stored for this cache")));
    }
    out
}

/// `refcheck show <date> [--report]`: the record, the measurements with a failure, and each
/// failure class with its example and the objects it names (or the report itself).
async fn refcheck_show(session: &str, whole: bool) -> Result<(), String> {
    let client = connect().await?;
    let tape = refcheck::tape_show(&client, session).await?;
    let Some((record, summary, report)) = refcheck::show(&client, session, None).await? else {
        if whole || tape["caches"].as_i64().unwrap_or(0) == 0 {
            return Err(format!("no reference check report for {session}"));
        }
        println!("no reference check report for {session}");
        tape_lines(&tape).iter().for_each(|l| println!("{l}"));
        return Ok(());
    };
    if whole {
        if report.is_empty() {
            return Err(format!("the report of {session} holds no report ({})", record["outcome"].as_str().unwrap_or_default()));
        }
        println!("{report}");
        return Ok(());
    }
    println!("{}", refcheck_line(&record));
    println!("{}", serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?);
    let failing: Vec<String> = summary
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v["failed"].as_i64().unwrap_or(0) > 0 || v["reference_errors"].as_i64().unwrap_or(0) > 0)
        .map(|(m, v)| {
            let unexplained = v["failed_unexplained"].as_i64().map(|u| format!(" ({u} unexplained)")).unwrap_or_default();
            format!("{m}: {} checked, {} failed{unexplained}, {} skipped, {} reference errors", v["checked"], v["failed"], v["skipped"], v["reference_errors"].as_i64().unwrap_or(0))
        })
        .collect();
    println!("== measurements with a failure or a reference error: {}", failing.len());
    failing.iter().for_each(|l| println!("{l}"));
    if tape["caches"].as_i64().unwrap_or(0) > 0 {
        tape_lines(&tape).iter().for_each(|l| println!("{l}"));
    }
    if report.is_empty() {
        return Ok(());
    }
    let report: serde_json::Value = serde_json::from_str(&report).map_err(|e| format!("the stored report: {e}"))?;
    let classes = report["failure_classes"].as_array().cloned().unwrap_or_default();
    println!("== failure classes: {}", classes.len());
    for c in &classes {
        let objects: Vec<&str> = c["objects"].as_array().into_iter().flatten().filter_map(|o| o.as_str()).collect();
        let shown = objects.iter().take(20).copied().collect::<Vec<_>>().join(" ");
        let more = if objects.len() > 20 { format!(" and {} more", objects.len() - 20) } else { String::new() };
        println!("{} {} [{}]: {} ({shown}{more})", c["measurement_id"].as_str().unwrap_or_default(), c["role_id"].as_str().unwrap_or_default(), c["class"].as_str().unwrap_or_default(), c["count"]);
        if let Some(alt) = c["matches_alternative"].as_object().filter(|a| !a.is_empty()) {
            println!("    matches an alternative reading: {}", serde_json::Value::Object(alt.clone()));
        }
        if !c["example"].is_null() {
            println!("    e.g. {}", c["example"]);
        }
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
            Ok(mut client) => build::withdraw(&mut client, engine_build, reason).await.map(|stopped| {
                if !stopped.is_empty() {
                    println!("running jobs stopped (their leases ended, queued again under the withdrawn build): {}", stopped.iter().map(i64::to_string).collect::<Vec<_>>().join(" "));
                }
            }),
            Err(e) => Err(e),
        },
        ["build", "hand-over", ref rest @ ..] => build_hand_over(rest).await,
        ["calendar", "load", file] => match (connect().await, json_file(file)) {
            (Ok(mut client), Ok(calendar)) => work::load_calendar(&mut client, &calendar).await.map(|n| println!("{n} sessions")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        ["schedule", kind, profile, engine_build, first, last] => schedule(kind, profile, engine_build, first, last, "30").await,
        ["schedule", kind, profile, engine_build, first, last, block] => schedule(kind, profile, engine_build, first, last, block).await,
        ["republish", profile, engine_build, scope, first, last, reason, ref flags @ ..] => republish(profile, engine_build, scope, first, last, reason, flags).await,
        ["compare-day", profile, engine_build, day] => match connect().await {
            Ok(mut client) => core_db::republish::schedule_compare(&mut client, profile, engine_build, day).await.map(|job| println!("compare-only day block {job}")),
            Err(e) => Err(e),
        },
        ["tape", "verify", profile, engine_build, ref sessions @ ..] => tape_verify(profile, engine_build, sessions).await,
        ["tape", "checks"] => tape_checks().await,
        ["queue", ref change @ ..] => queue(change).await,
        ["omit", profile, session, ref reason @ ..] if !reason.is_empty() => omit(profile, session, reason).await,
        ["days", "retire", ref rest @ ..] => days_retire(rest).await,
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
        ["refcheck", "list"] => refcheck_list(None, None).await,
        ["refcheck", "list", from] => refcheck_list(Some(from), None).await,
        ["refcheck", "list", from, to] => refcheck_list(Some(from), Some(to)).await,
        ["refcheck", "show", session] => refcheck_show(session, false).await,
        ["refcheck", "show", session, "--report"] => refcheck_show(session, true).await,
        ["archive", "list"] => archive_list(None).await,
        ["archive", "list", date] => archive_list(Some(date)).await,
        ["problems"] => problems().await,
        ["budget"] => budget_ledger("6").await,
        ["budget", hours] => budget_ledger(hours).await,
        ["digest"] => digest(false).await,
        ["digest", "weekly"] => digest(true).await,
        ["progress"] => progress_preview().await,
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
    use super::{archive_line, hand_over_args, publish_args, retire_args};

    /// Audit 2026-11-22 prebuild round 2 (core-watch.md) 4: the webhook URL put in the setting
    /// itself (instead of the file that holds it) printed the whole URL, token and all, when the
    /// file could not be read. What is said names the setting only. The URL is the shape of the
    /// existing tests' (`notices::tests`), not a real one.
    #[test]
    fn a_setting_that_cannot_be_read_is_named_never_printed() {
        let url = "https://discord.com/api/webhooks/1/a-token";
        let said = super::webhook_from("VYREON_CORE_PROGRESS_WEBHOOK_FILE", Some(url), "no progress is posted").unwrap_err();
        assert_eq!(said, "the file VYREON_CORE_PROGRESS_WEBHOOK_FILE names is absent or empty: no progress is posted");
        assert_eq!(super::webhook_from("VYREON_CORE_WEBHOOK_FILE", None, "kept").unwrap_err(), "VYREON_CORE_WEBHOOK_FILE is not set: kept");
        let conninfo = "host=db user=vyreon password=a-token";
        let said = super::secret_file("VYREON_CORE_PG_FILE", conninfo).unwrap_err();
        assert!(said.starts_with("VYREON_CORE_PG_FILE: the file it names cannot be read (") && !said.contains("a-token"), "{said}");
    }

    /// Audit 2026-11-17 round w, 18: `archive list` names the endpoint the object is kept at.
    #[test]
    fn an_archive_line_names_its_endpoint_bucket_and_key() {
        let row = serde_json::json!({
            "session": "2025-12-10", "category": "act01-auto", "state": "KEPT", "bytes": 4242, "bucket": "vyreon-data",
            "key": "perishables/act01-auto/2025-12-10/4ef2e745.tar.zst", "kept_at": "2025-12-11T13:06:00Z", "job_id": 7, "worker": "w1",
            "endpoint": "https://nyc3.digitaloceanspaces.com",
        });
        assert_eq!(
            archive_line(&row),
            "2025-12-10 act01-auto   4242 bytes https://nyc3.digitaloceanspaces.com/vyreon-data/perishables/act01-auto/2025-12-10/4ef2e745.tar.zst job 7 by w1: KEPT 2025-12-11T13:06:00Z"
        );
    }

    /// C-054: a report's line says the day, outcome, counts, cost and who sent it, and whether the
    /// day has been published again since (counts of the real B0-census report of 2025-12-16).
    #[test]
    fn a_refcheck_line_counts_and_costs_the_day() {
        let row = serde_json::json!({
            "report_id": 3, "session": "2025-12-16", "outcome": "COMPLETED", "why": null, "checked": 91066, "passed": 87108, "failed": 3958,
            "skipped": 3751, "reference_errors": 0, "failure_classes": 384, "measurements_failing": 41, "wall_seconds": 2460.0,
            "suite_seconds": 2400.0, "cpu_seconds": 16800.0, "cpu_complete": true, "jobs": 8, "deadline_seconds": 7200, "job_id": 7,
            "attempt": 1, "worker": "w1", "engine_build": "9bc02f2d+90a50956a51d", "base_revision": 17, "report_bytes": 520916,
            "report_trimmed": null, "stored_at": "2026-11-19T03:00:00Z", "current": false,
        });
        assert_eq!(
            super::refcheck_line(&row),
            "2025-12-16 COMPLETED 91066 checked, 3958 failed (not triaged against rulings) (384 classes, 41 measurements), 3751 skipped, 0 reference errors · suite 40 min, 280 CPU min, 8 jobs · job 7 attempt 1 by w1 · report 3 2026-11-19T03:00:00Z (superseded)"
        );
        let timeout = serde_json::json!({
            "report_id": 4, "session": "2025-12-17", "outcome": "TIMEOUT", "why": "not done in 120 min: its process group was killed; its log ends:\nstarted",
            "wall_seconds": 7203.0, "suite_seconds": 7200.0, "cpu_seconds": null, "cpu_complete": false, "jobs": 8, "job_id": 7, "attempt": 1,
            "worker": "w1", "stored_at": "2026-11-19T05:00:00Z", "current": true,
        });
        assert_eq!(
            super::refcheck_line(&timeout),
            "2025-12-17 TIMEOUT   not done in 120 min: its process group was killed; its log ends: · suite 120 min, 8 jobs · job 7 attempt 1 by w1 · report 4 2026-11-19T05:00:00Z"
        );
    }

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

    /// C-062: `days retire` reads its whole line first: `--preview` or `--expect <token>` (C-063: one
    /// of them) wherever it stands, the reason first, at least one date, and no other option.
    #[test]
    fn the_retire_command_line_is_read_whole_before_anything_is_retired() {
        let reason = "pre-D-397 build";
        assert_eq!(retire_args(&["--expect", "9f86d081884c7d65", reason, "2022-06-06", "2022-06-07"]), Ok((reason, vec!["2022-06-06", "2022-06-07"], false, Some("9f86d081884c7d65"))));
        assert_eq!(retire_args(&[reason, "2022-06-06", "--expect", "9f86d081884c7d65"]), Ok((reason, vec!["2022-06-06"], false, Some("9f86d081884c7d65"))));
        assert_eq!(retire_args(&["--preview", reason, "2022-06-06"]), Ok((reason, vec!["2022-06-06"], true, None)));
        assert_eq!(retire_args(&[reason, "2022-06-06", "--preview"]), Ok((reason, vec!["2022-06-06"], true, None)));
        for bad in [
            &[][..],
            &[reason],
            &["--preview", reason],
            &[reason, "--force", "2022-06-06"],
            &[reason, "2022-06-06"],
            &["--preview", "--expect", "9f86d081884c7d65", reason, "2022-06-06"],
            &[reason, "2022-06-06", "--expect"],
            &["--expect", "a", "--expect", "b", reason, "2022-06-06"],
        ] {
            let refused = retire_args(bad).unwrap_err();
            assert!(refused.contains("nothing was retired"), "{refused}");
        }
    }

    /// C-068: `build hand-over` reads its whole line first, as `days retire` does: the two builds
    /// (the real ones of the rollout), a reason of several words, and exactly one of the flags.
    #[test]
    fn the_hand_over_command_line_is_read_whole_before_anything_moves() {
        let (old, new) = ("914ffed7+7d4e45c05f63", "48461197+7361a31e2fda");
        let words = [old, new, "the", "rulings", "image"];
        assert_eq!(hand_over_args(&[&["--preview"][..], &words].concat()), Ok((old, new, "the rulings image".to_owned(), true, None)));
        assert_eq!(hand_over_args(&[&words[..], &["--expect", "9f86d081884c7d65"]].concat()), Ok((old, new, "the rulings image".to_owned(), false, Some("9f86d081884c7d65"))));
        for bad in [
            &[][..],
            &["--preview", old, new],
            &[old, new, "image"],
            &["--preview", "--expect", "9f86d081884c7d65", old, new, "image"],
            &[old, new, "image", "--expect"],
            &["--force", old, new, "image"],
        ] {
            let refused = hand_over_args(bad).unwrap_err();
            assert!(refused.contains("nothing was moved"), "{refused}");
        }
    }
}
