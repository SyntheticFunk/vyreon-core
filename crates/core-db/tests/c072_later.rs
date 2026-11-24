//! C-073: the C-072 audits' remaining items (rounds r2 and r4, Codex and Gemini), each in ordinary
//! terms: the progress channel names a set-aside block as it started, states a hand-over once (across
//! an upgrade and a window's end), counts only the jobs its own withdrawal stopped and keeps the admit
//! instruction for a successor withdrawn before it; a cause that comes during an open pause is alerted;
//! the log is on the owner's clock; a re-sent bundle is acknowledged only when it is the published
//! one; a day-local bundle cannot pass as a base. On the real slice of 2025-12-10 and the engine's
//! calendar. Run with `cargo test -- --ignored`.
use core_db::publish::publish_for_lease;
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, notice, progress, universe, watch};
use serde_json::json;
use test_support::Scratch;
use test_support::slice::{BASE, bundle, day_local_parts, inventories, parts, reading, with_day_local};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// A migrated database with the calendar, the universe and the slice's two builds (base admitted).
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
    let base = build::register(&mut client, &with_day_local(inv.remove(0))).await.unwrap().engine_build;
    build::admit(&client, &base, &json!({"checklist": "c072 later"})).await.unwrap();
    (scratch, client, base, derived)
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn now_plus(client: &Client, minutes: i32) -> String {
    client.query_one("SELECT (now() + make_interval(mins => $1))::text", &[&minutes]).await.unwrap().get(0)
}

/// r4 (Codex 1): a block that resumed on its second day and was set aside there is named in the
/// progress line as it started, 12-09 to 12-12 from 12-10, not as the one day its job was narrowed to.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_block_set_aside_is_named_as_it_started() {
    let (scratch, mut client, b, _) = ready("later_started").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-09", "2025-12-12", 30).await.unwrap();
    // its first day done by an earlier attempt; this one is its last
    client.execute("UPDATE core.job SET next_session = '2025-12-10', max_attempts = 1", &[]).await.unwrap();
    let before = now_plus(&client, -1).await;
    let claim = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    assert!(!work::fail(&mut client, &claim.token, "day 2025-12-10", "stop: 2025-12-10 not published: refused", "").await.unwrap());
    assert_eq!(count(&client, "SELECT count(*) FROM core.job WHERE state = 'FAILED' AND first_session = '2025-12-10' AND last_session = '2025-12-10'").await, 1, "narrowed");
    let end = now_plus(&client, 1).await;
    let lines = progress::lines(&mut client, PROFILE, &before, &end).await.unwrap();
    assert!(lines.iter().any(|l| l == "Started 1 block (2025): 12-09–12-12 (from 12-10)"), "{lines:?}");
    scratch.drop().await;
}

