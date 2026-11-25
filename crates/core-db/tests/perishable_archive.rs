//! The perishable archive (C-052) in the database: a day block asks to archive one object of a day
//! it published, core records the request and signs an upload URL for a content-addressed key,
//! reads the object back before it is KEPT, keeps another version beside the first until the
//! session's archive is final, never stores the URL, and counts what is missing. The objects are
//! real files core treats as opaque bytes (AAPL's reduced tape of 2025-12-10 and the macro capture
//! of 2025-12-03, as in the input-capture tests); the day is the real slice of 2025-12-10. Run with
//! `cargo test -- --ignored`.
use core_db::archive::{self, Answer, Head, Request, Spaces};
use core_db::publish::publish_for_lease;
use core_db::watch::{self, ArchiveWatch, Settings};
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use md5::{Digest, Md5};
use serde_json::json;
use test_support::slice::{BASE, bundle, inventories, parts};
use test_support::{MACRO_CAPTURE, Scratch, TAPE_AAPL};
use tokio_postgres::Client;

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;
const SECRET: &str = "the-spaces-secret-of-this-test";

fn spaces() -> Spaces {
    // never contacted here: the read-back is given as a `Head`
    Spaces::new("https://nyc3.digitaloceanspaces.com", "nyc3", "vyreon-data", "DO00ARCHIVETESTKEY", SECRET).unwrap()
}

fn request(category: &str, session: &str, bytes: &[u8]) -> Request {
    let md5: String = Md5::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    Request { category: category.into(), session: session.into(), bytes: bytes.len() as i64, sha256: core_contract::sha256_hex(bytes), md5 }
}

/// What the bucket answers for an object holding `bytes` uploaded with the signed headers.
fn head_of(bytes: &[u8]) -> Head {
    let r = request("act01-auto", "2025-12-10", bytes);
    Head { status: 200, bytes: Some(r.bytes), etag: Some(format!("\"{}\"", r.md5)), sha256: Some(r.sha256) }
}

