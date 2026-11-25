//! The real slice of 2025-12-10 (AAPL, six sensors, base and derived revisions, from the droplet's
//! store) as publishable bundles, with the six rows of the real inventory that cover it. Each
//! bundle states the 64 members as listed that day, so publishing one needs the universe loaded.
use core_contract::{BundleWriter, CONTRACT, Header, Scalar, Scalars, Slot, SlotFiles, sha256_hex};
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::io::Read;
use std::path::PathBuf;

pub const SLICE: &str = include_str!("../fixtures/droplet-2025-12-10-aapl.json");
pub const BASE: i64 = 1;
pub const DERIVED: i64 = 2;

pub fn slice() -> Value {
    serde_json::from_str(SLICE).unwrap()
}

/// The engine's profile at engine-v1 (`fixtures/engine-v1-PROFILE.json`).
pub const PROFILE_FILE: &str = include_str!("../fixtures/engine-v1-PROFILE.json");

/// The profile's 64 members in order. All 64 were listed on 2025-12-10 (the droplet published
/// every one of them that day; the latest first listing among them is ETHA's, 2024-07-23).
pub fn listed_2025_12_10() -> Vec<String> {
    let profile: Value = serde_json::from_str(PROFILE_FILE).unwrap();
    profile["ordered_security_ids"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_owned()).collect()
}

/// A bytea as PostgreSQL prints it in JSON (`\x` and hex).
fn bytea(v: &Value) -> Vec<u8> {
    let hex = v.as_str().unwrap().strip_prefix("\\x").unwrap();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

/// One of the slice's revisions as a bundle's parts.
pub fn parts(revision: i64) -> (Header, Scalars, Vec<SlotFiles>) {
    let s = slice();
    let of = |table: &str| -> Vec<Value> { s[table].as_array().unwrap().iter().filter(|r| r["revision_id"] == revision).cloned().collect() };
    let text = |v: &Value| v.as_str().unwrap().to_owned();
    let head = &of("day_revision")[0];
    let (mut slots, mut files) = (Vec::new(), Vec::new());
    for m in of("measurement") {
        let stream = of("measurement_stream").into_iter().find(|x| x["measurement_id"] == m["measurement_id"]).map(|x| {
            let mut events = Vec::new();
            GzDecoder::new(bytea(&x["events_gzip"]).as_slice()).read_to_end(&mut events).unwrap();
            (bytea(&x["root_bytes"]), events)
        });
        let body = format!("{}\n", text(&m["body_text"])).into_bytes();
        let receipt = m["receipt"].to_string().into_bytes();
        slots.push(Slot {
            measurement_id: text(&m["measurement_id"]),
            object_id: text(&m["object_id"]),
            slot_state: text(&m["slot_state"]),
            science_version: text(&m["science_version"]),
            method_version: text(&m["method_version"]),
            body_sha256: sha256_hex(&body),
            receipt_sha256: sha256_hex(&receipt),
            stream_root_sha256: stream.as_ref().map(|(root, _)| sha256_hex(root)),
            coordinates_sha256: None,
        });
        files.push(SlotFiles { body, receipt, coordinates: None, stream });
    }
    let header = Header {
        contract: CONTRACT.to_owned(),
        represented_date: text(&head["represented_date"]),
        profile_id: text(&head["profile_id"]),
        profile_revision: text(&head["profile_revision"]),
        engine_build: text(&head["engine_build"]),
        snapshot_cutoff: text(&head["snapshot_cutoff"]),
        base_profile_revision: (revision == DERIVED).then(|| text(&s["day_revision"][0]["profile_revision"])),
        base_revision: None,
        report: None,
        day_local: false,
        listed_members: Some(listed_2025_12_10()),
        republish: None,
        slots,
    };
    let reader_scalar = of("reader_scalar")
        .iter()
        .map(|r| Scalar { object_id: text(&r["object_id"]), measurement_id: text(&r["measurement_id"]), role_id: text(&r["role_id"]), value: r["value"].as_f64().unwrap() })
        .collect();
    (header, Scalars { reader_scalar, unusual_score: Vec::new() }, files)
}

pub fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("core-test-{}-{name}.tar.gz", std::process::id()))
}

/// A bundle written by the contract's writer.
pub fn bundle(name: &str, header: &Header, scalars: &Scalars, files: &[SlotFiles]) -> PathBuf {
    let path = temp(name);
    let mut writer = BundleWriter::new(std::fs::File::create(&path).unwrap(), header, scalars).unwrap();
    for f in files {
        writer.add(f).unwrap();
    }
    writer.finish().unwrap();
    path
}

/// The profile's members in order, less `keys`.
pub fn members_except(keys: &[&str]) -> Vec<String> {
    listed_2025_12_10().into_iter().filter(|k| !keys.contains(&k.as_str())).collect()
}

