//! C-079 (owner, 2026-11-24: automation debt 5): after a method change, the published days whose
//! methods differ from the new build's are republished without anyone looking for them.
//!
//! **What a build's methods are** is what it states in what it publishes: every row carries its
//! measurement's science and method version (engine D-273), and the engine keeps no other list of them.
//! So a build's versions are read from its own newest ordinary revision of each layer (a day block's
//! base and day-local revisions, an ordered pass's history revision; never a republish's, whose carried
//! rows keep the versions they were computed under): each measurement's set of (science, method)
//! labels. A published day differs where a measurement of a layer it has published carries other labels
//! (or none: a measurement the new build added). Days with unfinished day blocks or republishes are
//! being computed and are left alone; so are days before `VYREON_CORE_RECOMPUTE_FROM`.
//!
//! **The plan** groups the days by the measurements that differ and asks the C-074 planner for each
//! group (`republish::plan_days`): SCOPED when cheaper, or on days past the prints' retention (the old
//! days; a scope that reads the prints is no faithful recompute there and is left to the owner), FULL
//! day blocks otherwise or across profile revisions, ordered passes for history-only differences.
//!
//! **Its cost against the ceiling**: the planner's measured node-hours at the pools' list price
//! (`RecomputeWatch::node_hour_usd`). It is scheduled only when the spend known now (C-078) plus that
//! cost stays within the ceiling; otherwise it is held and the owner is told why on the alerts channel.
//! What was decided goes to the progress channel (measurements, days, scoped or full, node-hours). Once
//! per build and profile for its base and day-local layers, and once for its history layer when its
//! first pass has run (`core.recompute_plan`, 0061). `vyreon-core recompute <profile> <build> --plan`
//! shows the same plan without scheduling anything.
use crate::republish::{self, Choice, Plan};
use crate::spend::{self, Spend, dollars};
use crate::watch::one_line;
use crate::{notice, work};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tokio_postgres::{Client, GenericClient};

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// What the automatic recompute planning is told (`VYREON_CORE_RECOMPUTE_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecomputeWatch {
    /// The price of a worker node-hour, USD (the pools' list price), for the plan's cost against the ceiling.
    pub node_hour_usd: f64,
    /// The days a full recompute's day block covers.
    pub block: i64,
    /// The first session planned (`VYREON_CORE_RECOMPUTE_FROM`); None: every published day.
    pub from: Option<String>,
}

/// A revision's layer, in SQL, from `d`'s columns.
const LAYER: &str = "CASE WHEN d.base_revision IS NULL THEN 'base' WHEN d.day_local THEN 'day-local' ELSE 'history' END";

/// Per layer, per measurement, the labels ("<science> | <method>").
pub type Versions = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

/// The build's own versions, per layer it has published an ordinary revision of (its newest).
pub async fn reference(c: &impl GenericClient, profile: &str, build: &str) -> Result<Versions, String> {
    let rows = c
        .query(
            &format!(
                "WITH r AS (SELECT d.revision_id, d.represented_date, {LAYER} AS layer FROM vyreon.day_revision d
                            WHERE d.engine_build = $1 AND d.profile_id = $2 AND d.state IN ('PUBLISHED', 'SUPERSEDED') AND d.republish_scope IS NULL),
                      n AS (SELECT DISTINCT ON (layer) revision_id, represented_date, layer FROM r ORDER BY layer, revision_id DESC)
                 SELECT n.layer, m.measurement_id, array_agg(DISTINCT m.science_version || ' | ' || m.method_version)
                 FROM n JOIN vyreon.measurement m ON m.revision_id = n.revision_id AND m.represented_date = n.represented_date
                 GROUP BY 1, 2"
            ),
            &[&build, &profile],
        )
        .await
        .map_err(db)?;
    let mut out = Versions::new();
    for r in rows {
        let labels: Vec<String> = r.get(2);
        out.entry(r.get(0)).or_default().insert(r.get(1), labels.into_iter().collect());
    }
    Ok(out)
}

