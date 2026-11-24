//! C-074: a scoped recompute (engine D-104, D-462) on the real slice of 2025-12-10 (AAPL's M0-ANALYTICS
//! with its event stream, M0-GAMMA-CHANGE, VS-IVS01 with its reader scalar, VS-Q01; DLM-MAX-PAIN as the
//! day-local revision; the derived revision as the ordered pass's), the engine's calendar and the
//! engine's real republish plans (`test_support::REPUBLISH_PLANS`). Run with `cargo test -- --ignored`.
//!
//! The byte-identity gate's core half: a copy-forward revision holds the recomputed slots as sent and
//! every other slot of the layer byte for byte as the revision it replaces (body text and JSONB,
//! receipt, coordinates, versions, state, event stream, reader scalars), and a scoped recompute of the
//! day leaves exactly what a full recompute of the same day leaves when the engine sends the same
//! bodies (the engine half: `vyreon-cli/tests/republish.rs`).
use core_contract::Republish;
use core_db::publish::publish_for_lease;
use core_db::republish::{self as rp, Choice};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, refcheck, universe};
use serde_json::json;
use test_support::Scratch;
use test_support::slice::{BASE, DERIVED, bundle, day_local_parts, inventories, parts, reading, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAY: &str = "2025-12-10";
const TODAY: &str = "2026-11-24";
const REASON: &str = "D-999: the gamma change's window (test)";

async fn ready(test: &str) -> (Scratch, Client, String, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut inv = inventories();
    let derived = build::register(&mut client, &reading(inv.remove(1))).await.unwrap().engine_build;
    let base = build::register(&mut client, &test_support::with_republish(with_day_local(inv.remove(0)))).await.unwrap().engine_build;
    for b in [&base, &derived] {
        build::admit(&client, b, &json!({"checklist": "republish test"})).await.unwrap();
    }
    (scratch, client, base, derived)
}

async fn send(client: &mut Client, name: &str, parts: (core_contract::Header, core_contract::Scalars, Vec<core_contract::SlotFiles>), token: &str) -> Result<core_db::publish::Published, LeaseError> {
    static SENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let name = format!("{name}-{}", SENT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let path = bundle(&name, &parts.0, &parts.1, &parts.2);
    let r = publish_for_lease(client, &path, token).await;
    std::fs::remove_file(&path).unwrap();
    r
}

/// 2025-12-10 published by a day block (base, day-local), then its history revision by an ordered pass.
async fn published_day(client: &mut Client, base_build: &str, derived_build: &str) -> (i64, i64, i64) {
    work::schedule(client, Kind::DayBlock, PROFILE, base_build, DAY, DAY, 30).await.unwrap();
    let block = work::claim(client, Kind::DayBlock, base_build, "w1", HOUR).await.unwrap().unwrap();
    let base = send(client, "rp-base", parts(BASE), &block.token).await.unwrap().revision_id;
    let local = send(client, "rp-local", day_local_parts(base_build, DAY, base), &block.token).await.unwrap().revision_id;
    work::day_done(client, &block.token, DAY).await.unwrap();
    work::finish(client, &block.token).await.unwrap();
    work::schedule(client, Kind::OrderedPass, PROFILE, derived_build, DAY, DAY, 1).await.unwrap();
    let pass = work::claim(client, Kind::OrderedPass, derived_build, "p1", HOUR).await.unwrap().unwrap();
    let (mut h, s, f) = parts(DERIVED);
    h.base_revision = Some(base);
    let hist = send(client, "rp-hist", (h, s, f), &pass.token).await.unwrap().revision_id;
    work::finish(client, &pass.token).await.unwrap();
    (base, local, hist)
}

/// Every stored byte of a revision's slots, keyed by slot: the row (state, versions, digest, body text
/// and JSONB, receipt, coordinates), its event stream and its reader scalars.
async fn slots(client: &Client, revision: i64) -> Vec<(String, String)> {
    client
        .query(
            "SELECT m.measurement_id || '/' || m.object_id,
                    concat_ws(' | ', m.slot_state, m.science_version, m.method_version, m.body_sha256, m.body_text, m.body::text, m.receipt::text,
                              COALESCE(m.coordinates::text, '-'),
                              COALESCE((SELECT concat_ws(',', s.root_sha256, encode(s.root_bytes, 'hex'), s.events_sha256, s.event_count, s.byte_length, md5(s.events_gzip))
                                        FROM vyreon.measurement_stream s WHERE (s.revision_id, s.measurement_id, s.object_id) = (m.revision_id, m.measurement_id, m.object_id)), '-'),
                              COALESCE((SELECT string_agg(r.role_id || '=' || r.value::text, ',' ORDER BY r.role_id) FROM vyreon.reader_scalar r
                                        WHERE (r.revision_id, r.measurement_id, r.object_id) = (m.revision_id, m.measurement_id, m.object_id)), '-'))
             FROM vyreon.measurement m WHERE m.revision_id = $1 ORDER BY 1",
            &[&revision],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

async fn state(client: &Client, revision: i64) -> String {
    client.query_one("SELECT state FROM vyreon.day_revision WHERE revision_id = $1", &[&revision]).await.unwrap().get(0)
}

async fn carried(client: &Client, revision: i64) -> Vec<(String, Option<i64>)> {
    client
        .query("SELECT measurement_id, carried_from_revision FROM vyreon.measurement WHERE revision_id = $1 ORDER BY 1", &[&revision])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

fn scope(job: i64, replaces: i64, measurements: &[&str], recomputed: &[&str], changed: &[&str]) -> Republish {
    let v = |x: &[&str]| x.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>();
    Republish { job, replaces, families: vec![], measurements: v(measurements), recomputed: v(recomputed), value_changed: v(changed), reason: REASON.into() }
}

/// The slice's base revision narrowed to `keep`, as a copy-forward base bundle.
fn base_of(keep: &[&str], republish: Republish) -> (core_contract::Header, core_contract::Scalars, Vec<core_contract::SlotFiles>) {
    let (mut h, mut s, mut f) = parts(BASE);
    let kept: Vec<bool> = h.slots.iter().map(|slot| keep.contains(&slot.measurement_id.as_str())).collect();
    let mut i = 0;
    h.slots.retain(|_| (kept[i], i += 1).0);
    let mut i = 0;
    f.retain(|_| (kept[i], i += 1).0);
    s.reader_scalar.retain(|x| keep.contains(&x.measurement_id.as_str()));
    h.republish = Some(republish);
    (h, s, f)
}

/// The day-local revision on `base` carrying `keep` of its slots, as a copy-forward bundle.
fn local_of(build: &str, base: i64, keep: &[&str], republish: Republish) -> (core_contract::Header, core_contract::Scalars, Vec<core_contract::SlotFiles>) {
    let (mut h, mut s, mut f) = day_local_parts(build, DAY, base);
    let kept: Vec<bool> = h.slots.iter().map(|slot| keep.contains(&slot.measurement_id.as_str())).collect();
    let mut i = 0;
    h.slots.retain(|_| (kept[i], i += 1).0);
    let mut i = 0;
    f.retain(|_| (kept[i], i += 1).0);
    s.reader_scalar.retain(|x| keep.contains(&x.measurement_id.as_str()));
    h.republish = Some(republish);
    (h, s, f)
}

fn refused<T: std::fmt::Debug>(r: Result<T, LeaseError>, says: &str) {
    assert!(matches!(&r, Err(LeaseError::Refused(m)) if m.contains(says)), "{r:?} should say {says}");
}

/// The gate: a scoped recompute of M0-GAMMA-CHANGE (the engine's plan: base [M0-GAMMA-CHANGE], no
/// day-local member, history [DLM-UNUSUALNESS]) sends its one slot; core stages the base with the
/// other three slots copied byte for byte, publishes it with the carried day-local revision in one
/// transaction, and, no value having changed, leaves the history revision standing (D-104's narrowed
/// supersede). Every slot of both layers is then the published day's byte for byte, and a full
/// recompute of the day sending the same bodies leaves exactly the same slots. On the way: the scope's
/// bundles are refused when they carry another measurement, replace another revision, or change a
/// version without saying so; the day is not done before its day-local revision; the dump names the
/// day's revisions.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_scoped_recompute_is_the_full_days_slots_and_carries_every_other_byte_for_byte() {
    let (scratch, mut client, b, d) = ready("rp_gate").await;
    let (base1, local1, hist1) = published_day(&mut client, &b, &d).await;
    let (base_bytes, local_bytes) = (slots(&client, base1).await, slots(&client, local1).await);
    assert_eq!(base_bytes.len(), 4);

    // the plan, from the engine's own graph
    let plan = rp::plan(&client, PROFILE, &b, &["M0-GAMMA-CHANGE"], DAY, DAY, 3, TODAY).await.unwrap();
    assert_eq!(plan.work.base.iter().map(String::as_str).collect::<Vec<_>>(), ["M0-GAMMA-CHANGE"]);
    assert!(plan.work.day_local.is_empty() && plan.work.history.contains("DLM-UNUSUALNESS") && plan.work.feeds.contains("M0-ANALYTICS"));
    assert_eq!((plan.sessions.clone(), plan.slots), (vec![DAY.to_owned()], (1, 5)));
    let made = rp::schedule(&mut client, PROFILE, &b, &plan.work, &plan.sessions, REASON).await.unwrap();
    assert!(rp::schedule(&mut client, PROFILE, &b, &plan.work, &plan.sessions, REASON).await.unwrap_err().contains("unfinished REPUBLISH job"));
    let job = made.jobs[0];
    let lease = work::claim(&mut client, Kind::Republish, &b, "r1", HOUR).await.unwrap().unwrap();
    assert_eq!((lease.job_id, lease.sessions.clone()), (job, vec![DAY.to_owned()]));

    // the dump: the day's published base and day-local revision
    let dump = std::env::temp_dir().join(format!("rp-dump-{}.tar", std::process::id()));
    refcheck::dump_to(&mut client, &lease.token, DAY, &dump).await.unwrap();
    let mut tar = tar::Archive::new(std::fs::File::open(&dump).unwrap());
    let mut revisions = String::new();
    for e in tar.entries().unwrap() {
        let mut e = e.unwrap();
        if e.path().unwrap().to_str() == Some("revisions.jsonl") {
            std::io::Read::read_to_string(&mut e, &mut revisions).unwrap();
        }
    }
    std::fs::remove_file(&dump).unwrap();
    let ids: Vec<i64> = revisions.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["revision_id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [base1, local1]);

    // refused: another measurement carried, another revision replaced, a version changed unsaid
    let m = ["M0-GAMMA-CHANGE"];
    refused(send(&mut client, "rp-x1", base_of(&["M0-GAMMA-CHANGE", "VS-Q01"], scope(job, base1, &m, &m, &[])), &lease.token).await, "the slots are not the inventory");
    let both = ["M0-GAMMA-CHANGE", "VS-Q01"];
    refused(send(&mut client, "rp-x2", base_of(&both, scope(job, base1, &m, &both, &[])), &lease.token).await, "recomputes [M0-GAMMA-CHANGE]");
    refused(send(&mut client, "rp-x3", base_of(&m, scope(job, local1, &m, &m, &[])), &lease.token).await, "but 2025-12-10's published base revision is now");
    let mut bumped = base_of(&m, scope(job, base1, &m, &m, &[]));
    bumped.0.slots[0].method_version.push_str("-next");
    refused(send(&mut client, "rp-x4", bumped, &lease.token).await, "but the bundle does not state M0-GAMMA-CHANGE changed");

    // the base: staged, its other slots copied; the reader still serves the day as it was
    let staged = send(&mut client, "rp-base2", base_of(&m, scope(job, base1, &m, &m, &[])), &lease.token).await.unwrap();
    assert!(staged.staged && staged.superseded == 0);
    let base2 = staged.revision_id;
    assert_eq!((state(&client, base2).await, state(&client, base1).await), ("STAGED".to_owned(), "PUBLISHED".to_owned()));
    assert_eq!(slots(&client, base2).await, base_bytes, "every slot byte for byte");
    assert_eq!(
        carried(&client, base2).await,
        [("M0-ANALYTICS".to_owned(), Some(base1)), ("M0-GAMMA-CHANGE".to_owned(), None), ("VS-IVS01".to_owned(), Some(base1)), ("VS-Q01".to_owned(), Some(base1))]
    );
    refused(work::day_done(&mut client, &lease.token, DAY).await, "has not published 2025-12-10's day-local revision");

    // the day-local revision on it, every slot carried: both published at once
    let local = send(&mut client, "rp-local2", local_of(&b, base2, &[], scope(job, local1, &m, &[], &[])), &lease.token).await.unwrap();
    assert!(!local.staged);
    let local2 = local.revision_id;
    for (r, s) in [(base1, "SUPERSEDED"), (local1, "SUPERSEDED"), (base2, "PUBLISHED"), (local2, "PUBLISHED"), (hist1, "PUBLISHED")] {
        assert_eq!(state(&client, r).await, s, "revision {r}");
    }
    assert_eq!(slots(&client, local2).await, local_bytes);
    assert_eq!(carried(&client, local2).await, [("DLM-MAX-PAIN".to_owned(), Some(local1))]);
    let recorded: serde_json::Value = serde_json::from_str(
        &client.query_one("SELECT republish_scope::text FROM vyreon.day_revision WHERE revision_id = $1", &[&base2]).await.unwrap().get::<_, String>(0),
    )
    .unwrap();
    assert_eq!((recorded["job"].as_i64(), recorded["replaces"].as_i64(), recorded["recomputed"].clone()), (Some(job), Some(base1), json!(["M0-GAMMA-CHANGE"])));
    assert_eq!(client.query_one("SELECT count(*) FROM core.job WHERE kind = 'ORDERED_PASS' AND state = 'QUEUED'", &[]).await.unwrap().get::<_, i64>(0), 0, "no value changed: nothing to derive again");
    work::day_done(&mut client, &lease.token, DAY).await.unwrap();
    work::finish(&mut client, &lease.token).await.unwrap();

    // a full recompute of the day sending the same bodies: core finds them to be the very revisions the
    // scoped recompute published (every slot's digest equal), so it answers them as published already
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    let full = send(&mut client, "rp-base3", parts(BASE), &block.token).await.unwrap();
    assert_eq!((full.already_published, full.revision_id), (true, base2));
    let full_local = send(&mut client, "rp-local3", day_local_parts(&b, DAY, base2), &block.token).await.unwrap();
    assert_eq!((full_local.already_published, full_local.revision_id), (true, local2));
    work::day_done(&mut client, &block.token, DAY).await.unwrap();
    assert_eq!((state(&client, base2).await, state(&client, local2).await, state(&client, hist1).await), ("PUBLISHED".into(), "PUBLISHED".into(), "PUBLISHED".into()));
    scratch.drop().await;
}

/// A republish whose value changed cascades as any republish (the day's history revision falls and the
/// ordered pass derives it again, C-037); an attempt that ends before its day-local revision leaves the
/// day as it was (its staged base rejected) and the next attempt publishes it whole; the ordered pass
/// of the day waits for the republish.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_changed_value_cascades_and_a_failed_attempt_leaves_the_day_whole() {
    let (scratch, mut client, b, d) = ready("rp_cascade").await;
    let (base1, local1, hist1) = published_day(&mut client, &b, &d).await;
    let plan = rp::plan(&client, PROFILE, &b, &["M0-GAMMA-CHANGE"], DAY, DAY, 3, TODAY).await.unwrap();
    let job = rp::schedule(&mut client, PROFILE, &b, &plan.work, &plan.sessions, REASON).await.unwrap().jobs[0];
    // a pass of the day scheduled meanwhile waits for the republish
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &d, DAY, DAY, 1).await.unwrap();
    assert!(work::claim(&mut client, Kind::OrderedPass, &d, "p2", HOUR).await.unwrap().is_none());
    let m = ["M0-GAMMA-CHANGE"];
    let first = work::claim(&mut client, Kind::Republish, &b, "r1", HOUR).await.unwrap().unwrap();
    let staged = send(&mut client, "rpc-base", base_of(&m, scope(job, base1, &m, &m, &m)), &first.token).await.unwrap().revision_id;
    assert!(work::fail(&mut client, &first.token, "republish 2025-12-10", "the node went", "").await.unwrap());
    assert_eq!((state(&client, staged).await, state(&client, base1).await, state(&client, local1).await), ("REJECTED".into(), "PUBLISHED".into(), "PUBLISHED".into()));
    let second = work::claim(&mut client, Kind::Republish, &b, "r2", HOUR).await.unwrap().unwrap();
    let base2 = send(&mut client, "rpc-base2", base_of(&m, scope(job, base1, &m, &m, &m)), &second.token).await.unwrap().revision_id;
    send(&mut client, "rpc-local2", local_of(&b, base2, &[], scope(job, local1, &m, &[], &[])), &second.token).await.unwrap();
    // a value changed: the day's history revision falls; its pass (already queued) derives it again
    assert_eq!(state(&client, hist1).await, "SUPERSEDED");
    work::day_done(&mut client, &second.token, DAY).await.unwrap();
    assert!(work::claim(&mut client, Kind::OrderedPass, &d, "p2", HOUR).await.unwrap().is_none(), "the republish's lease still holds it");
    work::finish(&mut client, &second.token).await.unwrap();
    assert!(work::claim(&mut client, Kind::OrderedPass, &d, "p2", HOUR).await.unwrap().is_some(), "after the republish, the pass runs");
    // a scope of history sensors only is no republish
    let hist = rp::plan(&client, PROFILE, &b, &["DLM-POSITIONING"], DAY, DAY, 3, TODAY).await.unwrap();
    assert_eq!(hist.choice, Choice::Passes);
    assert!(rp::schedule(&mut client, PROFILE, &b, &hist.work, &hist.sessions, REASON).await.unwrap_err().contains("schedule the ordered pass"));
    scratch.drop().await;
}

/// The planner on the engine's real plans: the recompute set and its share of the slots, the inputs and
/// stages, the cost each way from the reference measurements, then from core's stored reports once a
/// base revision carries one; SCOPED exactly when its measured cost for the range is below FULL's; a
/// scope reading the perishable inputs on days past their retention is not SCOPED; a range's unpublished
/// sessions are named and left alone.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_planner_chooses_by_the_measured_cost_of_the_range() {
    let (scratch, mut client, b, d) = ready("rp_plan").await;
    let (base1, _, _) = published_day(&mut client, &b, &d).await;
    // planned two sessions after the day, its prints still served
    let soon = "2025-12-12";
    for names in [vec!["M0-GAMMA-CHANGE"], vec!["DLM-CHAIN"], vec!["CDD-FND01"], vec!["P"], vec!["DLM-CHAIN", "CDD-FND01"]] {
        let p = rp::plan(&client, PROFILE, &b, &names, "2025-12-09", "2025-12-11", 3, soon).await.unwrap();
        assert!(p.perished.is_empty());
        assert_eq!(p.sessions, [DAY], "{names:?}");
        assert_eq!(p.unpublished, ["2025-12-09", "2025-12-11"]);
        let lines = rp::describe(&p);
        assert!(lines.iter().any(|l| l.starts_with("measured by: reference: cluster job 1423")), "{lines:#?}");
        assert_eq!(p.choice == Choice::Scoped, p.scoped_total() < p.full_total(), "{names:?}: {lines:#?}");
        eprintln!("{names:?}\n  {}", lines.join("\n  "));
    }
    // a day-local family runs no intent; one company sensor fetches its capture and the daily bars
    let chain = rp::plan(&client, PROFILE, &b, &["DLM-CHAIN"], DAY, DAY, 3, soon).await.unwrap();
    assert!(!chain.work.stages.contains("intents") && chain.work.stages.contains("surfaces") && chain.choice == Choice::Scoped);
    let fnd = rp::plan(&client, PROFILE, &b, &["CDD-FND01"], DAY, DAY, 3, soon).await.unwrap();
    assert_eq!((fnd.work.categories.clone(), fnd.choice), (vec!["daily".to_owned(), "options".into(), "company".into()], Choice::Scoped));
    assert!(fnd.scoped.total() < chain.scoped.total() && chain.scoped.total() < chain.full_first.total());
    // the price family reaches nearly the whole day: alone, a day saves only intent B's other owners
    // (30 of 343 task-minutes) less the dump; over three days a full recompute's day blocks share their
    // download window (a later day's 32-48 min against a first day's 96) and win
    let mut p = rp::plan(&client, PROFILE, &b, &["P"], DAY, DAY, 3, soon).await.unwrap();
    assert_eq!(p.choice, Choice::Scoped, "{:#?}", rp::describe(&p));
    assert!(p.full_first.total() - p.scoped.total() < 600.0);
    p.sessions = vec!["2025-12-10".into(), "2025-12-11".into(), "2025-12-12".into()];
    assert!(p.full_total() < p.scoped_total(), "{} {}", p.full_total(), p.scoped_total());
    // a year on, the day's prints are no longer served: a full recompute would withhold EXQ01's readings,
    // the scoped one carries them (whatever the cost)
    let p = rp::plan(&client, PROFILE, &b, &["P"], DAY, DAY, 3, TODAY).await.unwrap();
    assert_eq!((p.choice, p.perished.clone()), (Choice::Scoped, vec![DAY.to_owned()]));
    assert!(p.why.contains("carries them forward"), "{}", p.why);
    // and EXQ itself has no faithful recompute
    let exq = rp::plan(&client, PROFILE, &b, &["EXQ"], DAY, DAY, 3, TODAY).await.unwrap();
    assert_eq!(exq.choice, Choice::Full);
    assert!(exq.why.starts_with("no faithful recompute of 1 of the sessions: the scope reads [exq01]"), "{}", exq.why);
    // core's own reports, once a base carries one (the stages of job 1423's 2026-11-06, its worker log)
    client
        .execute(
            "UPDATE vyreon.day_revision SET report = '{\"stages\": [{\"stage\": \"intent B (T)\", \"seconds\": 3064}, {\"stage\": \"intent A (history nodes)\", \"seconds\": 3605}]}'::jsonb WHERE revision_id = $1",
            &[&base1],
        )
        .await
        .unwrap();
    let m = rp::measured(&client, &serde_json::from_str(test_support::REPUBLISH_PLANS).unwrap(), PROFILE).await.unwrap();
    assert_eq!((m.stages["intent B (T)"], m.stages["intent A (history nodes)"], m.stages["intent B plan"]), (3064.0, 3605.0, 132.0));
    assert!(m.sources[0].starts_with("core's reports: the medians of the latest 1 base revisions' stages"), "{:?}", m.sources);
    // unknown names are refused
    assert!(rp::plan(&client, PROFILE, &b, &["NOPE"], DAY, DAY, 3, TODAY).await.unwrap_err().contains("neither a family nor a measurement"));
    scratch.drop().await;
}

/// A hand-over carries a queued republish with its recompute set composed again from the successor's
/// plans, and is refused (nothing moved) when the successor registered none.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_hand_over_carries_a_republish_on_the_successors_plans() {
    let (scratch, mut client, b, d) = ready("rp_handover").await;
    published_day(&mut client, &b, &d).await;
    let plan = rp::plan(&client, PROFILE, &b, &["DLM-CHAIN"], DAY, DAY, 3, TODAY).await.unwrap();
    let job = rp::schedule(&mut client, PROFILE, &b, &plan.work, &plan.sessions, REASON).await.unwrap().jobs[0];
    let mut bare = with_day_local(inventories().remove(0));
    bare["engine_build"] = json!("eeeeeeee+000000000000");
    build::register(&mut client, &bare).await.unwrap();
    let refused = build::hand_over(&mut client, &b, "eeeeeeee+000000000000", "no plans", true, None).await.unwrap_err();
    assert!(refused.contains(&format!("republish job {job} cannot move to build eeeeeeee+000000000000")) && refused.contains("registered no republish plans"), "{refused}");
    let mut next = test_support::with_republish(with_day_local(inventories().remove(0)));
    next["engine_build"] = json!("ffffffff+000000000000");
    build::register(&mut client, &next).await.unwrap();
    let token = build::hand_over(&mut client, &b, "ffffffff+000000000000", "the next image", true, None).await.unwrap().token;
    let h = build::hand_over(&mut client, &b, "ffffffff+000000000000", "the next image", false, Some(&token)).await.unwrap();
    let carried = h.carried.iter().find(|c| c.kind == "REPUBLISH").unwrap();
    let scope: serde_json::Value = serde_json::from_str(
        &client.query_one("SELECT scope::text FROM core.job WHERE job_id = $1", &[&carried.new_job.unwrap()]).await.unwrap().get::<_, String>(0),
    )
    .unwrap();
    assert_eq!((scope["families"].clone(), scope["reason"].clone()), (json!(["DLM-CHAIN"]), json!(REASON)));
    assert_eq!(scope["recompute"]["day_local"], json!(plan.work.day_local));
    scratch.drop().await;
}

/// The reference re-run of a republished day checks what the republish recomputed when the revisions
/// it replaced had a completed check (every carried slot was judged there); a day republished whole,
/// or one whose replaced revisions were never checked, is checked whole.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_republished_days_reference_rerun_checks_what_was_recomputed() {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let (scratch, mut client, b, d) = ready("rp_refcheck").await;
    // the day published, then its check completed under its block's lease
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let base1 = send(&mut client, "rr-base", parts(BASE), &block.token).await.unwrap().revision_id;
    let local1 = send(&mut client, "rr-local", day_local_parts(&b, DAY, base1), &block.token).await.unwrap().revision_id;
    work::day_done(&mut client, &block.token, DAY).await.unwrap();
    let account = json!({
        "schema": refcheck::SCHEMA, "session": DAY, "outcome": "COMPLETED", "why": null, "wall_seconds": 10.0, "suite_seconds": 9.0,
        "cpu_seconds": 60.0, "cpu_complete": true, "jobs": 8, "deadline_seconds": 36000, "revisions": [base1, local1],
        "counts": {"checked": 10, "passed": 10, "failed": 0, "skipped": 0, "reference_errors": 0, "unit_mismatch": 0, "failure_classes": 0, "measurements": 2, "measurements_failing": 0},
        "summary": {}, "report_trimmed": null, "versions": {"QuantLib": "1.43"},
    });
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(account.to_string().as_bytes()).unwrap();
    refcheck::put(&mut client, &block.token, &refcheck::check(gz.finish().unwrap(), DAY).unwrap()).await.unwrap();
    work::finish(&mut client, &block.token).await.unwrap();
    let _ = d;
    // a day-local republish of DLM-MAX-PAIN's family
    let plan = rp::plan(&client, PROFILE, &b, &["DLM-CHAIN"], DAY, DAY, 3, TODAY).await.unwrap();
    let job = rp::schedule(&mut client, PROFILE, &b, &plan.work, &plan.sessions, REASON).await.unwrap().jobs[0];
    let lease = work::claim(&mut client, Kind::Republish, &b, "r1", HOUR).await.unwrap().unwrap();
    let recomputed: Vec<String> = plan.work.day_local.iter().cloned().collect();
    let names: Vec<&str> = recomputed.iter().map(String::as_str).collect();
    let republish = Republish { job, replaces: local1, families: vec!["DLM-CHAIN".into()], measurements: vec![], recomputed: recomputed.clone(), value_changed: vec![], reason: REASON.into() };
    let local2 = send(&mut client, "rr-local2", local_of(&b, base1, &names, republish), &lease.token).await.unwrap().revision_id;
    work::day_done(&mut client, &lease.token, DAY).await.unwrap();
    work::finish(&mut client, &lease.token).await.unwrap();
    let later: String = client.query_one("SELECT (now() + interval '7 hours')::text", &[]).await.unwrap().get(0);
    refcheck::queue_reruns(&client, PROFILE, DAY, 6.0, &later).await.unwrap();
    let scope: serde_json::Value = serde_json::from_str(
        &client.query_one("SELECT scope::text FROM core.job WHERE kind = 'REFCHECK' AND state = 'QUEUED'", &[]).await.unwrap().get::<_, String>(0),
    )
    .unwrap();
    assert_eq!((scope["base_revision"].as_i64(), scope["day_local_revision"].as_i64()), (Some(base1), Some(local2)));
    assert_eq!(scope["measurements"], json!(recomputed));
    scratch.drop().await;
}

