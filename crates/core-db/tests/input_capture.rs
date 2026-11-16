//! Input captures (C-043) with real ones: the engine's packing of the real macro capture of
//! 2025-12-03 (`fixtures/input-capture-macro-2025-12-03.tar.gz`) and, as a second capture of the
//! same session and kind, AAPL's real reduced tape of 2025-12-10 packed the same way (core treats
//! an archive as opaque bytes; a second real manifest stands for a later fetch). Kept under a day
//! block's lease once its day (the real slice of 2025-12-10) is published. Run with
//! `cargo test -- --ignored`.
use core_db::capture::{self, Capture};
use core_db::publish::publish_for_lease;
use core_db::work::{self, Kind, LeaseError};
use core_db::{build, migrate, universe};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use std::io::Read;
use test_support::slice::{BASE, bundle, inventories, parts};
use test_support::{MACRO_CAPTURE, MACRO_CAPTURE_IDENTITY, Scratch, TAPE_AAPL};

const PROFILE: &str = "FREE_REPORT_64";
const HOUR: f64 = 3600.0;

/// The entries of a gzip tar, in order.
fn entries(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    let mut out = Vec::new();
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.push((name, bytes));
    }
    out
}

/// Packs files as the engine does: the manifest first, then by name; mode 0644, mtime 0.
fn pack(mut files: Vec<(String, Vec<u8>)>, level: Compression) -> Vec<u8> {
    files.sort_by(|a, b| ((a.0 != "manifest.json"), a.0.as_bytes()).cmp(&((b.0 != "manifest.json"), b.0.as_bytes())));
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), level));
    for (name, bytes) in &files {
        let mut head = tar::Header::new_gnu();
        head.set_size(bytes.len() as u64);
        head.set_mode(0o644);
        head.set_mtime(0);
        head.set_cksum();
        tar.append_data(&mut head, name, bytes.as_slice()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn archive(bytes: &[u8], identity: &str, captured_at: &str) -> Capture {
    Capture::Archive { checked: capture::check(bytes.to_vec(), identity).unwrap(), captured_at: captured_at.into() }
}

#[test]
fn an_archive_is_checked_against_its_identity() {
    let checked = capture::check(MACRO_CAPTURE.to_vec(), MACRO_CAPTURE_IDENTITY).unwrap();
    assert_eq!((checked.files, checked.unpacked_bytes), (39, 145_663));
    assert_eq!(checked.archive_sha256(), "96709c90cdf997c5906db1a3251aa9417bc2913df3cd1e0c377d6970d8d4fca0");
    // another identity, a damaged archive, an archive cut short, a tar that does not start with
    // its manifest (the tape cache as it is packed for core's tape route: a directory entry and
    // the manifest last)
    let other = capture::check(MACRO_CAPTURE.to_vec(), &"0".repeat(64)).unwrap_err();
    assert!(other.contains("not to its identity"), "{other}");
    assert!(capture::check(MACRO_CAPTURE.to_vec(), "efc96bf3").is_err());
    let mut damaged = MACRO_CAPTURE.to_vec();
    let n = damaged.len();
    damaged[n - 6] ^= 0xff;
    assert!(capture::check(damaged, MACRO_CAPTURE_IDENTITY).is_err(), "the gzip trailer's CRC is checked");
    assert!(capture::check(MACRO_CAPTURE[..MACRO_CAPTURE.len() / 2].to_vec(), MACRO_CAPTURE_IDENTITY).is_err());
    let tape_manifest = entries(TAPE_AAPL).into_iter().find(|(n, _)| n == "manifest.json").unwrap().1;
    let unordered = capture::check(TAPE_AAPL.to_vec(), &core_contract::sha256_hex(&tape_manifest)).unwrap_err();
    assert!(unordered.contains("files only") || unordered.contains("first entry"), "{unordered}");
    assert!(capture::check(Vec::new(), MACRO_CAPTURE_IDENTITY).is_err());
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_published_days_captures_are_kept_once_and_a_new_fetch_is_another_version() {
    let scratch = Scratch::new("captures").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let fixtures = test_support::fixtures();
    let doc = universe::read_responses(&fixtures.join("engine-v1-PROFILE.json"), &fixtures.join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for inventory in inventories() {
        let b = build::register(&mut client, &inventory).await.unwrap().engine_build;
        build::admit(&client, &b, &json!({"checklist": "capture test"})).await.unwrap();
        builds.push(b);
    }
    let (base_build, derived_build) = (builds[0].clone(), builds[1].clone());
    work::schedule(&mut client, Kind::DayBlock, PROFILE, &base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    work::schedule(&mut client, Kind::OrderedPass, PROFILE, &derived_build, "2025-12-10", "2025-12-10", 1).await.unwrap();
    let block = work::claim(&mut client, Kind::DayBlock, &base_build, "w1", HOUR).await.unwrap().unwrap();
    let macro_capture = || archive(MACRO_CAPTURE, MACRO_CAPTURE_IDENTITY, "2026-11-16T04:49:17.420677Z");

    // nothing is kept for a day not published (a refused day keeps nothing)
    let early = capture::put(&mut client, &block.token, "2025-12-03", "macro", &macro_capture()).await;
    assert!(matches!(early, Err(LeaseError::Refused(ref m)) if m.contains("has published no day")), "{early:?}");
    let (header, scalars, files) = parts(BASE);
    let path = bundle("captures-base", &header, &scalars, &files);
    publish_for_lease(&mut client, &path, &block.token).await.unwrap();
    std::fs::remove_file(&path).unwrap();

    // the day's captures are kept, T−5's among them; the same capture again changes nothing
    assert!(capture::put(&mut client, &block.token, "2025-12-03", "macro", &macro_capture()).await.unwrap());
    assert!(!capture::put(&mut client, &block.token, "2025-12-03", "macro", &macro_capture()).await.unwrap());
    // the same identity packed into other bytes is refused, and the first is kept
    let repacked = pack(entries(MACRO_CAPTURE), Compression::best());
    assert_ne!(repacked, MACRO_CAPTURE);
    let other_bytes = capture::put(&mut client, &block.token, "2025-12-03", "macro", &archive(&repacked, MACRO_CAPTURE_IDENTITY, "2026-11-16T05:00:00Z")).await;
    assert!(matches!(other_bytes, Err(LeaseError::Refused(ref m)) if m.contains("already kept")), "{other_bytes:?}");
    // a capture with another manifest (a later fetch) is another version, kept beside the first
    let second = pack(entries(TAPE_AAPL), Compression::default());
    let second_identity = core_contract::sha256_hex(&entries(&second)[0].1);
    assert!(capture::put(&mut client, &block.token, "2025-12-03", "macro", &archive(&second, &second_identity, "2026-11-17T21:00:00Z")).await.unwrap());
    // a capture not kept is recorded with its reason, each time it is reported
    for _ in 0..2 {
        assert!(capture::put(&mut client, &block.token, "2025-12-10", "company", &Capture::Absent("not packed: no manifest.json".into())).await.unwrap());
    }

    // what is refused: a session after the published day, a session the calendar does not hold,
    // another kind, a time PostgreSQL would read as a word, an empty reason, a gone lease
    let refused = |r: Result<bool, LeaseError>, says: &str| assert!(matches!(r, Err(LeaseError::Refused(ref m)) if m.contains(says)), "{r:?} should say {says}");
    refused(capture::put(&mut client, &block.token, "2025-12-11", "macro", &macro_capture()).await, "is after 2025-12-10");
    refused(capture::put(&mut client, &block.token, "2025-12-06", "macro", &macro_capture()).await, "not a session");
    refused(capture::put(&mut client, &block.token, "2025-12-03", "options", &macro_capture()).await, "not a kind");
    refused(capture::put(&mut client, &block.token, "2025-12-03", "macro", &archive(MACRO_CAPTURE, MACRO_CAPTURE_IDENTITY, "now")).await, "not an instant");
    refused(capture::put(&mut client, &block.token, "2025-12-03", "carry", &Capture::Absent(" ".into())).await, "needs its reason");
    assert_eq!(capture::put(&mut client, "not-a-lease", "2025-12-03", "macro", &macro_capture()).await, Err(LeaseError::Gone));

    // said done, the block's late resend of the same capture is still answered
    work::day_done(&mut client, &block.token, "2025-12-10").await.unwrap();
    assert!(!capture::put(&mut client, &block.token, "2025-12-03", "macro", &macro_capture()).await.unwrap());
    work::finish(&mut client, &block.token).await.unwrap();
    assert_eq!(capture::put(&mut client, &block.token, "2025-12-03", "macro", &macro_capture()).await, Err(LeaseError::Gone));
    // only a day block keeps captures
    let pass = work::claim(&mut client, Kind::OrderedPass, &derived_build, "p1", HOUR).await.unwrap().unwrap();
    refused(capture::put(&mut client, &pass.token, "2025-12-03", "macro", &macro_capture()).await, "not a day block");

    // listed by kind, oldest first; each archive as it was sent
    let listed = capture::list(&client, "2025-12-03").await.unwrap();
    let versions: Vec<(&str, &str, i64, &str)> = listed
        .iter()
        .map(|v| (v["kind"].as_str().unwrap(), v["identity"].as_str().unwrap(), v["archive_bytes"].as_i64().unwrap(), v["captured_at"].as_str().unwrap()))
        .collect();
    assert_eq!(
        versions,
        [
            ("macro", MACRO_CAPTURE_IDENTITY, MACRO_CAPTURE.len() as i64, "2026-11-16T04:49:17.420677Z"),
            ("macro", second_identity.as_str(), second.len() as i64, "2026-11-17T21:00:00.000000Z"),
        ]
    );
    assert_eq!((listed[0]["job_id"].as_i64(), listed[0]["engine_build"].as_str(), listed[0]["worker"].as_str()), (Some(block.job_id), Some(base_build.as_str()), Some("w1")));
    let absences: Vec<Value> = capture::list(&client, "2025-12-10").await.unwrap();
    assert_eq!(absences.len(), 2);
    assert_eq!(absences[0]["absence"], "not packed: no manifest.json");
    assert_eq!(capture::archive(&client, "2025-12-03", "macro", MACRO_CAPTURE_IDENTITY).await.unwrap().as_deref(), Some(MACRO_CAPTURE));
    assert_eq!(capture::archive(&client, "2025-12-03", "macro", &second_identity).await.unwrap(), Some(second));
    assert_eq!(capture::archive(&client, "2025-12-03", "carry", MACRO_CAPTURE_IDENTITY).await.unwrap(), None);
    assert!(capture::list(&client, "yesterday").await.is_err());
    // the stored archive is checked against its digest when it is read
    client.execute("UPDATE core.input_capture SET archive = archive || '\\x00'::bytea WHERE identity = $1", &[&MACRO_CAPTURE_IDENTITY]).await.unwrap();
    assert!(capture::archive(&client, "2025-12-03", "macro", MACRO_CAPTURE_IDENTITY).await.is_err());
    drop(client);
    scratch.drop().await;
}
