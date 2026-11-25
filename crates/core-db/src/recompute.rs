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
    /// The chosen way's estimated node-seconds, the work it causes included (release-3 audit, Codex 9: a
    /// scoped day's reference-check re-run; the ordered passes at their measured time).
    pub seconds: f64,
    /// Its days are past the prints' retention (planned apart: Gemini 10).
    pub perished: bool,
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
    /// Differing days with unfinished day blocks or republishes (compare-only blocks included: a schedule
    /// over them is refused): decided again once they are free (release-3 audit, Codex 5, Gemini 11).
    pub busy: Vec<String>,
    /// Published days before `from`, left alone.
    pub before: i64,
}

impl Proposal {
    pub fn days(&self) -> usize {
        self.groups.iter().map(|g| g.days.len()).sum()
    }
    /// The differing days planned (schedulable or not).
    pub fn planned_days(&self) -> Vec<String> {
        let mut d: Vec<String> = self.groups.iter().flat_map(|g| g.days.iter().cloned()).collect();
        d.sort();
        d
    }
    pub fn seconds(&self) -> f64 {
        self.groups.iter().filter(|g| g.schedulable).map(|g| g.seconds).sum()
    }
}

/// How many recent finished leases a measured duration is the median of.
pub const MEASURED_LEASES: i64 = 20;
/// A reference-check re-run not yet measured is priced at its deadline (the engine's `RERUN_MINUTES`, 600):
/// an upper bound, never a guess.
pub const RERUN_BOUND_SECONDS: f64 = 600.0 * 60.0;
/// An ordered pass not yet measured is priced at C-066's cap (a pass past 30 minutes is raised).
pub const PASS_BOUND_SECONDS: f64 = 30.0 * 60.0;

/// The median node-seconds of the latest finished leases of `kind`, else `bound`.
async fn measured_lease(c: &impl GenericClient, kind: &str, bound: f64) -> Result<f64, String> {
    let m: Option<f64> = c
        .query_one(
            "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY s) FROM (
                 SELECT extract(epoch FROM l.ended_at - l.granted_at)::float8 AS s FROM core.lease l JOIN core.job j USING (job_id)
                 WHERE j.kind = $1 AND l.outcome = 'DONE' ORDER BY l.ended_at DESC LIMIT $2) x",
            &[&kind, &MEASURED_LEASES],
        )
        .await
        .map_err(db)?
        .get(0);
    Ok(m.filter(|x| *x > 0.0).unwrap_or(bound))
}