async fn at(client: &Client, sql: &str) -> String {
    client.query_one(&format!("SELECT ({sql})::text"), &[]).await.unwrap().get(0)
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_published_days_perishables_are_requested_verified_kept_and_frozen() {
    let scratch = Scratch::new("archive").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for (i, inventory) in inventories().into_iter().enumerate() {
        // the base build publishes the day's day-local revision as well (C-031)
        let inventory = if i == 0 { test_support::slice::with_day_local(inventory) } else { test_support::slice::reading(inventory) };
        let b = build::register(&mut client, &inventory).await.unwrap().engine_build;
        build::admit(&client, &b, &json!({"checklist": "archive test"})).await.unwrap();
        builds.push(b);
    }
    let (base_build, derived_build) = (builds[0].clone(), builds[1].clone());
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &derived_build, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &base_build, "w1", HOUR).await.unwrap().unwrap();
    let s = spaces();
    let refused = |r: Result<Answer, LeaseError>, says: &str| assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains(says)), "{r:?} should say {says}");

    // a day not published archives nothing
    let early = "2025-12-11 21:00:00+00";
    refused(archive::request(&mut client, &s, &block.token, &request("act01-auto", "2025-12-10", TAPE_AAPL), early).await, "has published");
    let (header, scalars, files) = parts(BASE);
    let path = bundle("archive-base", &header, &scalars, &files);
    let base = publish_for_lease(&mut client, &path, &block.token).await.unwrap().revision_id;
    std::fs::remove_file(&path).unwrap();
    let (h, s2, f) = test_support::slice::day_local_parts(&base_build, "2025-12-10", base);
    let path = bundle("archive-day-local", &h, &s2, &f);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    // published, not yet done (audit round x, 9): the block's current day is not one it has
    // published until it says so; the worker archives after `day_done`
    refused(archive::request(&mut client, &s, &block.token, &request("act01-auto", "2025-12-10", TAPE_AAPL), early).await, "has published");
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();

    // the request is recorded and answered with an upload URL for its content-addressed key,
    // valid ten minutes from the database's now, signing the length, Content-MD5 and SHA-256
    let first = request("act01-auto", "2025-12-10", TAPE_AAPL);
    let now = "2025-12-11 13:00:00+00";
    let Answer::Upload { archive_id, key, url, headers, expires_seconds } = archive::request(&mut client, &s, &block.token, &first, now).await.unwrap() else { panic!("an upload") };
    assert_eq!(key, format!("perishables/act01-auto/2025-12-10/{}.tar.zst", first.sha256));
    assert_eq!(expires_seconds, 600);
    assert!(url.starts_with(&format!("https://nyc3.digitaloceanspaces.com/vyreon-data/{key}?X-Amz-Algorithm=AWS4-HMAC-SHA256&")), "{url}");
    for part in ["X-Amz-Date=20251211T130000Z&", "X-Amz-Expires=600&", "X-Amz-SignedHeaders=content-length%3Bcontent-md5%3Bhost%3Bx-amz-meta-sha256&", "X-Amz-Signature="] {
        assert!(url.contains(part), "{part}");
    }
    let md5_b64 = headers.iter().find(|(k, _)| k == "content-md5").unwrap().1.clone();
    assert_eq!(md5_b64.len(), 24);
    assert_eq!(headers.iter().find(|(k, _)| k == archive::META_SHA256).unwrap().1, first.sha256);
    // the debug form never shows the URL
    assert!(!format!("{:?}", archive::request(&mut client, &s, &block.token, &first, now).await.unwrap()).contains("X-Amz-Signature"));
    let (state, issued, valid): (String, i32, f64) = {
        let r = client
            .query_one("SELECT state, urls_issued, extract(epoch FROM url_expires_at - requested_at)::float8 FROM core.perishable_archive WHERE archive_id = $1", &[&archive_id])
            .await
            .unwrap();
        (r.get(0), r.get(1), r.get(2))
    };
    assert_eq!((state.as_str(), issued, valid), ("REQUESTED", 2, 600.0));
    // the URL (a credential) is stored nowhere
    let signature = url.rsplit_once("X-Amz-Signature=").unwrap().1.to_owned();
    let everything = at(&client, "SELECT string_agg(row_to_json(a)::text, ' ') FROM core.perishable_archive a").await;
    assert!(!everything.contains(&signature) && !everything.contains("X-Amz") && !everything.contains(SECRET));

    // read back: kept only when the bucket holds the archive requested
    let t = archive::target(&client, &block.token, archive_id).await.unwrap();
    assert_eq!(archive::verify(&t, &head_of(TAPE_AAPL)), Ok(()));
    assert!(archive::verify(&t, &head_of(MACRO_CAPTURE)).is_err());
    assert!(archive::record(&mut client, &block.token, &t, &archive::verify(&t, &head_of(TAPE_AAPL)), "2025-12-11 13:01:00+00").await.unwrap());
    // kept: the same archive again is answered kept, with no URL; a late failure report does not undo it
    assert_eq!(archive::request(&mut client, &s, &block.token, &first, now).await.unwrap(), Answer::Kept { archive_id, key: key.clone() });
    assert!(archive::record(&mut client, &block.token, &t, &Err("late".into()), now).await.unwrap());

    // another version within the session's first 14 days is another object, beside the first
    let second = request("act01-auto", "2025-12-10", MACRO_CAPTURE);
    let Answer::Upload { archive_id: second_id, key: second_key, .. } = archive::request(&mut client, &s, &block.token, &second, "2025-12-12 13:00:00+00").await.unwrap() else { panic!("an upload") };
    assert_ne!(second_key, key);
    // the bucket holds something else under its key: FAILED, with what was found
    let t2 = archive::target(&client, &block.token, second_id).await.unwrap();
    let wrong = archive::verify(&t2, &head_of(TAPE_AAPL));
    assert!(wrong.as_ref().unwrap_err().contains("bytes"), "{wrong:?}");
    assert!(!archive::record(&mut client, &block.token, &t2, &wrong, "2025-12-12 13:01:00+00").await.unwrap());
    // a worker's failure carrying the URL is not kept as it was said
    let said = format!("PUT {url}: 403");
    assert!(!archive::record(&mut client, &block.token, &t2, &Err(format!("the worker: {said}")), "2025-12-12 13:02:00+00").await.unwrap());
    let failure = at(&client, &format!("SELECT failure FROM core.perishable_archive WHERE archive_id = {second_id}")).await;
    assert_eq!(failure, "(the reason is not kept: it carried a credential pattern)");
    // asked again, a new URL; the same SHA-256 with another size is refused
    let again = archive::request(&mut client, &s, &block.token, &second, "2025-12-12 14:00:00+00").await.unwrap();
    assert!(matches!(again, Answer::Upload { archive_id, .. } if archive_id == second_id), "{again:?}");
    refused(archive::request(&mut client, &s, &block.token, &Request { bytes: second.bytes + 1, ..second.clone() }, now).await, "this request differs");
    // more than 14 days after the session, with a version kept, the archive is final
    let today = at(&client, "now()").await;
    assert_eq!(archive::request(&mut client, &s, &block.token, &second, &today).await.unwrap(), Answer::Frozen { key: key.clone() });

    // what is refused: another category, a bad digest, an empty archive, a day not published, a
    // pass's lease, a gone lease
    refused(archive::request(&mut client, &s, &block.token, &request("company-capture", "2025-12-10", TAPE_AAPL), now).await, "not a perishable category");
    refused(archive::request(&mut client, &s, &block.token, &Request { sha256: "A".repeat(64), ..first.clone() }, now).await, "lowercase hex");
    refused(archive::request(&mut client, &s, &block.token, &Request { bytes: 0, ..first.clone() }, now).await, "1 to");
    refused(archive::request(&mut client, &s, &block.token, &request("exq01-events", "2025-12-09", TAPE_AAPL), now).await, "has published");
    assert_eq!(archive::request(&mut client, &s, "not-a-lease", &first, now).await, Err(LeaseError::Gone));

    // missing: what was requested and never kept (the second version is FAILED, but its
    // category has a kept version: nothing is missing), then, expected from 2025-12-10, the
    // day's cohort prints that were never requested
    let later = at(&client, "now() + interval '3 days'").await;
    assert_eq!(archive::missing(&client, PROFILE, None, &later).await.unwrap(), []);
    let expected = archive::missing(&client, PROFILE, Some("2025-12-10"), &later).await.unwrap();
    assert_eq!(expected.iter().map(|m| (m.session.as_str(), m.category.as_str())).collect::<Vec<_>>(), [("2025-12-10", "exq01-events")]);
    assert!((3 * 86_400 - 60..=3 * 86_400 + 600).contains(&expected[0].age), "{expected:?}");
    // within the grace after publishing, nothing is missing yet
    assert_eq!(archive::missing(&client, PROFILE, Some("2025-12-10"), &at(&client, "now() + interval '10 minutes'").await).await.unwrap(), []);
    let listed = archive::list(&client, Some("2025-12-10"), 10).await.unwrap();
    assert_eq!(listed.iter().map(|r| r["state"].as_str().unwrap()).collect::<Vec<_>>(), ["KEPT", "REQUESTED"]);
    assert!(archive::list(&client, Some("yesterday"), 10).await.is_err());

    // the watch: counted in the digest; a problem only past the configured age, resolved once kept
    let mut settings = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None, refcheck_from: None, backup_days: None, spend: None, recompute: None, refcheck_rerun_hours: None };
    settings.archive = Some(ArchiveWatch { expected_from: Some("2025-12-10".into()), alert_days: None });
    let digest = watch::digest(&mut client, &settings, &later).await.unwrap();
    assert!(digest.contains("Perishable archive: 1 missing (the oldest 2025-12-10 exq01-events, 3 days)"), "{digest}");
    // (the engine-v1 calendar's end is near at `later`: only the archive's problem is looked at)
    let archive_lines = |lines: Vec<String>| lines.into_iter().filter(|l| l.contains("Perishable archive")).collect::<Vec<_>>();
    assert!(archive_lines(watch::check(&mut client, &settings, &later).await.unwrap().opened).is_empty());
    settings.archive = Some(ArchiveWatch { expected_from: Some("2025-12-10".into()), alert_days: Some(4) });
    assert!(archive_lines(watch::check(&mut client, &settings, &later).await.unwrap().opened).is_empty(), "3 days is not past 4");
    settings.archive = Some(ArchiveWatch { expected_from: Some("2025-12-10".into()), alert_days: Some(2) });
    let opened = archive_lines(watch::check(&mut client, &settings, &later).await.unwrap().opened);
    assert_eq!(opened.len(), 1);
    assert!(opened[0].contains("Perishable archive: 1 missing for more than 2 days, the oldest 2025-12-10 exq01-events"), "{opened:?}");
    let prints = request("exq01-events", "2025-12-10", MACRO_CAPTURE);
    let Answer::Upload { archive_id: prints_id, .. } = archive::request(&mut client, &s, &block.token, &prints, &later).await.unwrap() else { panic!("an upload") };
    let t3 = archive::target(&client, &block.token, prints_id).await.unwrap();
    assert!(archive::record(&mut client, &block.token, &t3, &Ok(()), &later).await.unwrap());
    assert_eq!(archive_lines(watch::check(&mut client, &settings, &later).await.unwrap().resolved).len(), 1);
    assert!(watch::digest(&mut client, &settings, &later).await.unwrap().contains("Perishable archive: nothing missing"));

    // only the requesting job's live lease reports; a day block's lease ended reports nothing
    // the worker archives after `day_done`: the block's last day done, its lease still asks
    assert_eq!(archive::request(&mut client, &s, &block.token, &first, now).await.unwrap(), Answer::Kept { archive_id, key: key.clone() });
    work::finish(&mut client, &block.token).await.unwrap();
    assert_eq!(archive::record(&mut client, &block.token, &t, &Ok(()), &later).await, Err(LeaseError::Gone));
    let pass = work::claim(&mut client, Kind::OrderedPass, &derived_build, "p1", HOUR).await.unwrap().unwrap();
    refused(archive::request(&mut client, &s, &pass.token, &first, now).await, "not a day block");
    assert!(matches!(archive::target(&client, &pass.token, archive_id).await, Err(LeaseError::Refused(ref m)) if m.contains("not one job")));
    drop(client);
    scratch.drop().await;
}

