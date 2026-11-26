//! C-079: after a release, core compares the methods the new build publishes with each published day's
//! and republishes what differs, within the owner's ceiling. The real slice of 2025-12-10 (published as
//! each day), the engine's calendar and its real republish plans; the new build's method change is
//! M0-GAMMA-CHANGE's method version moved on (as `republish.rs` moves one), the spend the account's
//! real figures (C-078's test). Run with `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::recompute::{self, RecomputeWatch};
use core_db::spend::{self, SpendWatch};
use core_db::work::{self, Claim, Kind};
use core_db::{build, migrate, universe};
use serde_json::json;
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAYS: [&str; 3] = ["2025-12-09", "2025-12-10", "2025-12-11"];
const NOW: &str = "2026-11-24 07:00:00+00";
const CHANGED: &str = "M0-GAMMA-CHANGE";

async fn ready(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    (scratch, client)
}

/// A build named `name` of the slice's base and day-local profiles with the engine's plans, admitted.
async fn admitted(client: &mut Client, name: &str) -> String {
    let mut inv = test_support::with_republish(with_day_local(inventories().remove(0)));
    inv["engine_build"] = json!(name);
    build::register(client, &inv).await.unwrap();
    build::admit(client, name, &json!({"checklist": "recompute test"})).await.unwrap();
    name.to_owned()
}

