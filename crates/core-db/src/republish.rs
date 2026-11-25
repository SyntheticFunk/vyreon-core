//! C-074: a scoped recompute (engine D-104, D-462). A change to some sensors is recomputed on each
//! day it touches as a REPUBLISH job: the families or measurements asked for and every same-day
//! dependant (the recompute set), published as copy-forward revisions of the day's base and day-local
//! layers (`publish`: every other slot copied byte for byte from the revision replaced). Its history
//! members are the ordered pass's, derived again from the store as after any publish (C-037).
//!
//! The planner decides, per range, whether the scope is cheaper recomputed alone (SCOPED) or as the
//! whole day (FULL, a day block): the recompute set and what a worker's day needs for it come from
//! the engine's plans in the build's inventory (`core.build_republish`: one per family and per
//! measurement, united here exactly as the engine unites them, `vyreon_graph::compose`); the costs
//! from what core's stored run reports measured, else the inventory's reference measurements. The
//! rule is the measured cost alone: SCOPED when its estimate for the range is below FULL's.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use tokio_postgres::{Client, GenericClient};

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// The stages a worker's day runs for a scope (engine `vyreon_graph::Stages`).
pub const STAGES: [&str; 8] = ["intents", "routes", "company", "surface_exports", "field", "field_options", "surfaces", "derive"];

/// A scope's work for one day, united from the build's per-seed plans.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Work {
    pub families: Vec<String>,
    pub measurements: Vec<String>,
    pub base: BTreeSet<String>,
    pub day_local: BTreeSet<String>,
    pub history: BTreeSet<String>,
    pub feeds: BTreeSet<String>,
    pub stages: BTreeSet<String>,
    /// The input categories the day needs, in the engine's order.
    pub categories: Vec<String>,
    /// The history sensors the scope itself asks for (release-2 audit, Codex 1): their method changes, so
    /// the history layer is derived again whatever the worker's values.
    pub seed_history: BTreeSet<String>,
}

impl Work {
    /// Whether a worker has anything to publish (else the ordered pass alone derives the scope).
    pub fn worker(&self) -> bool {
        !self.base.is_empty() || !self.day_local.is_empty()
    }
    pub fn computed(&self) -> BTreeSet<String> {
        self.base.union(&self.feeds).cloned().collect()
    }
    /// The job's `scope` (what the worker checks its own graph against).
    pub fn scope(&self, reason: &str) -> Value {
        serde_json::json!({
            "families": self.families, "measurements": self.measurements, "reason": reason,
            "recompute": {"base": self.base, "day_local": self.day_local, "history": self.history},
            "feeds": self.feeds, "stages": self.stages, "categories": self.categories, "seed_history": self.seed_history,
        })
    }
}

fn set(v: &Value) -> BTreeSet<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect()
}

/// The build's republish plans (its inventory's `republish`), or why there are none.
pub async fn plans(client: &impl GenericClient, build: &str) -> Result<Value, String> {
    client
        .query_opt("SELECT plans::text FROM core.build_republish WHERE engine_build = $1", &[&build])
        .await
        .map_err(db)?
        .map(|r| serde_json::from_str(r.get::<_, &str>(0)).map_err(|e| e.to_string()))
        .transpose()?
        .ok_or_else(|| format!("engine build {build} registered no republish plans (an engine before D-462): it cannot republish; recompute whole days"))
}

/// The scope's names read against the plans: each a family, else a measurement; refused if neither.
pub fn split_scope(plans: &Value, names: &[&str]) -> Result<(Vec<String>, Vec<String>), String> {
    let (mut families, mut measurements) = (Vec::new(), Vec::new());
    for n in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if plans["seeds"].get(format!("family:{n}")).is_some() {
            families.push(n.to_owned());
        } else if plans["seeds"].get(format!("measurement:{n}")).is_some() {
            measurements.push(n.to_owned());
        } else {
            return Err(format!("{n} is neither a family nor a measurement the build can republish (families: {})", plans["families"]));
        }
    }
    if families.is_empty() && measurements.is_empty() {
        return Err("name at least one family or measurement".into());
    }
    families.sort();
    families.dedup();
    measurements.sort();
    measurements.dedup();
    Ok((families, measurements))
}