/// A day block of the real slice of 2025-12-10 that has published its base: its lease token.
async fn published_block(scratch: &Scratch, test: &str) -> (Client, String) {
    published_block_at(scratch, test, migrate::embedded().unwrap().len()).await
}

/// `published_block` on a database brought to its first `migrations` migrations.
async fn published_block_at(scratch: &Scratch, test: &str, migrations: usize) -> (Client, String) {
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()[..migrations]).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    // C-070: the block's day is done with its base and its day-local revision (C-031)
    let b = build::register(&mut client, &test_support::slice::with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "archive test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle(&format!("archive-{test}"), &header, &scalars, &files);
    let leased = migrations == migrate::embedded().unwrap().len();
    // on an older schema the publish of a later one (0044 records the lease that published a
    // revision) cannot run: the day is published by hand, which the archive's checks take
    // alike (they read the block's build's published base, not who published it)
    let base = if leased { publish_for_lease(&mut client, &path, &block.token).await.unwrap() } else { core_db::publish::publish_file(&mut client, &path).await.unwrap() }.revision_id;
    std::fs::remove_file(&path).unwrap();
    let (h, s, f) = test_support::slice::day_local_parts(&b, "2025-12-10", base);
    let path = bundle(&format!("archive-{test}-day-local"), &h, &s, &f);
    if leased {
        publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    } else {
        core_db::publish::publish_file(&mut client, &path).await.unwrap();
    }
    std::fs::remove_file(&path).unwrap();
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    (client, block.token)
}

