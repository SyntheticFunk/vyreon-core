//! C-075 (coordinator, 2026-11-24; an observability gap: live, 2024-07-23 was refused by the engine's
//! input plan and core queued it again in silence): a day the input plan refuses is held at once and
//! alerted (DAY_REFUSED), never retried to the end; a refusal whose every input is a capture that
//! failed here retryably is retried once, then held; the problem resolves when the day is published.
//! The refusals are the engine's: the real refused day of 2024-07-16 (`close_quotes` missing, from
//! `test_support::WORKER_LOG_REFUSED_DAY`'s run) and the engine's text for a capture that failed here
//! (`plan::capture_failed`); the publication is the real slice of 2025-12-10. Run with
//! `cargo test -- --ignored`.
use core_db::evidence::{self, SCHEMA};
use core_db::publish::publish_for_lease;
use core_db::watch::{self, Settings};
use core_db::work::{self, Kind};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Write;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, with_day_local};
use test_support::{Scratch, WORKER_LOG_REFUSED_DAY};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const DAY: &str = "2025-12-10";

fn document(account: &Value) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(format!("{account}\n{WORKER_LOG_REFUSED_DAY}").as_bytes()).unwrap();
    gz.finish().unwrap()
}

fn stop(day: &str) -> String {
    format!("stop: {day} not published: {day} refused by the input plan (plan.json)")
}

/// The real refused day's refusal (2024-07-16: no quote file for its option attempts), on `day`.
fn quotes_refused(day: &str) -> Value {
    json!([{"input": "close_quotes", "session": day, "role": "T", "status": "missing", "blocking": true,
            "reason": "14016 published option attempts without a quote file", "day": day}])
}

/// An option input whose capture failed here retryably, as the engine's plan words it.
fn capture_failed(day: &str) -> Value {
    json!([{"input": "options", "session": day, "role": "T", "status": "missing", "blocking": true,
            "reason": "1 assets without a published attempt: 1 of them a capture that failed here, not a provider absence (D-262)",
            "assets": {"ETHA": "ENGINE_FAULT: capture failed here (options retryable), not a provider absence: no published attempt"}, "day": day}])
}

fn account(day: &str, refusals: Value) -> Value {
    json!({"schema": SCHEMA, "stage": format!("day {day}"), "message": stop(day), "session": day, "refusals": refusals})
}

async fn ready(test: &str) -> (Scratch, Client, String) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let base = build::register(&mut client, &with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    build::admit(&client, &base, &json!({"checklist": "refusal test"})).await.unwrap();
    (scratch, client, base)
}

fn settings() -> Settings {
    Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None, spend: None, recompute: None,
        refcheck_rerun_hours: None,
    }
}

async fn now(client: &Client) -> String {
    client.query_one("SELECT clock_timestamp()::text", &[]).await.unwrap().get(0)
}