/// The work of a scope: every seed's sets united, the feeds less what is recomputed, the stages and
/// categories as one day needs them (each is "some computed sensor needs it", so a union's is the
/// union of its seeds'; engine `a_union_of_scopes_is_the_union_of_their_work`).
pub fn compose(plans: &Value, families: &[String], measurements: &[String]) -> Result<Work, String> {
    let mut w = Work { families: families.to_vec(), measurements: measurements.to_vec(), ..Work::default() };
    let mut feeds = BTreeSet::new();
    let mut categories = BTreeSet::new();
    let seeds = families.iter().map(|f| format!("family:{f}")).chain(measurements.iter().map(|m| format!("measurement:{m}")));
    for key in seeds {
        let s = plans["seeds"].get(&key).ok_or_else(|| format!("the build's plans have no {key}"))?;
        w.base.extend(set(&s["base"]));
        w.day_local.extend(set(&s["day_local"]));
        w.history.extend(set(&s["history"]));
        feeds.extend(set(&s["feeds"]));
        categories.extend(set(&s["categories"]));
        for stage in STAGES {
            if s["stages"][stage] == true {
                w.stages.insert(stage.to_owned());
            }
        }
    }
    w.feeds = feeds.difference(&w.base).cloned().collect();
    // the history sensors asked for: named, or members of a family named
    for (id, m) in plans["measurements"].as_object().into_iter().flatten() {
        let asked = measurements.contains(id) || m["family"].as_str().is_some_and(|f| families.iter().any(|x| x == f));
        if asked && m["layer"] == "history" {
            w.seed_history.insert(id.clone());
        }
    }
    let order: Vec<String> = set_ordered(&plans["categories"]);
    w.categories = order.into_iter().filter(|c| categories.contains(c)).collect();
    Ok(w)
}

fn set_ordered(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect()
}

/// What a day costs, measured: core's stored run reports where they hold a figure (the median of the
/// latest base revisions'), else the inventory's reference measurements. Seconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measured {
    pub stages: BTreeMap<String, f64>,
    /// A day's whole download when it is a block's first day, and a later day of a block.
    pub download_first: f64,
    pub download_next: f64,
    pub plan: f64,
    pub publish: f64,
    pub surfaces_at_most: f64,
    /// Seconds of each fill category (or group run at once) where measured.
    pub fill: BTreeMap<String, f64>,
    /// Intent B's task-minutes and longest attempt per measurement.
    pub task_minutes: BTreeMap<String, (f64, f64)>,
    /// Where each figure came from.
    pub sources: Vec<String>,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(|a, b| a.total_cmp(b));
    (!v.is_empty()).then(|| if v.len() % 2 == 1 { v[v.len() / 2] } else { (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0 })
}

/// How many of the latest base revisions' reports the planner reads.
pub const REPORTS: i64 = 20;