/// Audit 2026-11-17 round v, 3: an archive is read back only where its URL was signed for. Asked
/// under one bucket, reported after the settings moved to another: FAILED, not verified in the
/// other; asked again, the row moves to the new place with its new URL and is kept there.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_archive_is_read_back_only_where_its_url_was_signed_for() {
    let scratch = Scratch::new("archive_place").await;
    let (mut client, token) = published_block(&scratch, "place").await;
    let a = spaces();
    let b = Spaces::new("https://nyc3.digitaloceanspaces.com", "nyc3", "vyreon-other", "DO00ARCHIVETESTKEY", SECRET).unwrap();
    let elsewhere = Spaces::new("https://sfo3.digitaloceanspaces.com", "sfo3", "vyreon-data", "DO00ARCHIVETESTKEY", SECRET).unwrap();
    let r = request("act01-auto", "2025-12-10", TAPE_AAPL);
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &a, &token, &r, "2025-12-11 13:00:00+00").await.unwrap() else { panic!("an upload") };
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert_eq!((t.endpoint.as_deref(), t.bucket.as_str()), (Some("https://nyc3.digitaloceanspaces.com"), "vyreon-data"));
    assert_eq!(archive::same_place(&t, &a), Ok(()));
    // the settings changed (another bucket, or the same name at another endpoint): not read back
    for other in [&b, &elsewhere] {
        let why = archive::same_place(&t, other).unwrap_err();
        assert!(why.contains("signed for https://nyc3.digitaloceanspaces.com/vyreon-data") && why.contains("not read back"), "{why}");
    }
    let why = archive::same_place(&t, &b);
    assert!(!archive::record(&mut client, &token, &t, &why, "2025-12-11 13:01:00+00").await.unwrap());
    // asked again under the new settings: the same row, now in the new bucket, its URL signed there
    let Answer::Upload { archive_id: again, url, .. } = archive::request(&mut client, &b, &token, &r, "2025-12-11 13:05:00+00").await.unwrap() else { panic!("an upload") };
    assert_eq!(again, archive_id);
    assert!(url.starts_with("https://nyc3.digitaloceanspaces.com/vyreon-other/perishables/"), "{url}");
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert_eq!(t.bucket, "vyreon-other");
    assert_eq!(archive::same_place(&t, &b), Ok(()));
    assert!(archive::same_place(&t, &a).is_err());
    assert!(archive::record(&mut client, &token, &t, &archive::verify(&t, &head_of(TAPE_AAPL)), "2025-12-11 13:06:00+00").await.unwrap());
    let listed = archive::list(&client, Some("2025-12-10"), 10).await.unwrap();
    assert_eq!((listed[0]["bucket"].as_str(), listed[0]["endpoint"].as_str(), listed[0]["state"].as_str()), (Some("vyreon-other"), Some("https://nyc3.digitaloceanspaces.com"), Some("KEPT")));
    drop(client);
    scratch.drop().await;
}