fn unique(name: &str) -> String {
    static SENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!("{name}-{}", SENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// `day` published whole by a day block of `build`; `changed`: its M0-GAMMA-CHANGE under the next method.
async fn publish_day(client: &mut Client, build: &str, day: &str, changed: bool) {
    work::schedule(client, Kind::DayBlock, PROFILE, build, day, day, 1).await.unwrap();
    let claim: Claim = work::claim(client, Kind::DayBlock, build, "w1", HOUR).await.unwrap().unwrap();
    let (mut h, s, f) = parts(BASE);
    (h.represented_date, h.engine_build) = (day.into(), build.into());
    if changed {
        for slot in h.slots.iter_mut().filter(|s| s.measurement_id == CHANGED) {
            slot.method_version.push_str("-next");
        }
    }
    let path = bundle(&unique(&format!("c079-{day}")), &h, &s, &f);
    let base = publish_for_lease(client, &path, &claim.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (h, s, f) = day_local_parts(build, day, base);
    let path = bundle(&unique(&format!("c079-local-{day}")), &h, &s, &f);
    publish_for_lease(client, &path, &claim.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::day_done(client, &claim.token, day).await.unwrap();
    work::finish(client, &claim.token).await.unwrap();
}

fn rw() -> RecomputeWatch {
    RecomputeWatch { node_hour_usd: 0.167, block: 3, from: None }
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

/// A reading of DigitalOcean's figures (the account's real month to date of 2026-11-24), as of `at`.
async fn reading(client: &Client, at: &str) {
    let balance = json!({"month_to_date_usage": "269.04", "generated_at": at});
    let r = spend::from_answers(&balance, &json!({"invoices": []}), "2026-11").unwrap();
    spend::put_reading(client, Ok(&r)).await.unwrap();
    client.execute("UPDATE core.spend_reading SET read_at = $1::text::timestamptz WHERE reading_id = (SELECT max(reading_id) FROM core.spend_reading)", &[&at]).await.unwrap();
}

/// Two days published by the old build; the new build (its M0-GAMMA-CHANGE method moved on) publishes a
/// third. Nothing is compared before it has published a whole day. The second day is being computed (the
/// new build's block queued over it): left, and decided again once it is free (release-3 audit, Codex 5,
/// Gemini 11). The first is planned for the measurement M0-GAMMA-CHANGE as the C-074 planner chooses, its
/// cost with the check's re-run (Codex 9). Held without a watched spend, said once on both channels; held
/// past the ceiling, the reserve of plans not yet billed counted (Codex 10); a raised ceiling decides it
/// again and schedules it (Codex 11, Gemini 8).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_new_builds_method_change_is_recomputed_within_the_ceiling() {
    let (scratch, mut client) = ready("c079_recompute").await;
    let old = admitted(&mut client, "slice-old-image").await;
    publish_day(&mut client, &old, DAYS[0], false).await;
    publish_day(&mut client, &old, DAYS[1], false).await;
    let new = admitted(&mut client, "slice-new-image").await;
    assert_eq!(recompute::due(&client, PROFILE, None).await.unwrap(), None, "nothing of the new build published yet");
    publish_day(&mut client, &new, DAYS[2], true).await;
    assert_eq!(recompute::due(&client, PROFILE, None).await.unwrap(), Some((new.clone(), vec!["base", "day-local"], None)));
    // the second day is being computed again under the new build
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &new, DAYS[1], DAYS[1], 1).await.unwrap();

    let p = recompute::propose(&client, PROFILE, &new, &["base", "day-local", "history"], None, None, 3, "2026-11-24").await.unwrap();
    assert_eq!((p.layers.clone(), p.waiting.clone(), p.busy.clone()), (vec!["base".to_owned(), "day-local".to_owned()], vec!["history".to_owned()], vec![DAYS[1].to_owned()]));
    assert_eq!(p.groups.len(), 1);
    let g = &p.groups[0];
    assert_eq!((g.names.clone(), g.days.clone(), g.perished), (vec![CHANGED.to_owned()], vec![DAYS[0].to_owned()], false));
    let plan = g.plan.as_ref().unwrap();
    assert!(g.schedulable && g.seconds > 0.0);
    // no re-run measured yet: priced at its deadline (an upper bound)
    let want = if g.choice == Some(core_db::republish::Choice::Scoped) { plan.scoped_total() + recompute::RERUN_BOUND_SECONDS } else { plan.full_total() };
    assert_eq!(g.seconds, want);
    let lines = recompute::describe(&p, 0.167);
    assert_eq!(lines[0], format!("Build {new}'s methods (base, day-local compared) differ on 1 published days"));
    assert!(lines[1].starts_with(&format!(" · [{CHANGED}] on 1 days ({}): ", DAYS[0])), "{lines:?}");
    assert!(lines.iter().any(|l| l == " 1 days being computed (unfinished day blocks or republishes): compared again once they are done"), "{lines:?}");
    // a day before VYREON_CORE_RECOMPUTE_FROM is left alone
    let later = recompute::propose(&client, PROFILE, &new, &["base", "day-local"], Some(DAYS[1]), None, 3, "2026-11-24").await.unwrap();
    assert!(later.groups.is_empty() && later.before == 1);

    // no spend watched: held, both channels told once, nothing queued; not decided again while nothing changes
    let w = rw();
    let jobs = "SELECT count(*) FROM core.job WHERE engine_build = 'slice-new-image' AND kind IN ('REPUBLISH', 'DAY_BLOCK') AND state = 'QUEUED' AND first_session = '2025-12-09'";
    let d = recompute::auto(&mut client, PROFILE, &w, None, NOW).await.unwrap().unwrap();
    assert_eq!((d.outcome, d.jobs), ("HELD", 0));
    assert!(d.why.unwrap().contains("the spend is not watched"));
    assert_eq!(count(&client, jobs).await, 0);
    assert!(recompute::auto(&mut client, PROFILE, &w, None, NOW).await.unwrap().is_none(), "nothing new to decide");
    // C-091: a decision left to the owner, one alert, no command; nothing on the progress channel
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'alerts' AND message LIKE '🔴 Needs you: a recompute%'", &[]).await.unwrap().get(0);
    assert!(alert.contains("the spend is not watched") && !alert.contains('`') && !alert.contains("vyreon-core"), "{alert}");
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'progress'").await, 0);

    // the spend watched (DigitalOcean's figure of 06:30): the plan's cost past a tight ceiling, held again
    reading(&client, "2026-11-24 06:30:00+00").await;
    let cost = p.seconds() / 3600.0 * 0.167;
    let tight = SpendWatch { since: "2026-11-21T06:09:00Z".into(), baseline_usd: 180.41, ceiling_usd: 88.63 + cost / 2.0 };
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&tight), NOW).await.unwrap().unwrap();
    assert_eq!(d.outcome, "HELD");
    assert!(d.why.as_deref().unwrap().contains("reserved by plans not yet billed") && d.why.as_deref().unwrap().contains("would pass the"), "{:?}", d.why);
    assert_eq!(count(&client, jobs).await, 0);
    assert!(recompute::auto(&mut client, PROFILE, &w, Some(&tight), NOW).await.unwrap().is_none(), "the same ceiling, no newer figure: not decided again");
    // the owner raises the ceiling: decided again, scheduled
    let ok = SpendWatch { ceiling_usd: 500.0, ..tight.clone() };
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&ok), NOW).await.unwrap().unwrap();
    assert_eq!((d.outcome, d.why.clone()), ("SCHEDULED", None));
    assert!(d.jobs >= 1);
    assert_eq!(count(&client, jobs).await, 1, "the day's republish (SCOPED) or its day block (FULL)");
    let tagged: i64 = count(&client, "SELECT count(*) FROM core.job WHERE scope ? 'recompute_plan' AND first_session = '2025-12-09'").await;
    assert_eq!(tagged, 1, "the job names its plan");
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'progress'").await, 0, "no recompute post on progress (C-091)");
    // reserved while its job is unfinished: a further plan is held by it
    let reserved = recompute::reserved(&client, "2026-11-24 06:30:00+00").await.unwrap();
    assert!((reserved - cost).abs() < 1e-6, "{reserved} {cost}");
    assert_eq!(recompute::due(&client, PROFILE, Some(500.0)).await.unwrap(), None);

    // the busy day's block is cancelled: the day is free and still differs, so it is decided now, alone
    client.execute("UPDATE core.job SET state = 'CANCELLED' WHERE kind = 'DAY_BLOCK' AND engine_build = $1 AND first_session = '2025-12-10'", &[&new]).await.unwrap();
    assert_eq!(recompute::due(&client, PROFILE, Some(500.0)).await.unwrap(), Some((new.clone(), vec!["base", "day-local"], Some(vec![DAYS[1].to_owned()]))));
    let tight2 = SpendWatch { ceiling_usd: 88.63 + cost * 1.5, ..tight };
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&tight2), NOW).await.unwrap().unwrap();
    assert_eq!(d.outcome, "HELD", "the first plan's reserve and this one's cost pass the ceiling: {:?}", d.why);
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&ok), NOW).await.unwrap().unwrap();
    assert_eq!(d.outcome, "SCHEDULED");
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE scope ? 'recompute_plan' AND first_session = '2025-12-10'").await, 1);
    assert_eq!(recompute::due(&client, PROFILE, Some(500.0)).await.unwrap(), None);
    scratch.drop().await;
}