/// The measured costs: the reference of the build's plans, overlaid by the medians of the latest
/// `REPORTS` reports of ordinary (not copy-forward) base revisions of the profile.
pub async fn measured(client: &impl GenericClient, plans: &Value, profile: &str) -> Result<Measured, String> {
    let r = &plans["timings"];
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let mut m = Measured {
        stages: r["stages"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), num(v))).collect(),
        download_first: num(&r["download_seconds"]),
        download_next: median(r["download_next_day_seconds"].as_array().into_iter().flatten().map(num).collect()).unwrap_or(num(&r["download_seconds"])),
        plan: num(&r["plan_seconds"]),
        publish: num(&r["stages"]["publish"]),
        surfaces_at_most: num(&r["surfaces_at_most_seconds"]),
        fill: r["fill_seconds"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), num(v))).collect(),
        task_minutes: r["intent_b_task_minutes"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), (num(&v[0]), num(&v[1])))).collect(),
        sources: vec![format!("reference: {}", r["source"].as_str().unwrap_or("the build's inventory"))],
    };
    let reports: Vec<Value> = client
        .query(
            "SELECT report::text FROM vyreon.day_revision
             WHERE profile_id = $1 AND base_revision IS NULL AND report IS NOT NULL AND republish_scope IS NULL AND state IN ('PUBLISHED', 'SUPERSEDED')
             ORDER BY revision_id DESC LIMIT $2",
            &[&profile, &REPORTS],
        )
        .await
        .map_err(db)?
        .iter()
        .filter_map(|row| serde_json::from_str(row.get::<_, &str>(0)).ok())
        .collect();
    let mut stages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut fill: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut tasks: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for rep in &reports {
        for s in rep["stages"].as_array().into_iter().flatten() {
            if let (Some(k), Some(v)) = (s["stage"].as_str(), s["seconds"].as_f64()) {
                stages.entry(k.to_owned()).or_default().push(v);
            }
        }
        for (k, v) in rep["fill_seconds"].as_object().into_iter().flatten() {
            if let Some(v) = v.as_f64() {
                fill.entry(k.clone()).or_default().push(v);
            }
        }
        for (k, v) in rep["task_minutes"]["intent b"].as_object().into_iter().flatten() {
            if let (Some(a), Some(b)) = (v[0].as_f64(), v[1].as_f64()) {
                tasks.entry(k.clone()).or_default().push((a, b));
            }
        }
    }
    let mut overlaid = Vec::new();
    for (k, v) in stages {
        if let Some(x) = median(v) {
            m.stages.insert(k, x);
            overlaid.push("stages");
        }
    }
    // (release-2 audit, Gemini 13b) a category's share of its reports' download, applied to a first day's
    // whole download: reports of a block's later days (shorter, the window shared) never mix with it
    let totals: Vec<f64> = reports.iter().map(|rep| rep["fill_seconds"].as_object().map_or(0.0, |o| o.values().filter_map(Value::as_f64).sum())).filter(|t| *t > 0.0).collect();
    if let Some(total) = median(totals) {
        for (k, v) in fill {
            if let Some(x) = median(v) {
                m.fill.insert(k, m.download_first * (x / total).min(1.0));
                overlaid.push("download by category (as shares)");
            }
        }
    }
    if !tasks.is_empty() {
        // (release-2 audit, Gemini 13c) merged by measurement: one a report omits keeps its reference figure
        for (k, v) in tasks {
            m.task_minutes.insert(k, (median(v.iter().map(|x| x.0).collect()).unwrap_or(0.0), median(v.iter().map(|x| x.1).collect()).unwrap_or(0.0)));
        }
        overlaid.push("intent B task-minutes");
    }
    // (release-2 audit, Gemini 13a) the publish as measured
    if let Some(p) = m.stages.get("publish") {
        m.publish = *p;
    }
    if !reports.is_empty() && !overlaid.is_empty() {
        overlaid.dedup();
        m.sources.insert(0, format!("core's reports: the medians of the latest {} base revisions' {}", reports.len(), overlaid.join(", ")));
    }
    Ok(m)
}

/// The full day's stages, in the order a day runs them (the engine's `acceptance_for_core`).
const FULL_STAGES: [&str; 14] = [
    "intent A plan", "intent A (history nodes)", "exports", "intent B plan", "intent B (T)", "export index", "company", "surface", "FIELD",
    "FIELD publish", "FIELD options", "bundle", "day-local derived", "derived bundle",
];

/// One day's estimate, each part with its seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    pub parts: Vec<(String, f64)>,
}

impl Estimate {
    pub fn total(&self) -> f64 {
        self.parts.iter().map(|(_, s)| s).sum()
    }
    fn add(&mut self, what: &str, s: f64) {
        if s > 0.0 {
            self.parts.push((what.to_owned(), s));
        }
    }
}

/// The full day (a day block's): its download, plan, every stage and its publish.
pub fn full_day(m: &Measured, first: bool) -> Estimate {
    let mut e = Estimate { parts: Vec::new() };
    e.add(if first { "download (a block's first day)" } else { "download (a block's later day)" }, if first { m.download_first } else { m.download_next });
    e.add("plan", m.plan);
    for s in FULL_STAGES {
        e.add(s, m.stages.get(s).copied().unwrap_or(0.0));
    }
    e.add("publish", m.publish);
    e
}