/// Audit 2026-11-17 round v, 5: asking again for a URL does not reset the age a missing archive is
/// counted from: it stays missing, and an `ARCHIVE_MISSING` problem stays open, until it is kept.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn asking_again_does_not_reset_a_missing_archives_age() {
    let scratch = Scratch::new("archive_age").await;
    let (mut client, token) = published_block(&scratch, "age").await;
    let s = spaces();
    let r = request("exq01-events", "2025-12-10", TAPE_AAPL);
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &s, &token, &r, "2025-12-11 13:00:00+00").await.unwrap() else { panic!("an upload") };
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert!(!archive::record(&mut client, &token, &t, &Err("the worker: the bucket answered 503".into()), "2025-12-11 13:01:00+00").await.unwrap());
    let day_later = "2025-12-12 14:00:00+00";
    let ages = |m: Vec<archive::Missing>| m.into_iter().map(|m| (m.session, m.category, m.age)).collect::<Vec<_>>();
    let missing = ages(archive::missing(&client, PROFILE, None, day_later).await.unwrap());
    assert_eq!(missing, [("2025-12-10".to_owned(), "exq01-events".to_owned(), 25 * 3600)]);
    // the watch, alerting past a day: open
    let settings = Settings {
        profile: PROFILE.into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: Some(ArchiveWatch { expected_from: None, alert_days: Some(1) }),
        refcheck_from: None,
        backup_days: None, spend: None, recompute: None,
        refcheck_rerun_hours: None,
    };
    let archive_lines = |lines: Vec<String>| lines.into_iter().filter(|l| l.contains("Perishable archive")).collect::<Vec<_>>();
    assert_eq!(archive_lines(watch::check(&mut client, &settings, day_later).await.unwrap().opened).len(), 1);
    // asked again (and failing again): still missing, aged from the first request; still open
    let again = archive::request(&mut client, &s, &token, &r, day_later).await.unwrap();
    assert!(matches!(again, Answer::Upload { archive_id: id, .. } if id == archive_id), "{again:?}");
    assert_eq!(ages(archive::missing(&client, PROFILE, None, day_later).await.unwrap()), missing);
    let change = watch::check(&mut client, &settings, day_later).await.unwrap();
    assert!(archive_lines(change.resolved).is_empty() && archive_lines(change.opened).is_empty());
    let requested = at(&client, &format!("SELECT requested_at AT TIME ZONE 'UTC' FROM core.perishable_archive WHERE archive_id = {archive_id}")).await;
    assert_eq!(requested, "2025-12-11 13:00:00");
    // kept: no longer missing, resolved
    assert!(archive::record(&mut client, &token, &t, &Ok(()), day_later).await.unwrap());
    assert_eq!(archive_lines(watch::check(&mut client, &settings, day_later).await.unwrap().resolved).len(), 1);
    drop(client);
    scratch.drop().await;
}