/// Release-3 audit, Codex 6: a measurement is planned as the measurement, never as the family of the same
/// name (DLM-TECHNICAL: the measurement recomputes itself, the family seven).
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_measurement_is_never_read_as_its_family() {
    let (scratch, mut client) = ready("c079_measurement").await;
    let b = admitted(&mut client, "slice-image").await;
    publish_day(&mut client, &b, DAYS[0], false).await;
    let days = vec![DAYS[0].to_owned()];
    let m = core_db::republish::plan_days(&client, PROFILE, &b, &["DLM-TECHNICAL".to_owned()], &days, 3, "2026-11-24").await.unwrap();
    let f = core_db::republish::plan(&client, PROFILE, &b, &["DLM-TECHNICAL"], DAYS[0], DAYS[0], 3, "2026-11-24").await.unwrap();
    assert_eq!(m.work.measurements, vec!["DLM-TECHNICAL".to_owned()]);
    assert!(m.work.families.is_empty() && f.work.families == vec!["DLM-TECHNICAL".to_owned()]);
    assert!(m.work.day_local.len() < f.work.day_local.len(), "{:?} {:?}", m.work.day_local, f.work.day_local);
    scratch.drop().await;
}

/// Release-3 audit, Codex 8, Gemini 6: FULL over days apart is priced as it is scheduled, a block's first
/// day at each gap.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_full_recompute_across_gaps_is_priced_by_its_runs() {
    let (scratch, mut client) = ready("c079_gaps").await;
    let b = admitted(&mut client, "slice-image").await;
    publish_day(&mut client, &b, DAYS[0], false).await;
    let mut p = core_db::republish::plan(&client, PROFILE, &b, &["P"], DAYS[0], DAYS[0], 3, "2026-11-24").await.unwrap();
    let (first, next) = (p.full_first.total(), p.full_next.total());
    // three days in a row: one block (a first day and two later days)
    p.sessions = vec!["2025-12-08".into(), "2025-12-09".into(), "2025-12-10".into()];
    p.unpublished = vec![];
    assert!((p.full_total() - (first + 2.0 * next)).abs() < 1e-6);
    // three days apart: three blocks, three first days
    p.sessions = vec!["2025-12-08".into(), "2025-12-10".into(), "2025-12-12".into()];
    p.skipped = vec!["2025-12-09".into(), "2025-12-11".into()];
    assert_eq!(p.runs().len(), 3);
    assert!((p.full_total() - 3.0 * first).abs() < 1e-6, "{} {}", p.full_total(), 3.0 * first);
    scratch.drop().await;
}

