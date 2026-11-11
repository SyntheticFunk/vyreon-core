//! The real slice of 2025-12-10 (AAPL, six sensors, base and derived revisions, from the droplet's
//! store) as publishable bundles, with the six rows of the real inventory that cover it.
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