/// Audit 2026-11-17 round w, 17: a row KEPT before migration 0022 names no endpoint, so where it
/// was read back is not recorded: migration 0023 counts it as not kept (missing, and asked again
/// it is given a URL), and no row can be KEPT without its endpoint from then on.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_row_kept_before_its_endpoint_was_recorded_is_not_counted_as_kept() {
    let scratch = Scratch::new("archive_legacy").await;
    let (mut client, token) = published_block_at(&scratch, "legacy", 22).await;
    let s = spaces();
    let r = request("act01-auto", "2025-12-10", TAPE_AAPL);
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &s, &token, &r, "2025-12-11 13:00:00+00").await.unwrap() else { panic!("an upload") };
    // as a row kept before 0022 was: KEPT, its endpoint never recorded
    client
        .execute("UPDATE core.perishable_archive SET state = 'KEPT', kept_at = '2025-12-11 13:01:00+00', endpoint = NULL WHERE archive_id = $1", &[&archive_id])
        .await
        .unwrap();
    let later = "2025-12-11 15:00:00+00";
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    let missing: Vec<(String, String)> = archive::missing(&client, PROFILE, None, later).await.unwrap().into_iter().map(|m| (m.session, m.category)).collect();
    assert_eq!(missing, [("2025-12-10".to_owned(), "act01-auto".to_owned())]);
    let listed = archive::list(&client, Some("2025-12-10"), 10).await.unwrap();
    assert_eq!(listed[0]["state"], "FAILED");
    assert!(listed[0]["failure"].as_str().unwrap().contains("not counted as kept"));
    let again = archive::request(&mut client, &s, &token, &r, later).await.unwrap();
    assert!(matches!(again, Answer::Upload { archive_id: id, .. } if id == archive_id), "{again:?}");
    let refused = client.execute("UPDATE core.perishable_archive SET state = 'KEPT', kept_at = now(), endpoint = NULL WHERE archive_id = $1", &[&archive_id]).await;
    assert!(refused.is_err(), "no row is KEPT without its endpoint");
    drop(client);
    scratch.drop().await;
}