/// Release-3 audit, Codex 1 and 15, Gemini 1 and 10; coordinator: "print retention rechecked at claim for
/// every job kind, FULL included; old days scoped regardless". A published day past the prints' retention
/// (the capture's start moved before it, as `republish.rs` moves it) is planned apart from the others, and a
/// queued full recompute over it is cut at its claim: the day becomes a scoped republish of the
/// measurements the block names, its other days queued again; a block that names none leaves the day
/// published and says so.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_full_recompute_never_runs_over_a_day_whose_prints_are_gone() {
    let (scratch, mut client) = ready("c079_perished").await;
    let old = admitted(&mut client, "slice-old-image").await;
    publish_day(&mut client, &old, DAYS[0], false).await;
    let new = admitted(&mut client, "slice-new-image").await;
    publish_day(&mut client, &new, DAYS[2], true).await;
    client.execute("UPDATE core.source_coverage SET first_date = '2025-06-02' WHERE source = 'option_prints'", &[]).await.unwrap();
    let p = recompute::propose(&client, PROFILE, &new, &["base", "day-local"], None, None, 3, "2026-11-24").await.unwrap();
    assert_eq!(p.groups.len(), 1);
    assert!(p.groups[0].perished, "planned as a day whose prints are gone");
    assert!(recompute::describe(&p, 0.167)[1].contains("prints no longer served"), "{:?}", recompute::describe(&p, 0.167));
    // a full recompute queued over it (and the day after, not published) by a C-079 plan, then claimed
    let scope = json!({"recompute_plan": 1, "measurements": [CHANGED], "reason": "C-079: test"});
    work::schedule_scoped(&mut client, Kind::DayBlock, PROFILE, &new, DAYS[0], DAYS[1], 3, Some(&scope)).await.unwrap();
    let claim = work::claim(&mut client, Kind::DayBlock, &new, "w9", HOUR).await.unwrap().unwrap();
    assert_eq!(claim.sessions, [DAYS[1]], "the published day past retention is cut out of the block");
    let republish: i64 = count(&client, "SELECT count(*) FROM core.job WHERE kind = 'REPUBLISH' AND first_session = '2025-12-09' AND scope ? 'recompute_plan'").await;
    let alerted: i64 = count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'alerts' AND message LIKE '🔴 Needs you: a full recompute of Dec 9 %'").await;
    assert_eq!(republish + alerted, 1, "scoped, or (its scope reading the prints) left to the owner and said");
    // a block naming nothing: the day stays published, said
    work::finish(&mut client, &claim.token).await.unwrap_err();
    client.execute("UPDATE core.job SET state = 'CANCELLED' WHERE kind IN ('DAY_BLOCK', 'REPUBLISH') AND state IN ('QUEUED', 'LEASED')", &[]).await.unwrap();
    client.execute("UPDATE core.lease SET ended_at = now(), outcome = 'CANCELLED' WHERE ended_at IS NULL", &[]).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &new, DAYS[0], DAYS[0], 1).await.unwrap();
    assert!(work::claim(&mut client, Kind::DayBlock, &new, "w9", HOUR).await.unwrap().is_none());
    assert_eq!(count(&client, "SELECT count(*) FROM core.notice WHERE channel = 'alerts' AND message LIKE '🔴 Needs you: a full recompute of Dec 9 %names no measurements%'").await, 1);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE represented_date = '2025-12-09' AND state = 'PUBLISHED'").await, 2);
    scratch.drop().await;
}