async fn open(client: &Client, kind: &str) -> Vec<String> {
    client
        .query("SELECT summary FROM core.problem WHERE resolved_at IS NULL AND kind = $1 ORDER BY key", &[&kind])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

async fn state(client: &Client, first: &str) -> (String, i32) {
    let r = client.query_one("SELECT state, attempts FROM core.job WHERE kind = 'DAY_BLOCK' AND first_session = $1::text::date ORDER BY job_id DESC LIMIT 1", &[&first]).await.unwrap();
    (r.get(0), r.get(1))
}

/// Claims the block, leaves the attempt's evidence and reports the stop as the engine's worker does.
async fn refuse(client: &mut Client, build: &str, day: &str, refusals: Value) -> bool {
    let lease = work::claim(client, Kind::DayBlock, build, "w1", HOUR).await.unwrap().unwrap();
    evidence::put(client, &lease.token, &evidence::check(document(&account(day, refusals))).unwrap()).await.unwrap();
    work::fail(client, &lease.token, &format!("day {day}"), &stop(day), "== day.log").await.unwrap()
}

#[test]
fn the_refused_inputs_read_on_one_line_and_only_failed_captures_are_transient() {
    let (line, transient) = work::refusal_inputs(&quotes_refused("2024-07-16"));
    assert_eq!(line, "close_quotes (T 2024-07-16, missing) 14016 published option attempts without a quote file");
    assert!(!transient);
    let (line, transient) = work::refusal_inputs(&capture_failed("2024-07-16"));
    assert!(line.starts_with("options (T 2024-07-16, missing) ETHA: ENGINE_FAULT: capture failed here (options retryable)"), "{line}");
    assert!(transient);
    // a fatal capture, or the refusals not reported, hold at once
    let fatal = json!([{"input": "options", "session": "2024-07-16", "role": "T", "status": "missing", "assets": {"ETHA": "ENGINE_FAULT: capture failed here (options fatal), not a provider absence: x"}}]);
    assert!(!work::refusal_inputs(&fatal).1);
    assert!(!work::refusal_inputs(&Value::Null).1);
    // C-076 (engine D-464): job 21818's refusal of 2024-07-23, its carry capture outlasted by Intrinio's
    // 429s, as the engine's plan now states it (the fill's class of the day-wide capture): transient
    let carry = |how: &str| json!([{"input": "carry", "session": "2024-07-23", "role": "T", "status": "missing", "blocking": true,
                                    "reason": "missing", "capture": how, "day": "2024-07-23"}]);
    let (line, transient) = work::refusal_inputs(&carry("carry retryable"));
    assert_eq!(line, "carry (T 2024-07-23, missing; the fill: carry retryable) missing");
    assert!(transient);
    assert!(!work::refusal_inputs(&carry("carry fatal")).1);
    assert!(!work::refusal_inputs(&carry("carry: the fill did not run to its end, exit 1")).1);
    // the same entry before D-464 (no `capture`): nothing says it was the fill's, held
    let mut before = carry("");
    before[0].as_object_mut().unwrap().remove("capture");
    assert!(!work::refusal_inputs(&before).1);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refused_day_is_held_and_alerted_at_once_and_resolved_when_published() {
    let (scratch, mut client, build) = ready("refused_held").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, DAY, DAY, 30).await.unwrap();
    // refused: not queued again, its attempts spent
    assert!(!refuse(&mut client, &build, DAY, quotes_refused(DAY)).await, "held, not queued again");
    assert_eq!(state(&client, DAY).await.0, "FAILED");
    let s = settings();
    let at = now(&client).await;
    watch::check(&mut client, &s, &at).await.unwrap();
    let refused = open(&client, "DAY_REFUSED").await;
    assert_eq!(refused.len(), 1);
    assert!(refused[0].starts_with(&format!("Day {DAY} refused by the engine's input plan (job ")) && refused[0].contains("close_quotes (T 2025-12-10, missing)"), "{refused:?}");
    assert!(open(&client, "JOB_FAILED").await.is_empty(), "one alert for one cause");
    let alert: String = client.query_one("SELECT message FROM core.notice WHERE post AND message LIKE '%refused by the engine%' ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(alert.contains("To do:") && alert.contains("retry-failed"), "{alert}");

    // the operator's retry: the day is computed and published; the problem resolves
    assert_eq!(work::retry_failed(&client).await.unwrap(), 1);
    let lease = work::claim(&mut client, Kind::DayBlock, &build, "w2", HOUR).await.unwrap().unwrap();
    let (h, sc, f) = parts(BASE);
    let path = bundle("refused-base", &h, &sc, &f);
    let base = publish_for_lease(&mut client, &path, &lease.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (lh, ls, lf) = day_local_parts(&build, DAY, base);
    let path = bundle("refused-local", &lh, &ls, &lf);
    publish_for_lease(&mut client, &path, &lease.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::day_done(&mut client, &lease.token, DAY).await.unwrap();
    work::finish(&mut client, &lease.token).await.unwrap();
    let at = now(&client).await;
    watch::check(&mut client, &s, &at).await.unwrap();
    assert!(open(&client, "DAY_REFUSED").await.is_empty(), "resolved once the day is published");
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_refusal_of_failed_captures_is_retried_once_then_held() {
    let (scratch, mut client, build) = ready("refused_once").await;
    let day = "2025-12-09";
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &build, day, day, 30).await.unwrap();
    assert!(refuse(&mut client, &build, day, capture_failed(day)).await, "tried once more");
    assert_eq!(state(&client, day).await.0, "QUEUED");
    let s = settings();
    let at = now(&client).await;
    watch::check(&mut client, &s, &at).await.unwrap();
    assert!(open(&client, "DAY_REFUSED").await.is_empty(), "not held yet: no alert");
    // the same refusal again: held
    assert!(!refuse(&mut client, &build, day, capture_failed(day)).await);
    assert_eq!(state(&client, day).await.0, "FAILED");
    let at = now(&client).await;
    watch::check(&mut client, &s, &at).await.unwrap();
    let refused = open(&client, "DAY_REFUSED").await;
    assert!(refused.len() == 1 && refused[0].contains("ETHA: ENGINE_FAULT: capture failed here (options retryable)"), "{refused:?}");
    let rows: Vec<(bool, bool)> = client.query("SELECT transient, held FROM core.day_refusal ORDER BY refusal_id", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(rows, [(true, false), (true, true)]);
    scratch.drop().await;
}