/// Audit 2026-11-17 round w, 7: a category the worker could not pack is reported, so it is counted
/// missing although nothing was requested and no expected-from session is set; only a day block
/// that published the day may report it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_category_not_packed_is_counted_missing() {
    let scratch = Scratch::new("archive_unpacked").await;
    let (mut client, token) = published_block(&scratch, "unpacked").await;
    let now = "2025-12-11 13:00:00+00";
    let why = "not packed: not packed by the archive's deadline: the tar was still being written (zstd killed)";
    archive::unpacked(&mut client, &token, "exq01-events", "2025-12-10", why, now).await.unwrap();
    for (category, session, says) in [("exq01-events", "2025-12-11", "has published"), ("company", "2025-12-10", "not a perishable category"), ("act01-auto", "12/10/2025", "not a date")] {
        let r = archive::unpacked(&mut client, &token, category, session, why, now).await;
        assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains(says)), "{r:?}");
    }
    assert!(archive::missing(&client, PROFILE, None, "2025-12-11 13:30:00+00").await.unwrap().is_empty(), "inside the grace hour");
    let missing: Vec<(String, String, i64)> = archive::missing(&client, PROFILE, None, "2025-12-11 14:00:00+00").await.unwrap().into_iter().map(|m| (m.session, m.category, m.age)).collect();
    assert_eq!(missing, [("2025-12-10".to_owned(), "exq01-events".to_owned(), 3600)]);
    let listed = archive::unpacked_list(&client, Some("2025-12-10"), 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].1.as_str(), listed[0].2.as_str(), listed[0].4.as_str()), ("exq01-events", why, "w1"));
    drop(client);
    scratch.drop().await;
}

/// Audit round x, 1: an outcome read back in one place is not recorded on an archive asked again in
/// another since (a core that lost its single-instance lock, still answering, beside the one that
/// took it): the row stays as the newer request left it; read back there, it is kept.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_outcome_read_in_the_old_place_is_not_recorded_on_an_archive_moved_since() {
    let scratch = Scratch::new("archive_moved").await;
    let (mut client, token) = published_block(&scratch, "moved").await;
    let (a, b) = (spaces(), Spaces::new("https://nyc3.digitaloceanspaces.com", "nyc3", "vyreon-other", "DO00ARCHIVETESTKEY", SECRET).unwrap());
    let r = request("act01-auto", "2025-12-10", TAPE_AAPL);
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &a, &token, &r, "2025-12-11 13:00:00+00").await.unwrap() else { panic!("an upload") };
    // the stale core reads it in bucket A; meanwhile the other core moves it to bucket B
    let stale = archive::target(&client, &token, archive_id).await.unwrap();
    let Answer::Upload { archive_id: again, .. } = archive::request(&mut client, &b, &token, &r, "2025-12-11 13:01:00+00").await.unwrap() else { panic!("an upload") };
    assert_eq!(again, archive_id);
    let verified = archive::verify(&stale, &head_of(TAPE_AAPL));
    assert_eq!(verified, Ok(()));
    let refused = archive::record(&mut client, &token, &stale, &verified, "2025-12-11 13:02:00+00").await;
    assert!(matches!(refused, Err(LeaseError::Refused(ref m)) if m.contains("asked again in another place")), "{refused:?}");
    assert_eq!(at(&client, &format!("SELECT state || ' ' || bucket FROM core.perishable_archive WHERE archive_id = {archive_id}")).await, "REQUESTED vyreon-other");
    // a failure read in the old place is not recorded either
    assert!(archive::record(&mut client, &token, &stale, &Err("the object is not in the bucket".into()), "2025-12-11 13:02:00+00").await.is_err());
    // read back where it now is: kept
    let current = archive::target(&client, &token, archive_id).await.unwrap();
    assert!(archive::record(&mut client, &token, &current, &archive::verify(&current, &head_of(TAPE_AAPL)), "2025-12-11 13:03:00+00").await.unwrap());
    // once kept, a late outcome of the old place changes nothing and says kept
    assert!(archive::record(&mut client, &token, &stale, &Ok(()), "2025-12-11 13:04:00+00").await.unwrap());
    drop(client);
    scratch.drop().await;
}