/// The scoped day: core's dump of the day (as long as its publish), the download less what the scope
/// does not fetch where that was measured (a category whose time is not measured saves nothing here),
/// the plan, the stages its work runs (intent B by the computed owners' share of its measured
/// task-minutes, never below their longest attempt), its bundles and publish.
pub fn scoped_day(m: &Measured, w: &Work, plans: &Value) -> Estimate {
    let mut e = Estimate { parts: Vec::new() };
    e.add("core's dump of the day", m.publish);
    let kept: BTreeSet<String> = set(&plans["kept_by_core"]);
    let saved: f64 = m
        .fill
        .iter()
        .filter(|(group, _)| group.split('+').all(|c| !w.categories.contains(&c.to_owned()) || kept.contains(c)))
        .map(|(_, s)| s)
        .sum();
    e.add("download (less the categories not fetched, where measured)", (m.download_first - saved).max(0.0));
    e.add("plan", m.plan);
    let st = |s: &str| w.stages.contains(s);
    let stage = |s: &str| m.stages.get(s).copied().unwrap_or(0.0);
    if st("surfaces") && !st("intents") {
        e.add("surfaces (alone: at most intent A's time, which hides them in a full day)", m.surfaces_at_most);
    }
    if st("intents") {
        for s in ["intent A plan", "intent A (history nodes)", "exports", "intent B plan"] {
            e.add(s, stage(s));
        }
    }
    if st("routes") {
        let total: f64 = m.task_minutes.values().map(|(t, _)| t).sum();
        let computed = w.computed();
        let mine: f64 = m.task_minutes.iter().filter(|(k, _)| computed.contains(*k)).map(|(_, (t, _))| t).sum();
        let longest = m.task_minutes.iter().filter(|(k, _)| computed.contains(*k)).map(|(_, (_, l))| *l).fold(0.0, f64::max) * 60.0;
        let wall = stage("intent B (T)");
        let scaled = if total > 0.0 { (wall * mine / total).max(longest).min(wall) } else { wall };
        e.add(&format!("intent B, restricted ({mine:.1} of {total:.1} task-min)"), scaled);
        e.add("export index", stage("export index"));
    }
    for (flag, s) in [("company", "company"), ("surface_exports", "surface"), ("field", "FIELD"), ("field", "FIELD publish"), ("field_options", "FIELD options")] {
        if st(flag) {
            e.add(s, stage(s));
        }
    }
    if st("derive") {
        e.add("day-local derived", stage("day-local derived"));
    }
    e.add("bundle", stage("bundle"));
    e.add("publish", m.publish);
    e
}

/// The planner's choice for a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Scoped,
    Full,
    /// The scope has no base or day-local member: the ordered pass alone derives it.
    Passes,
}

impl Choice {
    pub fn name(self) -> &'static str {
        match self {
            Choice::Scoped => "SCOPED",
            Choice::Full => "FULL",
            Choice::Passes => "ORDERED PASSES",
        }
    }
}

/// A plan for a range of days.
#[derive(Debug, Clone)]
pub struct Plan {
    pub work: Work,
    pub sessions: Vec<String>,
    /// Sessions of the range with no published day (nothing to republish).
    pub unpublished: Vec<String>,
    /// C-079: sessions of the range not asked for (`plan_days`): left as they are, and a run never spans them.
    pub skipped: Vec<String>,
    /// Published sessions whose revisions are of profile revisions the build does not publish.
    pub stale: Vec<String>,
    /// The slots the recompute set holds, base and day-local, and the day's base and day-local slots
    /// (all members listed), from the build's inventory.
    pub slots: (i64, i64),
    pub scoped: Estimate,
    pub full_first: Estimate,
    pub full_next: Estimate,
    /// Days a full day block covers (the range is split into blocks of this many).
    pub block: i64,
    pub measured: Measured,
    /// Sessions past the perishable inputs' retention (their prints are no longer served).
    pub perished: Vec<String>,
    pub choice: Choice,
    pub why: String,
}

impl Plan {
    /// (release-2 audit, Codex 6, Gemini 5) the published sessions in runs of consecutive sessions of the
    /// range: what FULL and the ordered passes schedule, an unpublished or omitted session left as it is.
    pub fn runs(&self) -> Vec<(String, String)> {
        let mut all: Vec<&String> = self.sessions.iter().chain(&self.unpublished).chain(&self.skipped).collect();
        all.sort();
        let mut runs: Vec<(String, String)> = Vec::new();
        let mut open: Option<(String, String)> = None;
        for s in all {
            match (self.sessions.contains(s), open.take()) {
                (true, None) => open = Some((s.clone(), s.clone())),
                (true, Some((a, _))) => open = Some((a, s.clone())),
                (false, Some(run)) => runs.push(run),
                (false, None) => {}
            }
        }
        runs.extend(open);
        runs
    }

    pub fn scoped_total(&self) -> f64 {
        self.scoped.total() * self.sessions.len() as f64
    }
    /// FULL's node-seconds: day blocks of `block` days within each run of consecutive sessions (release-3
    /// audit, Codex 8: a gap starts a new block, its first day a cold start, as `runs` schedules them).
    pub fn full_total(&self) -> f64 {
        let b = self.block.max(1);
        let all: Vec<&String> = {
            let mut a: Vec<&String> = self.sessions.iter().chain(&self.unpublished).chain(&self.skipped).collect();
            a.sort();
            a
        };
        let mut total = 0.0;
        for (first, last) in self.runs() {
            let n = all.iter().filter(|s| s.as_str() >= first.as_str() && s.as_str() <= last.as_str()).count() as i64;
            let blocks = (n + b - 1) / b;
            total += blocks as f64 * self.full_first.total() + (n - blocks) as f64 * self.full_next.total();
        }
        total
    }
}

