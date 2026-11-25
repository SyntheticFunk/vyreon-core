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

/// Two days published by the old build; the new build (its M0-GAMMA-CHANGE method moved on) publishes a
/// third. Nothing is compared before it has published a whole day, nor while a block of the old build
/// drains. Then: the differing day not being computed (the second has the new build's block queued over
/// it) is planned for M0-GAMMA-CHANGE as the C-074 planner chooses. Held without a watched spend, and
/// when the spend plus its cost would pass the ceiling, each said on both channels; scheduled within
/// it, once.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_new_builds_method_change_is_recomputed_within_the_ceiling() {
    let (scratch, mut client) = ready("c079_recompute").await;
    let old = admitted(&mut client, "slice-old-image").await;
    publish_day(&mut client, &old, DAYS[0], false).await;
    publish_day(&mut client, &old, DAYS[1], false).await;
    let new = admitted(&mut client, "slice-new-image").await;
    assert_eq!(recompute::due(&client, PROFILE).await.unwrap(), None, "nothing of the new build published yet");
    publish_day(&mut client, &new, DAYS[2], true).await;
    assert_eq!(recompute::due(&client, PROFILE).await.unwrap(), Some((new.clone(), vec!["base", "day-local"])));
    // the second day is being computed again under the new build: left alone
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &new, DAYS[1], DAYS[1], 1).await.unwrap();

    let p = recompute::propose(&client, PROFILE, &new, &["base", "day-local", "history"], None, 3, "2026-11-24").await.unwrap();
    assert_eq!((p.layers.clone(), p.waiting.clone(), p.busy.clone()), (vec!["base".to_owned(), "day-local".to_owned()], vec!["history".to_owned()], vec![DAYS[1].to_owned()]));
    assert_eq!(p.groups.len(), 1);
    let g = &p.groups[0];
    assert_eq!((g.names.clone(), g.days.clone()), (vec![CHANGED.to_owned()], vec![DAYS[0].to_owned()]));
    let plan = g.plan.as_ref().unwrap();
    assert!(g.schedulable && g.seconds > 0.0);
    assert_eq!(g.seconds, if g.choice == Some(core_db::republish::Choice::Scoped) { plan.scoped_total() } else { plan.full_total() });
    let lines = recompute::describe(&p, 0.167);
    assert_eq!(lines[0], "Build slice-old-image's methods (base, day-local compared) differ on 1 published days".replace("slice-old-image", &new));
    assert!(lines[1].starts_with(&format!(" · [{CHANGED}] on 1 days ({}): ", DAYS[0])), "{lines:?}");
    assert!(lines.iter().any(|l| l == " 1 days being computed (unfinished day blocks or republishes) left alone"), "{lines:?}");
    // a day before VYREON_CORE_RECOMPUTE_FROM is left alone
    let later = recompute::propose(&client, PROFILE, &new, &["base", "day-local"], Some(DAYS[1]), 3, "2026-11-24").await.unwrap();
    assert!(later.groups.is_empty() && later.before == 1);

    // no spend watched: held, both channels told, nothing queued
    let w = rw();
    let jobs = "SELECT count(*) FROM core.job WHERE engine_build = 'slice-new-image' AND kind IN ('REPUBLISH', 'DAY_BLOCK') AND state = 'QUEUED' AND first_session = '2025-12-09'";
    let d = recompute::auto(&mut client, PROFILE, &w, None, NOW).await.unwrap().unwrap();
    assert_eq!((d.outcome, d.jobs), ("HELD", 0));
    assert!(d.why.unwrap().contains("the spend is not watched"));
    assert_eq!(count(&client, jobs).await, 0);
    assert!(recompute::auto(&mut client, PROFILE, &w, None, NOW).await.unwrap().is_none(), "decided once");
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'alerts' AND message LIKE '🔴 Recompute%'", &[]).await.unwrap().get(0);
    assert!(alert.contains("not scheduled: the spend is not watched") && alert.contains(&format!("vyreon-core recompute {PROFILE} {new} --plan")), "{alert}");

    // the spend known: past the ceiling with the plan, held; within it, scheduled
    let balance = json!({"month_to_date_usage": "269.04", "generated_at": "2026-11-24T06:30:00Z"});
    let r = spend::from_answers(&balance, &json!({"invoices": []}), "2026-11").unwrap();
    spend::put_reading(&client, Ok(&r)).await.unwrap();
    client.execute("UPDATE core.spend_reading SET read_at = '2026-11-24 06:42:00+00'", &[]).await.unwrap();
    let cost = p.seconds() / 3600.0 * 0.167;
    let tight = SpendWatch { since: "2026-11-21T06:09:00Z".into(), baseline_usd: 180.41, ceiling_usd: 88.63 + cost / 2.0 };
    client.execute("DELETE FROM core.recompute_plan", &[]).await.unwrap();
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&tight), NOW).await.unwrap().unwrap();
    assert_eq!(d.outcome, "HELD");
    assert!(d.why.as_deref().unwrap().starts_with("$88.63 spent and $"), "{:?}", d.why);
    assert_eq!(count(&client, jobs).await, 0);
    client.execute("DELETE FROM core.recompute_plan", &[]).await.unwrap();
    let ok = SpendWatch { ceiling_usd: 500.0, ..tight };
    let d = recompute::auto(&mut client, PROFILE, &w, Some(&ok), NOW).await.unwrap().unwrap();
    assert_eq!((d.outcome, d.why.clone()), ("SCHEDULED", None));
    assert!(d.jobs >= 1);
    assert_eq!(count(&client, jobs).await, 1, "the day's republish (SCOPED) or its day block (FULL)");
    let progress: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'progress' AND message LIKE '♻️ Recompute% scheduled: %' ", &[]).await.unwrap().get(0);
    assert!(progress.contains(&format!("[{CHANGED}] on 1 days")), "{progress}");
    assert!(recompute::auto(&mut client, PROFILE, &w, Some(&ok), NOW).await.unwrap().is_none(), "once");
    assert_eq!(recompute::due(&client, PROFILE).await.unwrap(), None);
    scratch.drop().await;
}