/// Audit round x, 8 and 17: a "not packed" report sent again after its answer was lost is one
/// record; a report of a category the same job and worker had KEPT (the child stopped after its
/// upload, before its outcome line) is not listed beside the KEPT archive, in either order.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_not_packed_report_is_one_record_and_none_beside_a_kept_archive() {
    let scratch = Scratch::new("archive_unpacked_once").await;
    let (mut client, token) = published_block(&scratch, "unpacked-once").await;
    let s = spaces();
    let now = "2025-12-11 13:00:00+00";
    let why = "not packed: the archive child ended without its outcome";
    // (17) the same report twice: one record
    archive::unpacked(&mut client, &token, "exq01-events", "2025-12-10", why, now).await.unwrap();
    archive::unpacked(&mut client, &token, "exq01-events", "2025-12-10", why, "2025-12-11 13:00:05+00").await.unwrap();
    let rows = at(&client, "SELECT count(*) FROM core.perishable_unpacked").await;
    assert_eq!(rows, "1");
    assert_eq!(archive::unpacked_list(&client, Some("2025-12-10"), 10).await.unwrap().len(), 1);
    // (8) KEPT after the report (the done call landed late): the report is superseded
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &s, &token, &request("exq01-events", "2025-12-10", MACRO_CAPTURE), now).await.unwrap() else { panic!("an upload") };
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert!(archive::record(&mut client, &token, &t, &archive::verify(&t, &head_of(MACRO_CAPTURE)), now).await.unwrap());
    assert!(archive::unpacked_list(&client, Some("2025-12-10"), 10).await.unwrap().is_empty());
    // (8) a report after KEPT (the child died before its outcome line): not listed either
    let Answer::Upload { archive_id, .. } = archive::request(&mut client, &s, &token, &request("act01-auto", "2025-12-10", TAPE_AAPL), now).await.unwrap() else { panic!("an upload") };
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert!(archive::record(&mut client, &token, &t, &archive::verify(&t, &head_of(TAPE_AAPL)), now).await.unwrap());
    archive::unpacked(&mut client, &token, "act01-auto", "2025-12-10", why, now).await.unwrap();
    assert!(archive::unpacked_list(&client, Some("2025-12-10"), 10).await.unwrap().is_empty());
    assert!(archive::missing(&client, PROFILE, None, "2025-12-12 13:00:00+00").await.unwrap().is_empty());
    drop(client);
    scratch.drop().await;
}

/// Audit round x, 9: a block just leased on a day another job published (or one published by
/// hand) has published nothing: it may neither archive that day nor report it not packed.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_block_archives_only_days_it_has_published() {
    let scratch = Scratch::new("archive_whose").await;
    let (mut client, token) = published_block(&scratch, "whose").await;
    work::finish(&mut client, &token).await.unwrap();
    let b = build::register(&mut client, &test_support::slice::with_day_local(inventories().remove(0))).await.unwrap().engine_build;
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let late = work::claim(&mut client, Kind::DayBlock, &b, "w2", HOUR).await.unwrap().unwrap();
    let r = archive::request(&mut client, &spaces(), &late.token, &request("act01-auto", "2025-12-10", TAPE_AAPL), "2025-12-11 13:00:00+00").await;
    assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains("has published")), "{r:?}");
    let u = archive::unpacked(&mut client, &late.token, "exq01-events", "2025-12-10", "not packed", "2025-12-11 13:00:00+00").await;
    assert!(matches!(u, Err(LeaseError::Refused(ref m)) if m.contains("has published")), "{u:?}");
    drop(client);
    scratch.drop().await;
}