/// r4 (Codex 2, 4, 6; Gemini 1, 3): a hand-over is stated once: not again after an upgrade from a
/// core that recorded only its notice, nor twice when its notice and its row fall either side of a
/// window's end; "restarted" counts only what its own withdrawal stopped; a successor withdrawn before
/// it keeps the instruction to admit it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_hand_over_is_stated_once_with_what_it_did() {
    let (scratch, mut client, a, b) = ready("later_handover").await;
    work::schedule(&mut client, Kind::Tape, PROFILE, &a, "2025-12-10", "2025-12-11", 1).await.unwrap();
    // an earlier withdrawal stopped a running tape; the build admitted again, nothing runs now
    let claim = work::claim(&mut client, Kind::Tape, &a, "t1", HOUR).await.unwrap().unwrap();
    build::withdraw(&mut client, &a, "an earlier stop").await.unwrap();
    assert_eq!(work::heartbeat(&mut client, &claim.token, HOUR).await, Err(LeaseError::Gone));
    build::admit(&client, &a, &json!({"checklist": "again"})).await.unwrap();
    // the successor was admitted too early and withdrawn to let the hand-over through
    build::admit(&client, &b, &json!({"checklist": "too early"})).await.unwrap();
    build::withdraw(&mut client, &b, "admitted before its hand-over").await.unwrap();
    let t0 = now_plus(&client, -60).await;
    progress::tick(&mut client, PROFILE, &t0).await.unwrap();
    let token = build::hand_over(&mut client, &a, &b, "the rulings image", true, None).await.unwrap().token;
    let h = build::hand_over(&mut client, &a, &b, "the rulings image", false, Some(&token)).await.unwrap();
    let id = h.hand_over_id.unwrap();
    // its notice stamped before the window's end, its row after it (one transaction across the end)
    let end = now_plus(&client, 1).await;
    client.execute("UPDATE core.hand_over SET handed_at = $1::text::timestamptz + interval '1 second'", &[&end]).await.unwrap();
    client.execute("UPDATE core.notice SET created_at = $1::text::timestamptz - interval '1 second' WHERE message LIKE 'Hand-over %'", &[&end]).await.unwrap();
    let w = progress::tick(&mut client, PROFILE, &end).await.unwrap().unwrap();
    let posts: Vec<String> = client.query("SELECT message FROM core.notice WHERE channel = 'progress' ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    let text = posts.join("\n");
    let ours: Vec<&str> = text.lines().filter(|l| l.contains("Hand-over") || l.contains("Admit build")).collect();
    assert_eq!(ours.len(), 1, "{text}");
    assert!(ours[0].contains(&format!("Hand-over {id}: ")), "{}", ours[0]);
    assert!(
        ours[0].ends_with(&format!("2 tapes moved — the rulings image · build {b} is not admitted: admit it now (`vyreon-core build admit {b} <evidence.json>`)")),
        "no restart counted from the earlier stop; the successor withdrawn before keeps its instruction: {}",
        ours[0]
    );
    assert!(w.lines > 0);
    // the next window states nothing of it again
    let later = now_plus(&client, 40).await;
    progress::tick(&mut client, PROFILE, &later).await.unwrap();
    let again: Vec<String> = client
        .query("SELECT message FROM core.notice WHERE channel = 'progress' AND message LIKE '%Hand-over%' ORDER BY notice_id", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(again.len(), 1, "{again:?}");
    // the upgrade: a core before 0054 recorded only the hand-over's notice as stated
    client.execute("DELETE FROM core.progress_shown WHERE source = 'hand_over'", &[]).await.unwrap();
    let end = now_plus(&client, 70).await;
    let lines = progress::lines(&mut client, PROFILE, &t0, &end).await.unwrap();
    assert!(!lines.iter().any(|l| l.contains("Hand-over")), "{lines:?}");
    scratch.drop().await;
}

/// r4 (Gemini 2, pre-existing; owner: such an event must alert): a cause that comes while the queue is
/// already paused (a bundle refused, a job failed for the last time) is kept and alerted at once, once;
/// the watch's open QUEUE_PAUSED says it; the log is on the owner's clock.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_cause_during_an_open_pause_is_alerted() {
    let (scratch, mut client, _, _) = ready("later_pause").await;
    let pause = async |client: &mut Client, why: &str| {
        let tx = client.transaction().await.unwrap();
        work::pause(&tx, why).await.unwrap();
        tx.commit().await.unwrap();
    };
    let alerts = "SELECT count(*) FROM core.notice WHERE post AND channel = 'alerts'";
    let channel_alerts = count(&client, &alerts.replace(" AND channel = 'alerts'", "")).await;
    pause(&mut client, "hand-over to the rulings image").await;
    let s = watch::Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None,
        refcheck_rerun_hours: None,
    };
    let now = watch::now(&client).await.unwrap();
    watch::check(&mut client, &s, &now).await.unwrap();
    async fn posted(client: &Client) -> Vec<String> {
        client.query("SELECT message FROM core.notice WHERE post ORDER BY notice_id", &[]).await.unwrap().iter().map(|r| r.get::<_, String>(0)).collect()
    }
    let first = posted(&client).await;
    assert_eq!(first.len() as i64 - channel_alerts, 1, "the pause's alert: {first:?}");
    pause(&mut client, "job 7 (2025-12-10): a bundle refused: the day's members differ").await;
    pause(&mut client, "job 7 (2025-12-10): a bundle refused: the day's members differ").await;
    let after = posted(&client).await;
    assert_eq!(after.len(), first.len() + 1, "alerted at once, once: {after:?}");
    assert_eq!(after.last().unwrap(), "🔴 Work queue already paused; also: job 7 (2025-12-10): a bundle refused: the day's members differ\nTo see more: `vyreon-core queue`");
    let change = watch::check(&mut client, &s, &now).await.unwrap();
    assert!(change.opened.is_empty() && change.resolved.is_empty(), "{change:?}");
    assert_eq!(
        change.restated,
        ["Work queue paused, nothing is handed out: hand-over to the rulings image; also: job 7 (2025-12-10): a bundle refused: the day's members differ"]
    );
    // the log, on the owner's clock (C-072's fixed UTC−6)
    let log = notice::recent_log(&client, 1).await.unwrap();
    assert!(log[0].0.ends_with(" MT") && log[0].1.starts_with("Queue already paused; also: job 7"), "{log:?}");
    let utc: String = client.query_one("SELECT to_char(max(created_at) AT TIME ZONE 'Etc/GMT+6', 'YYYY-MM-DD HH24:MI:SS \"MT\"') FROM core.notice WHERE NOT post", &[]).await.unwrap().get(0);
    assert_eq!(log[0].0, utc);
    // resumed, the reason goes with it
    let tx = client.transaction().await.unwrap();
    let was = work::resume(&tx).await.unwrap().unwrap();
    tx.commit().await.unwrap();
    assert_eq!(was, "hand-over to the rulings image\nalso: job 7 (2025-12-10): a bundle refused: the day's members differ");
    scratch.drop().await;
}

