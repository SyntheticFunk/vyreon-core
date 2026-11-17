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
    for inventory in inventories() {
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
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();

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
    assert!(archive::record(&mut client, &block.token, archive_id, &archive::verify(&t, &head_of(TAPE_AAPL)), "2025-12-11 13:01:00+00").await.unwrap());
    // kept: the same archive again is answered kept, with no URL; a late failure report does not undo it
    assert_eq!(archive::request(&mut client, &s, &block.token, &first, now).await.unwrap(), Answer::Kept { archive_id, key: key.clone() });
    assert!(archive::record(&mut client, &block.token, archive_id, &Err("late".into()), now).await.unwrap());

    // another version within the session's first 14 days is another object, beside the first
    let second = request("act01-auto", "2025-12-10", MACRO_CAPTURE);
    let Answer::Upload { archive_id: second_id, key: second_key, .. } = archive::request(&mut client, &s, &block.token, &second, "2025-12-12 13:00:00+00").await.unwrap() else { panic!("an upload") };
    assert_ne!(second_key, key);
    // the bucket holds something else under its key: FAILED, with what was found
    let t2 = archive::target(&client, &block.token, second_id).await.unwrap();
    let wrong = archive::verify(&t2, &head_of(TAPE_AAPL));
    assert!(wrong.as_ref().unwrap_err().contains("bytes"), "{wrong:?}");
    assert!(!archive::record(&mut client, &block.token, second_id, &wrong, "2025-12-12 13:01:00+00").await.unwrap());
    // a worker's failure carrying the URL is not kept as it was said
    let said = format!("PUT {url}: 403");
    assert!(!archive::record(&mut client, &block.token, second_id, &Err(format!("the worker: {said}")), "2025-12-12 13:02:00+00").await.unwrap());
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
    let mut settings = Settings { profile: PROFILE.into(), deadlines_from: None, volume_bytes: None, deadline_hour: watch::DEADLINE_HOUR, daily: None, archive: None };
    settings.archive = Some(ArchiveWatch { expected_from: Some("2025-12-10".into()), alert_days: None });
    let digest = watch::digest(&client, &settings, &later).await.unwrap();
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
    assert!(archive::record(&mut client, &block.token, prints_id, &Ok(()), &later).await.unwrap());
    assert_eq!(archive_lines(watch::check(&mut client, &settings, &later).await.unwrap().resolved).len(), 1);
    assert!(watch::digest(&client, &settings, &later).await.unwrap().contains("Perishable archive: nothing missing"));

    // only the requesting job's live lease reports; a day block's lease ended reports nothing
    // the worker archives after `day_done`: the block's last day done, its lease still asks
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    assert_eq!(archive::request(&mut client, &s, &block.token, &first, now).await.unwrap(), Answer::Kept { archive_id, key: key.clone() });
    work::finish(&mut client, &block.token).await.unwrap();
    assert_eq!(archive::record(&mut client, &block.token, archive_id, &Ok(()), &later).await, Err(LeaseError::Gone));
    let pass = work::claim(&mut client, Kind::OrderedPass, &derived_build, "p1", HOUR).await.unwrap().unwrap();
    refused(archive::request(&mut client, &s, &pass.token, &first, now).await, "not a day block");
    assert!(matches!(archive::target(&client, &pass.token, archive_id).await, Err(LeaseError::Refused(ref m)) if m.contains("not one job")));
    drop(client);
    scratch.drop().await;
}

/// A day block of the real slice of 2025-12-10 that has published its base: its lease token.
async fn published_block(scratch: &Scratch, test: &str) -> (Client, String) {
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let b = build::register(&mut client, &inventories()[0]).await.unwrap().engine_build;
    build::admit(&client, &b, &json!({"checklist": "archive test"})).await.unwrap();
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &b, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &b, "w1", HOUR).await.unwrap().unwrap();
    let (header, scalars, files) = parts(BASE);
    let path = bundle(&format!("archive-{test}"), &header, &scalars, &files);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();
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
    assert!(!archive::record(&mut client, &token, archive_id, &why, "2025-12-11 13:01:00+00").await.unwrap());
    // asked again under the new settings: the same row, now in the new bucket, its URL signed there
    let Answer::Upload { archive_id: again, url, .. } = archive::request(&mut client, &b, &token, &r, "2025-12-11 13:05:00+00").await.unwrap() else { panic!("an upload") };
    assert_eq!(again, archive_id);
    assert!(url.starts_with("https://nyc3.digitaloceanspaces.com/vyreon-other/perishables/"), "{url}");
    let t = archive::target(&client, &token, archive_id).await.unwrap();
    assert_eq!(t.bucket, "vyreon-other");
    assert_eq!(archive::same_place(&t, &b), Ok(()));
    assert!(archive::same_place(&t, &a).is_err());
    assert!(archive::record(&mut client, &token, archive_id, &archive::verify(&t, &head_of(TAPE_AAPL)), "2025-12-11 13:06:00+00").await.unwrap());
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
    assert!(!archive::record(&mut client, &token, archive_id, &Err("the worker: the bucket answered 503".into()), "2025-12-11 13:01:00+00").await.unwrap());
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
    assert!(archive::record(&mut client, &token, archive_id, &Ok(()), day_later).await.unwrap());
    assert_eq!(archive_lines(watch::check(&mut client, &settings, day_later).await.unwrap().resolved).len(), 1);
    drop(client);
    scratch.drop().await;
}