/// One set of days the same measurements differ on, and its plan.
#[derive(Debug, Clone)]
pub struct Group {
    /// The measurements whose versions differ (the republish's scope).
    pub names: Vec<String>,
    pub days: Vec<String>,
    /// The planner's plan; None when it could not plan (`why` says why: nothing is scheduled for it).
    pub plan: Option<Plan>,
    pub choice: Option<Choice>,
    /// The planner's reason, or why nothing can be scheduled.
    pub why: String,
    /// False when the owner must decide (no faithful recompute, no plan).
    pub schedulable: bool,
    /// The chosen way's estimated node-seconds (ordered passes: none counted, minutes each, no node of their own).
    pub seconds: f64,
}

/// What a recompute after `build` would do.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub build: String,
    pub profile: String,
    /// The layers compared.
    pub layers: Vec<String>,
    /// The layers asked for whose versions the build has not published yet.
    pub waiting: Vec<String>,
    pub groups: Vec<Group>,
    /// Days with unfinished day blocks or republishes, left alone (being computed).
    pub busy: Vec<String>,
    /// Published days before `from`, left alone.
    pub before: i64,
}

impl Proposal {
    pub fn days(&self) -> usize {
        self.groups.iter().map(|g| g.days.len()).sum()
    }
    pub fn seconds(&self) -> f64 {
        self.groups.iter().filter(|g| g.schedulable).map(|g| g.seconds).sum()
    }
}