/// The plan of republishing `names` (families or measurements) under `build` over `first..=last`:
/// the recompute set, its share of the day's slots, the inputs and stages a day needs, the measured
/// cost each way, and the choice. `block`: the days a full recompute's day block covers.
#[allow(clippy::too_many_arguments)]
pub async fn plan(client: &impl GenericClient, profile: &str, build: &str, names: &[&str], first: &str, last: &str, block: i64, today: &str) -> Result<Plan, String> {
    plan_in(client, profile, build, Scope::Names(names), first, last, None, block, today).await
}

/// C-079: `plan` over the listed sessions only (each a session; their runs of consecutive sessions are
/// what FULL and the passes schedule).
/// `measurements` are measurements, never read as families (release-3 audit, Codex 6: a measurement named
/// as its family, DLM-TECHNICAL, would recompute the family's seven).
pub async fn plan_days(client: &impl GenericClient, profile: &str, build: &str, measurements: &[String], sessions: &[String], block: i64, today: &str) -> Result<Plan, String> {
    let (Some(first), Some(last)) = (sessions.iter().min(), sessions.iter().max()) else {
        return Err("no session to plan".into());
    };
    plan_in(client, profile, build, Scope::Measurements(measurements), first, last, Some(sessions), block, today).await
}

/// What a plan recomputes: names read as families first (the command's), or measurements only.
enum Scope<'a> {
    Names(&'a [&'a str]),
    Measurements(&'a [String]),
}