/// The plan of recomputing, under `build`, the published days of `profile` (`only`: of those days) whose
/// versions in `layers` differ from the build's own.
#[allow(clippy::too_many_arguments)]
pub async fn propose(c: &impl GenericClient, profile: &str, build: &str, layers: &[&str], from: Option<&str>, only: Option<&[String]>, block: i64, today: &str) -> Result<Proposal, String> {
    let reference = reference(c, profile, build).await?;
    let compared: Vec<String> = layers.iter().filter(|l| reference.contains_key(**l)).map(|l| (*l).to_owned()).collect();
    let waiting: Vec<String> = layers.iter().filter(|l| !reference.contains_key(**l)).map(|l| (*l).to_owned()).collect();
    let mut p = Proposal { build: build.into(), profile: profile.into(), layers: compared.clone(), waiting, groups: Vec::new(), busy: Vec::new(), before: 0 };
    if compared.is_empty() {
        return Ok(p);
    }
    let only: Option<Vec<String>> = only.map(<[String]>::to_vec);
    p.before = c
        .query_one(
            "SELECT count(DISTINCT represented_date) FROM vyreon.day_revision
             WHERE state = 'PUBLISHED' AND profile_id = $1 AND $2::text IS NOT NULL AND represented_date < $2::text::date
               AND ($3::text[] IS NULL OR represented_date::text = ANY($3))",
            &[&profile, &from, &only],
        )
        .await
        .map_err(db)?
        .get(0);
    // the published layers of each day, and whether the build publishes their profile revisions
    let published: Vec<(String, String, bool)> = c
        .query(
            &format!(
                "SELECT d.represented_date::text, {LAYER}, EXISTS (SELECT FROM core.expected_slot e WHERE e.engine_build = $4 AND e.profile_revision = d.profile_revision)
                 FROM vyreon.day_revision d
                 WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND ($2::text IS NULL OR d.represented_date >= $2::text::date) AND {LAYER} = ANY($3)
                   AND ($5::text[] IS NULL OR d.represented_date::text = ANY($5))"
            ),
            &[&profile, &from, &compared, &build, &only],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let rows = c
        .query(
            &format!(
                "SELECT d.represented_date::text, {LAYER}, m.measurement_id, array_agg(DISTINCT m.science_version || ' | ' || m.method_version)
                 FROM vyreon.day_revision d JOIN vyreon.measurement m ON m.revision_id = d.revision_id AND m.represented_date = d.represented_date
                 WHERE d.state = 'PUBLISHED' AND d.profile_id = $1 AND ($2::text IS NULL OR d.represented_date >= $2::text::date) AND {LAYER} = ANY($3)
                   AND ($4::text[] IS NULL OR d.represented_date::text = ANY($4))
                 GROUP BY 1, 2, 3"
            ),
            &[&profile, &from, &compared, &only],
        )
        .await
        .map_err(db)?;
    let mut labels: BTreeMap<(String, String), BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for r in rows {
        let l: Vec<String> = r.get(3);
        labels.entry((r.get(0), r.get(1))).or_default().insert(r.get(2), l.into_iter().collect());
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
    let empty = BTreeMap::new();
    let mut differ: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (day, layer, readable) in &published {
        let want = &reference[layer];
        let have = labels.get(&(day.clone(), layer.clone())).unwrap_or(&empty);
        let names = differ.entry(day.clone()).or_default();
        if !*readable {
            // (release-3 audit, Gemini 17) a layer in a profile revision the build does not publish: the whole
            // layer is recomputed (the planner makes it FULL, or the owner's where the prints are gone)
            names.extend(want.keys().cloned());
            continue;
        }
        for (m, labels) in want {
            if have.get(m) != Some(labels) {
                names.insert(m.clone());
            }
        }
        // a measurement the build no longer publishes: the day differs by it (the planner says what it can do)
        names.extend(have.keys().filter(|m| !want.contains_key(*m)).cloned());
    }
    differ.retain(|_, n| !n.is_empty());
    let days: Vec<String> = differ.keys().cloned().collect();
    let perished: BTreeSet<String> = c
        .query("SELECT d FROM unnest($1::text[]) d WHERE core.prints_perished(d::date, $2::text::date)", &[&days, &today])
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    // (release-3 audit, Gemini 10) days past the prints' retention are planned apart from the others: neither
    // decides the other's way
    let mut groups: BTreeMap<(BTreeSet<String>, bool), Vec<String>> = BTreeMap::new();
    for (day, names) in differ {
        if busy.contains(&day) {
            p.busy.push(day);
        } else {
            let gone = perished.contains(&day);
            groups.entry((names, gone)).or_default().push(day);
        }
    }
    let rerun = measured_lease(c, "REFCHECK", RERUN_BOUND_SECONDS).await?;
    let pass = measured_lease(c, "ORDERED_PASS", PASS_BOUND_SECONDS).await?;
    for ((names, gone), days) in groups {
        let names: Vec<String> = names.into_iter().collect();
        let g = match republish::plan_days(c, profile, build, &names, &days, block, today).await {
            Ok(plan) => {
                let faithful = !(plan.choice == Choice::Full && plan.why.starts_with("no faithful recompute"));
                let n = plan.sessions.len() as f64;
                let seconds = match plan.choice {
                    // a republished day's current revisions get a reference-check re-run (C-074 ruling 1)
                    Choice::Scoped => plan.scoped_total() + n * rerun,
                    Choice::Full => plan.full_total(),
                    Choice::Passes => n * pass,
                };
                let why = if faithful { plan.why.clone() } else { format!("left to the owner: {}", plan.why) };
                Group { names, days, choice: Some(plan.choice), why, schedulable: faithful, seconds, perished: gone, plan: Some(plan) }
            }
            Err(e) => Group { names, days, plan: None, choice: None, why: format!("not planned: {e}"), schedulable: false, seconds: 0.0, perished: gone },
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
            (Some(Choice::Scoped), _) if g.schedulable => format!("SCOPED, one job a day and its check's re-run, {}", hours(g.seconds)),
            (Some(Choice::Full), Some(plan)) if g.schedulable => format!("FULL, day blocks over {} runs of days, {}", plan.runs().len(), hours(g.seconds)),
            (Some(Choice::Passes), _) => format!("ordered passes, {}", hours(g.seconds)),
            _ => "not scheduled".to_owned(),
        };
        let span = match (g.days.first(), g.days.last()) {
            (Some(a), Some(b)) if a != b => format!("{a}..{b}"),
            (Some(a), _) => a.clone(),
            _ => String::new(),
        };
        let unsaid = if g.schedulable { String::new() } else { format!(" — {}", one_line(&g.why, 160)) };
        let gone = if g.perished { ", prints no longer served" } else { "" };
        out.push(format!(" · [{}] on {} days ({span}{gone}): {way}{unsaid}", names_words(&g.names, 6), g.days.len()));
    }
    if p.groups.len() > GROUPS_SHOWN {
        out.push(format!(" · and {} more groups of days", p.groups.len() - GROUPS_SHOWN));
    }
    if !p.groups.is_empty() {
        let s = p.seconds();
        out.push(format!(" ≈ {} ≈ {} at {}/node-h (the planner's measured times)", hours(s), dollars(s / 3600.0 * node_hour_usd), dollars(node_hour_usd)));
    }
    if !p.busy.is_empty() {
        out.push(format!(" {} days being computed (unfinished day blocks or republishes): compared again once they are done", p.busy.len()));
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

/// The cost of plans already scheduled and not yet in DigitalOcean's figure as of `asof` (release-3 audit,
/// Codex 10): each SCHEDULED or FAILED plan whose jobs are unfinished, or finished after it, counted whole.
pub async fn reserved(c: &impl GenericClient, asof: &str) -> Result<f64, String> {
    Ok(c
        .query_one(
            "SELECT COALESCE(sum(p.cost_usd), 0)::float8 FROM core.recompute_plan p
             WHERE p.outcome IN ('SCHEDULED', 'FAILED')
               AND EXISTS (SELECT FROM core.job j WHERE j.scope ? 'recompute_plan' AND (j.scope->>'recompute_plan')::bigint = p.plan_id
                             AND (j.state IN ('QUEUED', 'LEASED', 'FAILED') OR j.finished_at > $1::text::timestamptz))",
            &[&asof],
        )
        .await
        .map_err(db)?
        .get(0))
}

/// Why the proposal may not be scheduled by itself now, if it may not: the ceiling (`spend` None: not
/// watched; its spend not known: not applied; the spend now, what earlier plans reserve and this plan's
/// cost past it).
async fn held_by_ceiling(c: &impl GenericClient, p: &Proposal, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<String>, String> {
    let cost = p.seconds() / 3600.0 * w.node_hour_usd;
    Ok(match watch {
        None => Some("the spend is not watched (VYREON_CORE_SPEND_SINCE is not set), so the ceiling cannot be applied".to_owned()),
        Some(sw) => match spend::spend(c, sw, now).await? {
            Spend::Unread(why) => Some(format!("the spend is not known ({}), so the ceiling cannot be applied", one_line(&why, 160))),
            Spend::Known(k) => {
                let held = reserved(c, &k.asof_at).await?;
                (k.now + held + cost > k.ceiling).then(|| {
                    format!(
                        "{} spent, {} reserved by plans not yet billed and {} more would pass the {} ceiling",
                        dollars(k.now),
                        dollars(held),
                        dollars(cost),
                        dollars(k.ceiling)
                    )
                })
            }
        },
    })
}

/// Schedules the proposal when its cost keeps the spend within the ceiling, records the decision
/// (`core.recompute_plan`) and says so: on the progress channel when it differs from the last decision of
/// these layers, on the alerts channel when the owner must decide (held, failed, or groups left to him).
/// None while another decision runs.
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
    out.map(Some)
}

async fn decide_locked(client: &mut Client, p: &Proposal, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Decided, String> {
    let layers = p.layers.join(",");
    let schedulable = p.groups.iter().any(|g| g.schedulable);
    let held = if schedulable { held_by_ceiling(&*client, p, w, watch, now).await? } else { None };
    let cost = p.seconds() / 3600.0 * w.node_hour_usd;
    let ceiling = watch.map(|s| s.ceiling_usd);
    let (outcome, why): (&'static str, Option<String>) = match &held {
        _ if p.groups.is_empty() => ("NOTHING", None),
        Some(h) => ("HELD", Some(h.clone())),
        None if !schedulable => ("HELD", Some("no group of days can be scheduled by itself (see each)".to_owned())),
        None => ("SCHEDULED", None),
    };
    let previous: Option<(String, Option<String>)> = client
        .query_opt(
            "SELECT outcome, why FROM core.recompute_plan WHERE engine_build = $1 AND profile_id = $2 AND layers = $3 ORDER BY plan_id DESC LIMIT 1",
            &[&p.build, &p.profile, &layers],
        )
        .await
        .map_err(db)?
        .map(|r| (r.get(0), r.get(1)));
    let lines = describe(p, w.node_hour_usd);
    let days = p.planned_days();
    let plan_id: i64 = client
        .query_one(
            "INSERT INTO core.recompute_plan (engine_build, profile_id, layers, outcome, why, plan, days, busy, cost_usd, ceiling_usd)
             VALUES ($1, $2, $3, $4, $5, $6::text::jsonb, $7, $8, $9, $10) RETURNING plan_id",
            &[&p.build, &p.profile, &layers, &outcome, &why, &json!({"describe": lines}).to_string(), &days, &p.busy, &cost, &ceiling],
        )
        .await
        .map_err(db)?
        .get(0);
    let mut jobs = 0u64;
    let mut failed: Option<String> = None;
    let mut queued: Vec<Value> = Vec::new();
    if outcome == "SCHEDULED" {
        let tag = json!(plan_id);
        for g in p.groups.iter().filter(|g| g.schedulable) {
            let Some(plan) = &g.plan else { continue };
            let why = reason(p, g);
            // (release-3 audit, Codex 18) what each run queued counts, also when a later one fails
            let mut n = 0u64;
            let made: Result<(), String> = match plan.choice {
                Choice::Scoped => republish::schedule_tagged(client, &p.profile, &p.build, &plan.work, &plan.sessions, &why, Some(&tag)).await.map(|s| n += s.jobs.len() as u64),
                Choice::Full | Choice::Passes => {
                    let (kind, size, scope) = match plan.choice {
                        // the block names what it recomputes, so a claim past the prints' retention can make
                        // its days scoped (`work::guard_perished_blocks`)
                        Choice::Full => (work::Kind::DayBlock, w.block as i32, json!({"recompute_plan": plan_id, "measurements": g.names, "reason": why})),
                        _ => (work::Kind::OrderedPass, 1, json!({"recompute_plan": plan_id})),
                    };
                    let mut out = Ok(());
                    for (a, b) in plan.runs() {
                        match work::schedule_scoped(client, kind, &p.profile, &p.build, &a, &b, size, Some(&scope)).await {
                            Ok(s) => n += s.jobs,
                            Err(e) => {
                                out = Err(e);
                                break;
                            }
                        }
                    }
                    if out.is_ok() && plan.choice == Choice::Full {
                        notice::log(&*client, &format!("Full recompute of {} sessions under build {} queued as {n} day blocks ({why}).", plan.sessions.len(), p.build)).await?;
                    }
                    out
                }
            };
            jobs += n;
            queued.push(json!({"names": g.names, "days": g.days.len(), "choice": plan.choice.name(), "jobs": n}));
            if let Err(e) = made {
                failed = Some(format!("[{}]: {e}", names_words(&g.names, 4)));
                break;
            }
        }
    }
    let (outcome, why) = match failed {
        Some(f) => ("FAILED", Some(format!("scheduling stopped part-way ({jobs} jobs queued before): {f}"))),
        None => (outcome, why),
    };
    let tx = client.transaction().await.map_err(db)?;
    tx.execute(
        "UPDATE core.recompute_plan SET outcome = $2, why = $3, plan = plan || $4::text::jsonb WHERE plan_id = $1",
        &[&plan_id, &outcome, &why, &json!({"queued": queued, "jobs": jobs}).to_string()],
    )
    .await
    .map_err(db)?;
    // the same held decision again (each hour, until the ceiling or the spend allows it) is not said again
    let same = previous.as_ref().is_some_and(|(o, w)| o == outcome && w.as_deref() == why.as_deref() && outcome == "HELD");
    if !same {
        let head = match outcome {
            "SCHEDULED" => format!("scheduled: {jobs} jobs"),
            "NOTHING" => "nothing to recompute".to_owned(),
            _ => format!("NOT scheduled: {}", why.clone().unwrap_or_default()),
        };
        notice::progress(&tx, &format!("recompute {plan_id}"), &format!("♻️ Recompute after a method change ({head})\n{}", lines.join("\n"))).await?;
    }
    let left: Vec<&Group> = p.groups.iter().filter(|g| !g.schedulable).collect();
    let alert = match outcome {
        "HELD" | "FAILED" => Some(why.clone().unwrap_or_default()),
        // (release-3 audit, Gemini 12) groups left to the owner beside those scheduled
        "SCHEDULED" if !left.is_empty() => Some(format!(
            "{} groups of days ({} days) are left to you: {}",
            left.len(),
            left.iter().map(|g| g.days.len()).sum::<usize>(),
            one_line(&left.iter().map(|g| format!("[{}] {}", names_words(&g.names, 3), g.why)).collect::<Vec<_>>().join("; "), 400)
        )),
        _ => None,
    };
    if let Some(text) = alert.filter(|_| !same) {
        notice::alert(
            &tx,
            &format!(
                "🔴 A recompute after the new version's method changes was not scheduled by itself: {text}\nPublished numbers unchanged: those days stay under the older methods until recomputed. To do: read the plan; schedule it on your word, or raise the ceiling (a held plan is decided again within the hour): `vyreon-core recompute {} {} --plan`",
                p.profile, p.build
            ),
        )
        .await?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Decided { outcome, why, jobs })
}

/// What is to be decided for `build`: (layers, only these days) — its layers no decision has compared yet
/// (the day's two together once both are published, the history once its first pass has run); else, per
/// layers, the newest decision HELD (decided again once an hour has passed and the ceiling changed or the
/// spend was read since), or its busy days of which one is free now (decided again for those).
pub async fn pending(c: &impl GenericClient, profile: &str, build: &str, ceiling: Option<f64>) -> Result<Option<(Vec<&'static str>, Option<Vec<String>>)>, String> {
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
    if !need.is_empty() {
        return Ok(Some((need, None)));
    }
    // the newest decision of each set of layers
    let rows = c
        .query(
            "SELECT DISTINCT ON (p.layers) p.layers, p.outcome, p.ceiling_usd, p.busy,
                    p.planned_at < now() - interval '1 hour'
                      AND EXISTS (SELECT FROM core.spend_reading s WHERE s.error IS NULL AND s.read_at > p.planned_at),
                    ARRAY(SELECT d FROM unnest(p.busy) d
                          WHERE NOT EXISTS (SELECT FROM core.job j WHERE j.kind IN ('DAY_BLOCK', 'REPUBLISH') AND j.profile_id = p.profile_id
                                              AND j.state IN ('QUEUED', 'LEASED', 'FAILED')
                                              AND d::date BETWEEN COALESCE(j.next_session, j.first_session) AND j.last_session))
             FROM core.recompute_plan p WHERE p.engine_build = $1 AND p.profile_id = $2 ORDER BY p.layers, p.plan_id DESC",
            &[&build, &profile],
        )
        .await
        .map_err(db)?;
    for r in rows {
        let (layers, outcome, at_ceiling, busy, read_since, free): (String, String, Option<f64>, Vec<String>, bool, Vec<String>) =
            (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
        let layers: Vec<&'static str> = ["base", "day-local", "history"].into_iter().filter(|l| layers.split(',').any(|x| x == *l)).collect();
        // (release-3 audit, Codex 11, Gemini 8) a raised ceiling, or the spend read since, decides it again
        let ceiling_changed = at_ceiling.is_some() && ceiling.is_some() && at_ceiling != ceiling;
        if outcome == "HELD" && (ceiling_changed || read_since || (at_ceiling.is_none() && ceiling.is_some())) {
            return Ok(Some((layers, None)));
        }
        if !free.is_empty() {
            // the still-busy days are carried into the new decision by `propose` itself
            let _ = busy;
            return Ok(Some((layers, Some(free))));
        }
    }
    Ok(None)
}

/// What the watch decides next: the newest running build's `pending` work, when no block of another build
/// still drains (C-077: a draining block publishes its day under the old methods).
pub async fn due(c: &impl GenericClient, profile: &str, ceiling: Option<f64>) -> Result<Option<(String, Vec<&'static str>, Option<Vec<String>>)>, String> {
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
    Ok(pending(c, profile, &build, ceiling).await?.map(|(l, only)| (build, l, only)))
}

/// The watch's step, each minute: the recompute of what `due` finds, proposed and decided.
pub async fn auto(client: &mut Client, profile: &str, w: &RecomputeWatch, watch: Option<&spend::SpendWatch>, now: &str) -> Result<Option<Decided>, String> {
    let Some((build, need, only)) = due(&*client, profile, watch.map(|s| s.ceiling_usd)).await? else { return Ok(None) };
    let today: String = client.query_one("SELECT ($1::text::timestamptz AT TIME ZONE 'UTC')::date::text", &[&now]).await.map_err(db)?.get(0);
    let mut p = propose(&*client, profile, &build, &need, w.from.as_deref(), only.as_deref(), w.block, &today).await?;
    if only.is_some() {
        // the days of the earlier decision still busy stay pending in this one
        let still: Vec<String> = client
            .query(
                "SELECT DISTINCT d FROM core.recompute_plan p, unnest(p.busy) d
                 WHERE p.plan_id = (SELECT max(plan_id) FROM core.recompute_plan WHERE engine_build = $1 AND profile_id = $2 AND layers = $3)
                   AND NOT d = ANY($4)",
                &[&build, &profile, &need.join(","), &only.clone().unwrap_or_default()],
            )
            .await
            .map_err(db)?
            .iter()
            .map(|r| r.get(0))
            .collect();
        p.busy.extend(still);
        p.busy.sort();
        p.busy.dedup();
    }
    decide(client, &p, w, watch, now).await
}