/// r2 (Codex, pre-existing publish defects): a re-sent day is acknowledged only when it is the
/// published one (a changed reader scalar is refused); a day-local bundle stripped of its base fields
/// and `day_local` cannot pass as a second base of the day, nor its profile as a base later.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_bundle_is_what_it_says_it_is() {
    let (scratch, mut client, b, _) = ready("later_publish").await;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle("later-base", &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    assert!(publish_for_lease(&mut client, &path, &block.token).await.unwrap().already_published, "the same again: acknowledged");
    std::fs::remove_file(&path).unwrap();
    let mut changed = scalars.clone();
    changed.reader_scalar[0].value += 1.0;
    let path = bundle("later-base-changed", &header, &changed, &files);
    let refused = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.contains(&format!("is not published revision {}: its reader scalars differ", base.revision_id))), "{refused:?}");
    let served: f64 = client
        .query_one("SELECT value FROM vyreon.reader_scalar WHERE revision_id = $1 AND object_id = $2 AND measurement_id = $3 AND role_id = $4", &[&base.revision_id, &scalars.reader_scalar[0].object_id, &scalars.reader_scalar[0].measurement_id, &scalars.reader_scalar[0].role_id])
        .await
        .unwrap()
        .get(0);
    assert_eq!(served, scalars.reader_scalar[0].value);

    // the day-local bundle without its base fields: refused as a second base of the day
    let (mut lh, ls, lf) = day_local_parts(&b, "2025-12-10", base.revision_id);
    let proper = lh.clone();
    (lh.base_profile_revision, lh.base_revision, lh.day_local) = (None, None, false);
    let path = bundle("later-stripped", &lh, &ls, &lf);
    let refused = publish_for_lease(&mut client, &path, &block.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.contains("a second base in profile")), "{refused:?}");
    assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.day_revision WHERE revision_id = {} AND state = 'PUBLISHED'", base.revision_id)).await, 1, "the real base stands");
    // the proper one goes through; from then on its profile is a day-local one for this build
    let path = bundle("later-local", &proper, &ls, &lf);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    work::finish(&mut client, &block.token).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-11", "2025-12-11", 30).await.unwrap();
    let next = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    lh.represented_date = "2025-12-11".into();
    let path = bundle("later-stripped-2", &lh, &ls, &lf);
    let refused = publish_for_lease(&mut client, &path, &next.token).await.unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(&refused, LeaseError::Refused(m) if m.contains("is registered as its day-local profile; this bundle is presented as a base revision")), "{refused:?}");
    scratch.drop().await;
}

/// r4 (Codex, rare: a credential in a build's name reached the progress channel): an odd build name
/// is refused at registration without being repeated.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_build_is_named_as_the_engine_names_it() {
    let (scratch, mut client, _, _) = ready("later_name").await;
    let mut inv = with_day_local(inventories().remove(0));
    inv["engine_build"] = json!("abc+1 password=hunter2pass");
    let e = build::register(&mut client, &inv).await.unwrap_err();
    assert!(e.contains("not such a name") && !e.contains("hunter2"), "{e}");
    for ok in ["5f637983+8855e39e0be8", "d9462aad+65064dee5b99"] {
        assert_eq!(build::build_name(ok), Ok(()));
    }
    scratch.drop().await;
}