/// Engine D-462 finishes the C-073 hotfix: a download that reports its progress is judged by it. While
/// its progress advances it is no stall; once it stops advancing, NO_PROGRESS alerts within the usual
/// 20 minutes, not the implicit 240-minute bound a download that reports none keeps.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_download_reporting_its_progress_is_judged_by_it() {
    use core_db::watch::{self, Settings};
    let (scratch, mut client, b, _) = ready("rp_progress").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, DAY, DAY, 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "day-1", 48.0 * HOUR).await.unwrap().unwrap();
    let settings = Settings {
        profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None,
        refcheck_from: None, backup_days: None, refcheck_rerun_hours: None,
    };
    let step = "download 2025-12-10: fill 2025-12-10: options";
    // reported now with its progress; the watch looks 10 and 30 minutes on (the report stands still)
    work::report_step_with(&mut client, &block.token, step, None, None, Some("bulk files 3 of 31")).await.unwrap();
    for (minutes, alerts) in [(10.0, 0), (30.0, 1)] {
        let t: String = client.query_one("SELECT (now() + make_interval(secs => $1::float8 * 60))::text", &[&minutes]).await.unwrap().get(0);
        watch::check(&mut client, &settings, &t).await.unwrap();
        assert_eq!(no_progress(&client).await, alerts, "at {minutes} min");
    }
    // advancing again: resolved at the next look
    work::report_step_with(&mut client, &block.token, step, None, None, Some("bulk files 4 of 31")).await.unwrap();
    let t: String = client.query_one("SELECT (now() + interval '5 minutes')::text", &[]).await.unwrap().get(0);
    watch::check(&mut client, &settings, &t).await.unwrap();
    assert_eq!(no_progress(&client).await, 0);
    scratch.drop().await;
}

async fn no_progress(c: &Client) -> i64 {
    c.query_one("SELECT count(*) FROM core.problem WHERE resolved_at IS NULL AND kind = 'NO_PROGRESS'", &[]).await.unwrap().get(0)
}