/// The plan of recomputing, under `build`, the published days of `profile` whose versions in `layers`
/// differ from the build's own.
pub async fn propose(c: &impl GenericClient, profile: &str, build: &str, layers: &[&str], from: Option<&str>, block: i64, today: &str) -> Result<Proposal, String> {
    let reference = reference(c, profile, build).await?;
    let compared: Vec<String> = layers.iter().filter(|l| reference.contains_key(**l)).map(|l| (*l).to_owned()).collect();
    let waiting: Vec<String> = layers.iter().filter(|l| !reference.contains_key(**l)).map(|l| (*l).to_owned()).collect();
    let mut p = Proposal { build: build.into(), profile: profile.into(), layers: compared.clone(), waiting, groups: Vec::new(), busy: Vec::new(), before: 0 };
    if compared.is_empty() {
        return Ok(p);
    }
    p.before = c
        .query_one(
            "SELECT count(DISTINCT represented_date) FROM vyreon.day_revision
             WHERE state = 'PUBLISHED' AND profile_id = $1 AND $2::text IS NOT NULL AND represented_date < $2::text::date",
            &[&profile, &from],
        )
        .await
        .map_err(db)?
        .get(0);
    // the published layers of each day, and their measurements' labels
    let published: Vec<(String, String)> = c
        .query(
            &format!(
                "SELECT d.represented_date::text, {LAYER} FROM vyreon.day_revision d
                 WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND ($2::text IS NULL OR d.represented_date >= $2::text::date) AND {LAYER} = ANY($3)"
            ),
            &[&profile, &from, &compared],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let rows = c
        .query(
            &format!(
                "SELECT d.represented_date::text, {LAYER}, m.measurement_id, array_agg(DISTINCT m.science_version || ' | ' || m.method_version)
                 FROM vyreon.day_revision d JOIN vyreon.measurement m ON m.revision_id = d.revision_id AND m.represented_date = d.represented_date
                 WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND ($2::text IS NULL OR d.represented_date >= $2::text::date) AND {LAYER} = ANY($3)
                 GROUP BY 1, 2, 3"
            ),
            &[&profile, &from, &compared],
        )
        .await
        .map_err(db)?;
    let mut labels: BTreeMap<(String, String, String), BTreeSet<String>> = BTreeMap::new();
    for r in rows {
        let l: Vec<String> = r.get(3);
        labels.insert((r.get(0), r.get(1), r.get(2)), l.into_iter().collect());
    }
    let busy: BTreeSet<String> = c
        .query(
            "SELECT DISTINCT s.session::text FROM core.job j JOIN core.session s ON s.session BETWEEN COALESCE(j.next_session, j.first_session) AND j.last_session
             WHERE j.kind IN ('DAY_BLOCK', 'REPUBLISH') AND j.profile_id = $1 AND j.state IN ('QUEUED', 'LEASED', 'FAILED')",
            &[&profile],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let empty = BTreeSet::new();
    let mut differ: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (day, layer) in &published {
        for (m, want) in &reference[layer] {
            let have = labels.get(&(day.clone(), layer.clone(), m.clone())).unwrap_or(&empty);
            if have != want {
                differ.entry(day.clone()).or_default().insert(m.clone());
            }
        }
    }
    let mut groups: BTreeMap<BTreeSet<String>, Vec<String>> = BTreeMap::new();
    for (day, names) in differ {
        if busy.contains(&day) {
            p.busy.push(day);
        } else {
            groups.entry(names).or_default().push(day);
        }
    }
    for (names, days) in groups {
        let names: Vec<String> = names.into_iter().collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let g = match republish::plan_days(c, profile, build, &refs, &days, block, today).await {
            Ok(plan) => {
                let faithful = !(plan.choice == Choice::Full && plan.why.starts_with("no faithful recompute"));
                let seconds = match plan.choice {
                    Choice::Scoped => plan.scoped_total(),
                    Choice::Full => plan.full_total(),
                    Choice::Passes => 0.0,
                };
                let why = if faithful { plan.why.clone() } else { format!("left to the owner: {}", plan.why) };
                Group { names, days, choice: Some(plan.choice), why, schedulable: faithful, seconds, plan: Some(plan) }
            }
            Err(e) => Group { names, days, plan: None, choice: None, why: format!("not planned: {e}"), schedulable: false, seconds: 0.0 },
        };
        p.groups.push(g);
    }
    Ok(p)
}

/// "a, b, c +2 more".
fn names_words(names: &[String], max: usize) -> String {
    let mut out = names.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > max {
        out.push_str(&format!(" +{} more", names.len() - max));
    }
    out
}

fn hours(seconds: f64) -> String {
    format!("{:.1} node-h", seconds / 3600.0)
}

/// How many groups a description lists one by one.
pub const GROUPS_SHOWN: usize = 12;

/// The proposal as the operator and the progress channel read it: a headline, then one line a group.
pub fn describe(p: &Proposal, node_hour_usd: f64) -> Vec<String> {
    let commit = p.build.split('+').next().unwrap_or(&p.build);
    if p.layers.is_empty() {
        return vec![format!("Build {commit} has published none of its {} revisions yet: nothing compared", p.waiting.join(", "))];
    }
    let mut out = vec![format!(
        "Build {commit}'s methods ({} compared) differ on {} published days{}",
        p.layers.join(", "),
        p.days(),
        if p.groups.is_empty() { ": nothing to recompute" } else { "" }
    )];
    for g in p.groups.iter().take(GROUPS_SHOWN) {
        let way = match (g.choice, &g.plan) {
            (Some(Choice::Scoped), _) if g.schedulable => format!("SCOPED, one job a day, {}", hours(g.seconds)),
            (Some(Choice::Full), Some(plan)) if g.schedulable => format!("FULL, day blocks over {} runs of days, {}", plan.runs().len(), hours(g.seconds)),
            (Some(Choice::Passes), _) => "ordered passes (minutes each)".to_owned(),
            _ => "not scheduled".to_owned(),
        };
        let span = match (g.days.first(), g.days.last()) {
            (Some(a), Some(b)) if a != b => format!("{a}..{b}"),
            (Some(a), _) => a.clone(),
            _ => String::new(),
        };
        let unsaid = if g.schedulable { String::new() } else { format!(" — {}", one_line(&g.why, 160)) };
        out.push(format!(" · [{}] on {} days ({span}): {way}{unsaid}", names_words(&g.names, 6), g.days.len()));
    }
    if p.groups.len() > GROUPS_SHOWN {
        out.push(format!(" · and {} more groups of days", p.groups.len() - GROUPS_SHOWN));
    }
    if !p.groups.is_empty() {
        let s = p.seconds();
        out.push(format!(" ≈ {} ≈ {} at {}/node-h (the planner's measured times)", hours(s), dollars(s / 3600.0 * node_hour_usd), dollars(node_hour_usd)));
    }
    if !p.busy.is_empty() {
        out.push(format!(" {} days being computed (unfinished day blocks or republishes) left alone", p.busy.len()));
    }
    if p.before > 0 {
        out.push(format!(" {} published days before VYREON_CORE_RECOMPUTE_FROM left alone", p.before));
    }
    if !p.waiting.is_empty() {
        out.push(format!(" {} not compared yet: the build has published no such revision", p.waiting.join(", ")));
    }
    out
}

/// The reason each scheduled job carries.
fn reason(p: &Proposal, g: &Group) -> String {
    let commit = p.build.split('+').next().unwrap_or(&p.build);
    one_line(&format!("C-079: method versions of build {commit} differ ({})", names_words(&g.names, 8)), 200)
}

/// What `decide` did.
#[derive(Debug, Clone, PartialEq)]
pub struct Decided {
    /// SCHEDULED, HELD, NOTHING or FAILED (`core.recompute_plan`).
    pub outcome: &'static str,
    pub why: Option<String>,
    /// Jobs queued.
    pub jobs: u64,
}

/// Why the proposal may not be scheduled by itself now, if it may not: the ceiling (`spend` None: not
/// watched; its spend not known: not applied; spent plus the plan's cost past it).
async fn held_by_ceiling(c: &impl GenericClient, p: &Proposal, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<String>, String> {
    let cost = p.seconds() / 3600.0 * w.node_hour_usd;
    Ok(match watch {
        None => Some("the spend is not watched (VYREON_CORE_SPEND_SINCE is not set), so the ceiling cannot be applied".to_owned()),
        Some(sw) => match spend::spend(c, sw, now).await? {
            Spend::Unread(why) => Some(format!("the spend is not known ({}), so the ceiling cannot be applied", one_line(&why, 160))),
            Spend::Known(k) if k.spent + cost > k.ceiling => {
                Some(format!("{} spent and {} more would pass the {} ceiling", dollars(k.spent), dollars(cost), dollars(k.ceiling)))
            }
            Spend::Known(_) => None,
        },
    })
}

/// Schedules the proposal when its cost keeps the spend within the ceiling, records it once
/// (`core.recompute_plan`) and says so: on the progress channel always, on the alerts channel when the
/// owner must decide. None when it was recorded before (another tick or the command).
pub async fn decide(client: &mut Client, p: &Proposal, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<Decided>, String> {
    // one decision at a time (the watch's tick, the command), so nothing is scheduled twice; the lock is
    // the session's, given back whatever the outcome
    const LOCK: &str = "hashtextextended('core.recompute_plan', 0)";
    let got: bool = client.query_one(&format!("SELECT pg_try_advisory_lock({LOCK})"), &[]).await.map_err(db)?.get(0);
    if !got {
        return Ok(None);
    }
    let out = decide_locked(client, p, w, watch, now).await;
    client.execute(&format!("SELECT pg_advisory_unlock({LOCK})"), &[]).await.map_err(db)?;
    out
}

async fn decide_locked(client: &mut Client, p: &Proposal, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<Decided>, String> {
    let layers = p.layers.join(",");
    let recorded = client
        .query_opt("SELECT FROM core.recompute_plan WHERE engine_build = $1 AND profile_id = $2 AND layers = $3", &[&p.build, &p.profile, &layers])
        .await
        .map_err(db)?
        .is_some();
    if recorded || p.layers.is_empty() {
        return Ok(None);
    }
    let schedulable = p.groups.iter().any(|g| g.schedulable);
    let held = if schedulable { held_by_ceiling(&*client, p, w, watch, now).await? } else { None };
    let mut jobs = 0u64;
    let mut failed: Option<String> = None;
    let mut queued: Vec<Value> = Vec::new();
    if held.is_none() {
        for g in p.groups.iter().filter(|g| g.schedulable) {
            let Some(plan) = &g.plan else { continue };
            let why = reason(p, g);
            let made: Result<u64, String> = match plan.choice {
                Choice::Scoped => republish::schedule(client, &p.profile, &p.build, &plan.work, &plan.sessions, &why).await.map(|s| s.jobs.len() as u64),
                Choice::Full => {
                    let mut n = 0;
                    let mut out = Ok(());
                    for (a, b) in plan.runs() {
                        match work::schedule(client, work::Kind::DayBlock, &p.profile, &p.build, &a, &b, w.block as i32).await {
                            Ok(s) => n += s.jobs,
                            Err(e) => {
                                out = Err(e);
                                break;
                            }
                        }
                    }
                    match out {
                        Ok(()) => notice::log(&*client, &format!("Full recompute of {} sessions under build {} queued as {n} day blocks ({why}).", plan.sessions.len(), p.build))
                            .await
                            .map(|()| n),
                        Err(e) => Err(e),
                    }
                }
                Choice::Passes => {
                    let mut n = 0;
                    let mut out = Ok(n);
                    for (a, b) in plan.runs() {
                        match work::schedule(client, work::Kind::OrderedPass, &p.profile, &p.build, &a, &b, 1).await {
                            Ok(s) => {
                                n += s.jobs;
                                out = Ok(n);
                            }
                            Err(e) => {
                                out = Err(e);
                                break;
                            }
                        }
                    }
                    out
                }
            };
            match made {
                Ok(n) => {
                    jobs += n;
                    queued.push(json!({"names": g.names, "days": g.days.len(), "choice": plan.choice.name(), "jobs": n}));
                }
                Err(e) => {
                    failed = Some(format!("[{}]: {e}", names_words(&g.names, 4)));
                    break;
                }
            }
        }
    }
    let (outcome, why) = match (&held, &failed) {
        _ if p.groups.is_empty() => ("NOTHING", None),
        (Some(h), _) => ("HELD", Some(h.clone())),
        (None, Some(f)) => ("FAILED", Some(format!("scheduling stopped part-way: {f}"))),
        (None, None) if !schedulable => ("HELD", Some("no group of days can be scheduled by itself (see each)".to_owned())),
        (None, None) => ("SCHEDULED", None),
    };
    let lines = describe(p, w.node_hour_usd);
    let plan = json!({"describe": lines, "queued": queued, "busy": p.busy.len(), "waiting": p.waiting, "cost_usd": p.seconds() / 3600.0 * w.node_hour_usd});
    let tx = client.transaction().await.map_err(db)?;
    let n = tx
        .execute(
            "INSERT INTO core.recompute_plan (engine_build, profile_id, layers, outcome, why, plan) VALUES ($1, $2, $3, $4, $5, $6::text::jsonb)
             ON CONFLICT (engine_build, profile_id, layers) DO NOTHING",
            &[&p.build, &p.profile, &layers, &outcome, &why, &plan.to_string()],
        )
        .await
        .map_err(db)?;
    if n == 1 {
        let head = match outcome {
            "SCHEDULED" => format!("scheduled: {jobs} jobs"),
            "NOTHING" => "nothing to recompute".to_owned(),
            _ => format!("NOT scheduled: {}", why.clone().unwrap_or_default()),
        };
        let key = format!("recompute {} {} {layers}", p.build, p.profile);
        notice::progress(&tx, &key, &format!("♻️ Recompute after a method change ({head})\n{}", lines.join("\n"))).await?;
        if matches!(outcome, "HELD" | "FAILED") {
            notice::alert_once(
                &tx,
                &format!("{key} alert"),
                &format!(
                    "🔴 Recompute after build {} not scheduled: {}\nPublished numbers stay as they are, under their older methods, until recomputed.\nTo do: read the plan, then schedule it on your word (`vyreon-core republish`, or raise the ceiling): `vyreon-core recompute {} {} --plan`",
                    p.build.split('+').next().unwrap_or(&p.build),
                    why.clone().unwrap_or_default(),
                    p.profile,
                    p.build
                ),
            )
            .await?;
        }
    }
    tx.commit().await.map_err(db)?;
    Ok((n == 1).then_some(Decided { outcome, why, jobs }))
}

/// The layers of `build` whose versions it has published and no plan has compared yet: the day's two
/// layers together, once both are published; the history layer once its first pass has run.
pub async fn uncompared(c: &impl GenericClient, profile: &str, build: &str) -> Result<Vec<&'static str>, String> {
    let row = c
        .query_one(
            &format!(
                "SELECT ARRAY(SELECT DISTINCT {LAYER} FROM vyreon.day_revision d
                              WHERE d.engine_build = $2 AND d.profile_id = $1 AND d.state IN ('PUBLISHED', 'SUPERSEDED') AND d.republish_scope IS NULL),
                        ARRAY(SELECT x FROM core.recompute_plan r, unnest(string_to_array(r.layers, ',')) x WHERE r.engine_build = $2 AND r.profile_id = $1)"
            ),
            &[&profile, &build],
        )
        .await
        .map_err(db)?;
    let (known, covered): (Vec<String>, Vec<String>) = (row.get(0), row.get(1));
    let has = |l: &str| known.iter().any(|k| k == l) && !covered.iter().any(|c| c == l);
    let mut need: Vec<&'static str> = Vec::new();
    if has("base") && has("day-local") {
        need.extend(["base", "day-local"]);
    }
    if has("history") && (need.len() == 2 || covered.iter().any(|c| c == "base")) {
        need.push("history");
    }
    Ok(need)
}

/// What the watch compares next: the newest running build's `uncompared` layers, when no block of
/// another build still drains (C-077: a draining block publishes its day under the old methods).
pub async fn due(c: &impl GenericClient, profile: &str) -> Result<Option<(String, Vec<&'static str>)>, String> {
    let row = c
        .query_opt(
            "SELECT b.engine_build,
                    EXISTS (SELECT FROM core.drain d JOIN core.lease l USING (lease_id) JOIN core.job j ON j.job_id = d.job_id
                            WHERE l.ended_at IS NULL AND j.engine_build <> b.engine_build)
             FROM core.engine_build b WHERE core.build_runs(b.engine_build) ORDER BY b.admitted_at DESC LIMIT 1",
            &[],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else { return Ok(None) };
    let (build, draining): (String, bool) = (row.get(0), row.get(1));
    if draining {
        return Ok(None);
    }
    let need = uncompared(c, profile, &build).await?;
    Ok((!need.is_empty()).then_some((build, need)))
}

/// The watch's step, each minute: the recompute of what `due` finds, proposed and decided once.
pub async fn auto(client: &mut Client, profile: &str, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<Decided>, String> {
    let Some((build, need)) = due(&*client, profile).await? else { return Ok(None) };
    let today: String = client.query_one("SELECT ($1::text::timestamptz AT TIME ZONE 'UTC')::date::text", &[&now]).await.map_err(db)?.get(0);
    let p = propose(&*client, profile, &build, &need, w.from.as_deref(), w.block, &today).await?;
    decide(client, &p, w, watch, now).await
}