#[allow(clippy::too_many_arguments)]
async fn plan_in(client: &impl GenericClient, profile: &str, build: &str, scope: Scope<'_>, first: &str, last: &str, only: Option<&[String]>, block: i64, today: &str) -> Result<Plan, String> {
    if !core_contract::is_date(first) || !core_contract::is_date(last) || !core_contract::is_date(today) {
        return Err(format!("{first} to {last}: dates are written YYYY-MM-DD"));
    }
    let plans = plans(client, build).await?;
    let (families, measurements) = match scope {
        Scope::Names(names) => split_scope(&plans, names)?,
        Scope::Measurements(m) => (Vec::new(), m.to_vec()),
    };
    let work = compose(&plans, &families, &measurements)?;
    let rows = client
        .query(
            "SELECT s.session::text, core.day_published(s.session, $1), core.day_published(s.session, $1, $4), core.prints_perished(s.session, $5::text::date, $6)
             FROM core.session s WHERE s.session BETWEEN $2::text::date AND $3::text::date ORDER BY 1",
            &[&profile, &first, &last, &build, &today, &(plans["perishable_sessions"].as_i64().unwrap_or(5) as i32)],
        )
        .await
        .map_err(db)?;
    let asked = |r: &tokio_postgres::Row| only.is_none_or(|o| o.iter().any(|s| s == r.get::<_, &str>(0)));
    let skipped: Vec<String> = rows.iter().filter(|r| !asked(r)).map(|r| r.get(0)).collect();
    let rows: Vec<_> = rows.into_iter().filter(|r| asked(r)).collect();
    if rows.is_empty() {
        return Err(format!("no trading session from {first} to {last}"));
    }
    let sessions: Vec<String> = rows.iter().filter(|r| r.get::<_, bool>(1)).map(|r| r.get(0)).collect();
    let unpublished: Vec<String> = rows.iter().filter(|r| !r.get::<_, bool>(1)).map(|r| r.get(0)).collect();
    // (release-2 audit, Gemini 3) published, but not in the profile revisions this build publishes: a
    // copy-forward cannot cross a profile revision; those days are recomputed whole
    let stale: Vec<String> = rows.iter().filter(|r| r.get::<_, bool>(1) && !r.get::<_, bool>(2)).map(|r| r.get(0)).collect();
    // (release-2 audit, Gemini 6) the prints' capture covers a session from its first date only: an earlier
    // day never read prints, so none perished there
    let perished_days: BTreeSet<String> = rows.iter().filter(|r| r.get::<_, bool>(1) && r.get::<_, bool>(3)).map(|r| r.get(0)).collect();
    let recomputed: Vec<&String> = work.base.iter().chain(&work.day_local).collect();
    let slots = client
        .query_one(
            "SELECT count(*) FILTER (WHERE e.measurement_id = ANY($2)), count(*) FROM core.expected_slot e JOIN core.build_profile p
               ON p.engine_build = e.engine_build AND p.profile_revision = e.profile_revision
             WHERE e.engine_build = $1 AND p.layer IN ('base', 'day-local')",
            &[&build, &recomputed],
        )
        .await
        .map_err(db)?;
    let slots: (i64, i64) = (slots.get(0), slots.get(1));
    let measured = measured(client, &plans, profile).await?;
    // the perishable inputs (engine D-103, D-360): `core.prints_perished`, as the claim reads it
    let perished: Vec<String> = sessions.iter().filter(|s| perished_days.contains(*s)).cloned().collect();
    let scoped = scoped_day(&measured, &work, &plans);
    let (full_first, full_next) = (full_day(&measured, true), full_day(&measured, false));
    let mut p = Plan { work, sessions, unpublished, skipped, stale, slots, scoped, full_first, full_next, block: block.max(1), measured, perished, choice: Choice::Full, why: String::new() };
    let (s, f) = (p.scoped_total(), p.full_total());
    let perishable: Vec<String> = set(&plans["perishable"]).into_iter().filter(|c| p.work.categories.contains(c)).collect();
    // (release-3 audit, Codex 15) a day both past the prints' retention and in other profile revisions has
    // no faithful recompute: a copy-forward cannot cross the revisions and a full one would withhold the prints'
    let stale_perished: Vec<&String> = p.stale.iter().filter(|s| p.perished.contains(s)).collect();
    (p.choice, p.why) = if !p.work.worker() {
        (Choice::Passes, "the recompute set holds history sensors only: the ordered pass derives them again from the store, with no input fetched".into())
    } else if let Some(first) = stale_perished.first() {
        (
            Choice::Full,
            format!(
                "no faithful recompute of {} of the sessions ({first}…): published in profile revisions build {build} does not publish, so a \
                 copy-forward cannot cross them, and past the prints' retention, so a full recompute would publish [{}] withheld",
                stale_perished.len(),
                set(&plans["perishable"]).into_iter().collect::<Vec<_>>().join(",")
            ),
        )
    } else if !p.stale.is_empty() {
        (
            Choice::Full,
            format!(
                "{} of the sessions are published in profile revisions build {build} does not publish ({}…): a copy-forward cannot cross a profile \
                 revision, so they are recomputed whole",
                p.stale.len(),
                p.stale[0]
            ),
        )
    } else if !perishable.is_empty() && !p.perished.is_empty() {
        (
            Choice::Full,
            format!(
                "no faithful recompute of {} of the sessions: the scope reads [{}], which the provider no longer serves for them (engine D-103; the \
                 bucket's archive is not read back), so a scoped recompute and a full one alike would publish those sensors withheld; narrow the \
                 scope or the range (`--full` recomputes them whole anyway)",
                p.perished.len(),
                perishable.join(",")
            ),
        )
    } else if !p.perished.is_empty() {
        (
            Choice::Scoped,
            format!(
                "a full recompute of {} of the sessions would publish the sensors that read [{}] withheld (no longer served, engine D-103) where \
                 the day holds their readings; the scoped one carries them forward (scoped {}, full {}, measured)",
                p.perished.len(),
                set(&plans["perishable"]).into_iter().collect::<Vec<_>>().join(","),
                hours(s),
                hours(f)
            ),
        )
    } else if s < f {
        (Choice::Scoped, format!("scoped {} is below full {} for the range (measured)", hours(s), hours(f)))
    } else {
        (Choice::Full, format!("scoped {} is not below full {} for the range (measured): its stages are the whole day's", hours(s), hours(f)))
    };
    Ok(p)
}

/// Seconds as node-hours, one decimal.
pub fn hours(s: f64) -> String {
    format!("{:.1} node-h", s / 3600.0)
}