/// A session on which ETHA was not listed (its first session is 2024-07-23) and IBIT was (from
/// 2024-01-11).
pub const ETHA_UNLISTED: &str = "2024-07-01";

/// The base build's inventory rows of the slice's four base sensors for ETHA as well as AAPL: the
/// real rows of the engine's inventory, which lays each base sensor out over the 64 in profile
/// order, so a sensor's ETHA row (member 33) sits 15 ordinals before its AAPL row (member 48).
pub fn base_inventory_with_etha() -> Value {
    let mut inventory = inventories().remove(0);
    let slots = inventory["slots"].as_array_mut().unwrap();
    let etha: Vec<Value> = slots
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row["object_id"] = json!("ETHA");
            row["slot_ordinal"] = json!(row["slot_ordinal"].as_i64().unwrap() - 15);
            row
        })
        .collect();
    slots.extend(etha);
    inventory
}

/// Each of the slice's builds with the real inventory rows of the profile it published.
pub fn inventories() -> Vec<Value> {
    let s = slice();
    s["day_revision"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            let slots: Vec<&Value> = s["expected_slot"].as_array().unwrap().iter().filter(|e| e["profile_revision"] == d["profile_revision"]).collect();
            json!({"engine_build": d["engine_build"], "slots": slots})
        })
        .collect()
}

/// The day-local derived sensor of the tests' day-local profile: DLM-MAX-PAIN reads its own day
/// only (C-031 splits the derived sensors into a day-local profile and a history profile).
pub const DAY_LOCAL_SENSOR: &str = "DLM-MAX-PAIN";

/// The profile revision of the tests' day-local derived profile: the slice's derived profile
/// revision with `-DAY-LOCAL`. The slice's real derived revision is one profile (engine-v1, before
/// C-031's split); the day-local profile here is its DLM-MAX-PAIN slot under this name.
pub fn day_local_profile() -> String {
    format!("{}-DAY-LOCAL", text_of(&slice()["day_revision"][1]["profile_revision"]))
}

fn text_of(v: &Value) -> String {
    v.as_str().unwrap().to_owned()
}

/// `inventory` (one of `inventories()`, the build that publishes the day's base) with the real
/// inventory row of DLM-MAX-PAIN added under the day-local profile revision: a build that publishes
/// each day's base and its day-local revision, as a day block does (C-031). Only the profile name
/// is made here.
pub fn with_day_local(mut inventory: Value) -> Value {
    let s = slice();
    let profile = day_local_profile();
    let rows: Vec<Value> = s["expected_slot"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["profile_revision"] == s["day_revision"][1]["profile_revision"] && e["measurement_id"] == DAY_LOCAL_SENSOR)
        .map(|e| {
            let mut e = e.clone();
            e["profile_revision"] = json!(profile);
            e
        })
        .collect();
    inventory["slots"].as_array_mut().unwrap().extend(rows);
    inventory
}

/// `inventory` (the build that runs the ordered pass: the slice's derived build) with the profiles its
/// pass reads registered as well: the base build's real rows and the day-local rows (`with_day_local`).
/// The engine's pass reads exactly its own build's base and day-local profile revisions, and the
/// cluster's image is one build for every profile; core counts a day as published for a pass only in
/// profile revisions of the pass's build (audit 2026-11-22 round 2, core-r2 2). Only the rows' build
/// is made here.
pub fn reading(mut inventory: Value) -> Value {
    let base = inventories().remove(0);
    inventory["slots"].as_array_mut().unwrap().extend(base["slots"].as_array().unwrap().iter().cloned());
    with_day_local(inventory)
}

/// The day-local revision of `day` computed on base revision `base`, sent as build `build`: the
/// slice's DLM-MAX-PAIN slot, its body, receipt and reader scalars (core's checks never read a
/// body's date or build), under the day-local profile revision.
pub fn day_local_parts(build: &str, day: &str, base: i64) -> (Header, Scalars, Vec<SlotFiles>) {
    let (mut h, mut s, mut f) = parts(DERIVED);
    let keep: Vec<bool> = h.slots.iter().map(|slot| slot.measurement_id == DAY_LOCAL_SENSOR).collect();
    let mut i = 0;
    h.slots.retain(|_| (keep[i], i += 1).0);
    let mut i = 0;
    f.retain(|_| (keep[i], i += 1).0);
    s.reader_scalar.retain(|x| x.measurement_id == DAY_LOCAL_SENSOR);
    (h.engine_build, h.profile_revision, h.represented_date, h.base_revision, h.day_local) = (build.to_owned(), day_local_profile(), day.to_owned(), Some(base), true);
    (h, s, f)
}