/// The plan as the operator reads it (`vyreon-core republish --plan`).
pub fn describe(p: &Plan) -> Vec<String> {
    let list = |s: &BTreeSet<String>| if s.is_empty() { "none".to_owned() } else { s.iter().cloned().collect::<Vec<_>>().join(",") };
    let w = &p.work;
    let mut out = vec![
        format!(
            "scope: families [{}] measurements [{}]; {} sessions with a published day{}",
            w.families.join(","),
            w.measurements.join(","),
            p.sessions.len(),
            if p.unpublished.is_empty() { String::new() } else { format!(" ({} without one, left as they are: {})", p.unpublished.len(), p.unpublished.join(" ")) }
        ),
        format!(
            "recompute set: base [{}]; day-local [{}]; history (the ordered pass's) [{}]",
            list(&w.base),
            list(&w.day_local),
            list(&w.history)
        ),
        format!(
            "slots: {} of the day's {} base and day-local slots ({:.1}%); the rest copied forward byte for byte",
            p.slots.0,
            p.slots.1,
            if p.slots.1 > 0 { 100.0 * p.slots.0 as f64 / p.slots.1 as f64 } else { 0.0 }
        ),
        format!("feeds (computed, checked against the published bodies, not published): [{}]", list(&w.feeds)),
        format!(
            "inputs fetched: [{}] (carry, macro, company: the versions core keeps, the provider asked only where it keeps none); stages: [{}]",
            w.categories.join(","),
            w.stages.iter().cloned().collect::<Vec<_>>().join(",")
        ),
    ];
    if !p.perished.is_empty() {
        out.push(format!("past the perishable inputs' retention: {} of the sessions ({}…)", p.perished.len(), p.perished[0]));
    }
    if !p.stale.is_empty() {
        out.push(format!("published in other profile revisions than the build's: {} of the sessions ({}…)", p.stale.len(), p.stale[0]));
    }
    let parts = |e: &Estimate| e.parts.iter().map(|(k, s)| format!("{k} {:.0} s", s)).collect::<Vec<_>>().join(", ");
    out.push(format!("scoped, per day: {} ({})", hours(p.scoped.total()), parts(&p.scoped)));
    out.push(format!("full, per day: {} first of a block ({}); {} a later day", hours(p.full_first.total()), parts(&p.full_first), hours(p.full_next.total())));
    out.push(format!("range: scoped {} (one job per day); full {} (day blocks of {})", hours(p.scoped_total()), hours(p.full_total()), p.block));
    out.push(format!("measured by: {}", p.measured.sources.join("; ")));
    out.push(format!("choice: {} — {}", p.choice.name(), p.why));
    out
}

/// What scheduling a republish made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    pub jobs: Vec<i64>,
    pub sessions: Vec<String>,
}

/// One REPUBLISH job per session of `sessions` (each with a published day), the scope's work as the
/// job's scope. Refused when a session has an unfinished day block or republish of the profile, the
/// build is not registered with plans, or the reason is empty. All or none.
pub async fn schedule(client: &mut Client, profile: &str, build: &str, work: &Work, sessions: &[String], reason: &str) -> Result<Scheduled, String> {
    schedule_tagged(client, profile, build, work, sessions, reason, None).await
}

/// `schedule`, each job's scope carrying `recompute_plan` (C-079: the plan it is part of, for the
/// spend reserved for it).
pub async fn schedule_tagged(client: &mut Client, profile: &str, build: &str, work: &Work, sessions: &[String], reason: &str, plan: Option<&Value>) -> Result<Scheduled, String> {
    if reason.trim().is_empty() {
        return Err("a republish states its reason".into());
    }
    // (release-2 audit, Codex 20) a reason is stored, logged and posted: never a credential
    if let Some(found) = crate::evidence::unredacted(reason) {
        return Err(format!("{found}: a republish's reason carries no credential; nothing was scheduled"));
    }
    if !work.worker() {
        return Err("the scope has no base or day-local member: schedule the ordered pass instead".into());
    }
    if sessions.is_empty() {
        return Err("no session with a published day to republish".into());
    }
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    // (release-2 audit, Codex 19) a build no worker claims for would strand the job
    let runs: bool = tx.query_one("SELECT core.build_runs($1)", &[&build]).await.map_err(db)?.get(0);
    if !runs {
        return Err(format!("engine build {build} is not admitted, or withdrawn: no worker would claim its republishes; nothing was scheduled"));
    }
    let mut scope = work.scope(reason);
    if let Some(plan) = plan {
        scope["recompute_plan"] = plan.clone();
    }
    let scope = scope.to_string();
    let mut jobs = Vec::new();
    for s in sessions {
        // (release-2 audit, Gemini 3; Codex 2) published in this build's profile revisions, and the prints it reads still served
        let row = tx
            .query_one(
                "SELECT core.day_published($1::text::date, $2, $3), core.prints_perished($1::text::date, (now() AT TIME ZONE 'UTC')::date)",
                &[s, &profile, &build],
            )
            .await
            .map_err(db)?;
        if !row.get::<_, bool>(0) {
            return Err(format!("{s} is not published in the profile revisions build {build} publishes: recompute it whole; nothing was scheduled"));
        }
        if row.get::<_, bool>(1) && work.categories.iter().any(|c| c == "exq01") {
            return Err(format!("{s}'s option prints are no longer served and the scope reads them: no faithful recompute; nothing was scheduled"));
        }
        let clash: Option<(i64, String)> = tx
            .query_opt(
                "SELECT job_id, kind FROM core.job WHERE kind IN ('DAY_BLOCK', 'REPUBLISH') AND profile_id = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED')
                   AND $2::text::date BETWEEN first_session AND last_session LIMIT 1",
                &[&profile, s],
            )
            .await
            .map_err(db)?
            .map(|r| (r.get(0), r.get(1)));
        if let Some((job, kind)) = clash {
            return Err(format!("{s} has unfinished {kind} job {job}: let it finish or cancel it first; nothing was scheduled"));
        }
        let published: bool = tx.query_one("SELECT core.day_published($1::text::date, $2)", &[s, &profile]).await.map_err(db)?.get(0);
        if !published {
            return Err(format!("{s} has no published day to republish; nothing was scheduled"));
        }
        let job: i64 = tx
            .query_one(
                "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope)
                 VALUES ('REPUBLISH', $1, $2, $3::text::date, $3::text::date, $3::text::date, $4::text::jsonb) RETURNING job_id",
                &[&profile, &build, s, &scope],
            )
            .await
            .map_err(db)?
            .get(0);
        jobs.push(job);
    }
    let names: Vec<String> = work.families.iter().chain(&work.measurements).cloned().collect();
    crate::notice::log(
        &tx,
        &format!(
            "Republish of [{}] queued under build {build}: {} days, {} to {} ({}); recomputes {} base and {} day-local sensors, the ordered pass the history.",
            names.join(","),
            jobs.len(),
            sessions[0],
            sessions[sessions.len() - 1],
            crate::watch::one_line(reason, 200),
            work.base.len(),
            work.day_local.len()
        ),
    )
    .await?;
    tx.commit().await.map_err(db)?;
    Ok(Scheduled { jobs, sessions: sessions.to_vec() })
}

/// C-074 (release-2 audit, Codex 13): a compare-only day block of `day` (the cluster check's full
/// recompute, `deploy/k8s/README.md` 11): a day block that stages its base and day-local revisions and
/// publishes neither, so the comparison is judged before anything replaces the day. Refused over an
/// unfinished day block or republish of the day, or under a build that does not run.
pub async fn schedule_compare(client: &mut Client, profile: &str, build: &str, day: &str) -> Result<i64, String> {
    let tx = client.transaction().await.map_err(db)?;
    tx.execute("SELECT pg_advisory_xact_lock(hashtextextended('core.job schedule', 0))", &[]).await.map_err(db)?;
    let runs: bool = tx.query_one("SELECT core.build_runs($1)", &[&build]).await.map_err(db)?.get(0);
    if !runs {
        return Err(format!("engine build {build} is not admitted, or withdrawn"));
    }
    if let Some(r) = tx
        .query_opt(
            "SELECT job_id, kind FROM core.job WHERE kind IN ('DAY_BLOCK', 'REPUBLISH') AND profile_id = $1 AND state IN ('QUEUED', 'LEASED', 'FAILED')
               AND $2::text::date BETWEEN first_session AND last_session LIMIT 1",
            &[&profile, &day],
        )
        .await
        .map_err(db)?
    {
        return Err(format!("{day} has unfinished {} job {}", r.get::<_, String>(1), r.get::<_, i64>(0)));
    }
    let job: i64 = tx
        .query_one(
            "INSERT INTO core.job (kind, profile_id, engine_build, first_session, last_session, next_session, scope)
             SELECT 'DAY_BLOCK', $1, $2, s.session, s.session, s.session, '{\"compare_only\": true}'::jsonb FROM core.session s WHERE s.session = $3::text::date
             RETURNING job_id",
            &[&profile, &build, &day],
        )
        .await
        .map_err(|e| format!("{day}: {}", db(e)))?
        .get(0);
    crate::notice::log(&tx, &format!("Compare-only day block of {day} queued (job {job}): it stages the day whole and publishes nothing (C-074's cluster check).")).await?;
    tx.commit().await.map_err(db)?;
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medians_of_odd_and_even_counts() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(vec![]), None);
    }
}
