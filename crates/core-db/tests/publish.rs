//! Publishing through core, with bundles built from the real slice of 2025-12-10 (AAPL, six
//! sensors, base and derived revisions, from the droplet's store) and the six rows of the real
//! inventory that cover it. Run with `cargo test -- --ignored`.
//!
//! `a_full_day_from_the_engines_store_publishes_whole` is the exit test of the publish step: it
//! runs when `VYREON_CORE_EXIT_DIR` names a directory written by `vyreon-core engine-store export`.
use test_support::Scratch;
use test_support::slice::{BASE, DERIVED, SLICE, bundle, inventories, parts, temp};
use core_contract::{Header, Item, Scalars, SlotFiles, read_bundle, sha256_hex};
use core_db::publish::{Error, Published, publish_file};
use core_db::{build, migrate};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio_postgres::Client;

/// A bundle written entry by entry, without the writer's checks: for bundles that are wrong.
fn raw_bundle(name: &str, header: &Header, scalars: &Scalars, files: &[SlotFiles]) -> PathBuf {
    let path = temp(name);
    let mut tar = tar::Builder::new(GzEncoder::new(std::fs::File::create(&path).unwrap(), Compression::default()));
    let mut entry = |path: String, bytes: &[u8]| {
        let mut head = tar::Header::new_gnu();
        head.set_size(bytes.len() as u64);
        head.set_mode(0o644);
        head.set_cksum();
        tar.append_data(&mut head, path, bytes).unwrap();
    };
    entry("bundle.json".into(), &serde_json::to_vec(header).unwrap());
    entry("scalars.json".into(), &serde_json::to_vec(scalars).unwrap());
    for (i, f) in files.iter().enumerate() {
        entry(format!("slots/{i:05}/body.json"), &f.body);
        entry(format!("slots/{i:05}/receipt.json"), &f.receipt);
        if let Some((root, events)) = &f.stream {
            entry(format!("slots/{i:05}/events/root.json"), root);
            entry(format!("slots/{i:05}/events/events.jsonl"), events);
        }
    }
    tar.into_inner().unwrap().finish().unwrap();
    path
}

async fn ready(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    for inventory in inventories() {
        assert!(build::register(&mut client, &inventory).await.unwrap().new);
    }
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    (scratch, client)
}

async fn publish(client: &mut Client, path: &Path) -> Result<Published, Error> {
    let result = publish_file(client, path).await;
    std::fs::remove_file(path).unwrap();
    result
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn states(client: &Client) -> Vec<(i64, String, Option<i64>)> {
    client.query("SELECT revision_id, state, base_revision FROM vyreon.day_revision ORDER BY 1", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_publishes_whole_and_a_new_base_supersedes_what_was_built_on_the_old() {
    let (scratch, mut client) = ready("publish").await;
    let (header, scalars, files) = parts(BASE);
    let base = publish(&mut client, &bundle("base", &header, &scalars, &files)).await.unwrap();
    assert_eq!((base.slots, base.streams, base.reader_scalars, base.unusual_scores, base.superseded), (4, 2, 1, 0, 0));
    assert_eq!(base.stream_bytes, 15443 + 11094);

    // the derived revision is bound to the day's published base
    let (dh, ds, df) = parts(DERIVED);
    let derived = publish(&mut client, &bundle("derived", &dh, &ds, &df)).await.unwrap();
    assert_eq!((derived.slots, derived.streams, derived.reader_scalars), (2, 0, 3));
    assert_eq!(states(&client).await, [(base.revision_id, "PUBLISHED".to_owned(), None), (derived.revision_id, "PUBLISHED".to_owned(), Some(base.revision_id))]);

    // what a reader finds is what the droplet's store held: every body text, every scalar, exactly
    let same_bodies = count(
        &client,
        &format!(
            "SELECT count(*) FROM vyreon.published_measurement m JOIN jsonb_to_recordset('{}'::jsonb->'measurement')
                 AS f(measurement_id text, object_id text, body_text text, body_sha256 text, receipt jsonb, science_version text, method_version text, slot_state text)
                 USING (measurement_id, object_id, body_text, body_sha256, receipt, science_version, method_version, slot_state)",
            SLICE.replace('\'', "''")
        ),
    )
    .await;
    assert_eq!(same_bodies, 6);
    let same_scalars = count(
        &client,
        &format!(
            "SELECT count(*) FROM vyreon.accepted_reader_scalar s JOIN jsonb_to_recordset('{}'::jsonb->'reader_scalar')
                 AS f(object_id text, measurement_id text, role_id text, value double precision) USING (object_id, measurement_id, role_id, value)",
            SLICE.replace('\'', "''")
        ),
    )
    .await;
    assert_eq!(same_scalars, 4);
    let same_streams = count(
        &client,
        &format!(
            "SELECT count(*) FROM vyreon.accepted_stream s JOIN jsonb_to_recordset('{}'::jsonb->'measurement_stream')
                 AS f(measurement_id text, root_sha256 text, root_bytes bytea, events_sha256 text, event_count bigint, byte_length bigint)
                 USING (measurement_id, root_sha256, root_bytes, events_sha256, event_count, byte_length)",
            SLICE.replace('\'', "''")
        ),
    )
    .await;
    assert_eq!(same_streams, 2);
    let cutoff = count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE snapshot_cutoff = '2026-11-08T13:49:56.940739+00:00' AND published_at IS NOT NULL").await;
    assert_eq!(cutoff, 2);

    // a derived bundle that names the base it was computed from is refused once that base is replaced
    let mut stale = dh.clone();
    stale.base_revision = Some(base.revision_id);
    let again = publish(&mut client, &bundle("base2", &header, &scalars, &files)).await.unwrap();
    assert_eq!(again.superseded, 2, "the old base and the revision computed from it");
    assert_eq!(
        states(&client).await.iter().map(|(_, s, _)| s.as_str()).collect::<Vec<_>>(),
        ["SUPERSEDED", "SUPERSEDED", "PUBLISHED"]
    );
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.published_measurement").await, 4);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.accepted_measurement").await, 10);
    let refused = publish(&mut client, &bundle("stale", &stale, &ds, &df)).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("published base is now")), "{refused}");
    // and accepted when it names the new one
    stale.base_revision = Some(again.revision_id);
    let rederived = publish(&mut client, &bundle("rederived", &stale, &ds, &df)).await.unwrap();
    assert_eq!(states(&client).await.last().unwrap(), &(rederived.revision_id, "PUBLISHED".to_owned(), Some(again.revision_id)));
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_wrong_bundle_is_refused_and_leaves_nothing() {
    let (scratch, mut client) = ready("refusals").await;
    let (header, scalars, files) = parts(BASE);
    let stream_slot = files.iter().position(|f| f.stream.is_some()).unwrap();
    type Case = (&'static str, Header, Scalars, Vec<SlotFiles>, &'static str);
    let mut cases: Vec<Case> = Vec::new();
    let mut case = |name: &'static str, expect: &'static str, change: &dyn Fn(&mut Header, &mut Scalars, &mut Vec<SlotFiles>)| {
        let (mut h, mut s, mut f) = (header.clone(), scalars.clone(), files.clone());
        change(&mut h, &mut s, &mut f);
        cases.push((name, h, s, f, expect));
    };
    case("unregistered", "is not registered", &|h, _, _| h.engine_build = "0000000".into());
    case("no_inventory", "has no slot inventory", &|h, _, _| h.profile_revision = "VYREON-DLM-PROFILE-2.0.0".into());
    case("missing_slot", "1 missing (VS-Q01/AAPL)", &|h, s, f| {
        let i = h.slots.iter().position(|x| x.measurement_id == "VS-Q01").unwrap();
        h.slots.remove(i);
        f.remove(i);
        s.reader_scalar.clear();
    });
    case("foreign_slot", "1 not in the inventory (VS-Q01/MSFT)", &|h, _, _| {
        h.slots.iter_mut().find(|x| x.measurement_id == "VS-Q01").unwrap().object_id = "MSFT".into();
    });
    case("twice", "is listed twice", &|h, _, f| {
        h.slots.push(h.slots[0].clone());
        f.push(f[0].clone());
    });
    case("body_changed", "body digest mismatch", &|_, _, f| {
        let at = f[0].body.len() / 2;
        f[0].body[at] ^= 1;
    });
    case("receipt_changed", "receipt digest mismatch", &|_, _, f| f[0].receipt.push(b' '));
    case("no_newline", "does not end with a newline", &|h, _, f| {
        f[0].body.pop();
        h.slots[0].body_sha256 = sha256_hex(&f[0].body);
    });
    case("stream_dropped", "commits to an event stream the bundle does not hold", &|_, _, f| f[stream_slot].stream = None);
    case("stream_changed", "stream events do not match the stream root", &|_, _, f| {
        let events = &mut f[stream_slot].stream.as_mut().unwrap().1;
        let at = events.len() / 2;
        events[at] ^= 1;
    });
    case("stream_swapped", "stream root does not match the body's commitment", &|_, _, f| {
        let other = f.iter().rposition(|x| x.stream.is_some()).unwrap();
        assert_ne!(other, stream_slot);
        f[stream_slot].stream = f[other].stream.clone();
    });
    case("stray_scalar", "which is not a slot of the day", &|_, s, _| s.reader_scalar[0].object_id = "MSFT".into());
    case("scalar_twice", "duplicate key", &|_, s, _| s.reader_scalar.push(s.reader_scalar[0].clone()));
    case("base_without_profile", "base_revision without base_profile_revision", &|h, _, _| h.base_revision = Some(1));
    case("bad_date", "date", &|h, _, _| h.represented_date = "2025-13-45".into());
    for (name, h, s, f, expect) in cases {
        let refused = publish(&mut client, &raw_bundle(name, &h, &s, &f)).await.unwrap_err();
        assert!(matches!(&refused, Error::Refused(m) if m.contains(expect)), "{name}: {refused}");
    }

    // a bundle cut short, and one that is not a bundle
    let whole = std::fs::read(bundle("whole", &header, &scalars, &files)).unwrap();
    for (name, bytes) in [("cut", &whole[..whole.len() * 2 / 3]), ("noise", &b"not a bundle"[..])] {
        let path = temp(name);
        std::fs::write(&path, bytes).unwrap();
        let refused = publish(&mut client, &path).await.unwrap_err();
        assert!(matches!(&refused, Error::Refused(m) if m.contains("unreadable bundle")), "{name}: {refused}");
    }
    std::fs::remove_file(temp("whole")).unwrap();
    // a revision computed from a base needs the day's base published
    let (dh, ds, df) = parts(DERIVED);
    let refused = publish(&mut client, &bundle("orphan", &dh, &ds, &df)).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("no published")), "{refused}");
    // a month without partitions is core's failure, not the bundle's
    let mut january = header.clone();
    january.represented_date = "2026-01-05".into();
    let failed = publish(&mut client, &bundle("january", &january, &scalars, &files)).await.unwrap_err();
    assert!(matches!(&failed, Error::Failed(m) if m.contains("no partitions")), "{failed}");

    for table in ["day_revision", "measurement", "measurement_stream", "reader_scalar"] {
        assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.{table}")).await, 0, "{table}");
    }
    // and the same client still publishes a right one
    publish(&mut client, &bundle("right", &header, &scalars, &files)).await.unwrap();
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_build_keeps_the_inventory_it_was_registered_with() {
    let (scratch, mut client) = ready("register").await;
    let inventory = inventories().remove(0);
    let again = build::register(&mut client, &inventory).await.unwrap();
    assert!(!again.new);
    assert_eq!(again.slots, 4);
    let mut fewer = inventory.clone();
    fewer["slots"].as_array_mut().unwrap().pop();
    let mut moved = inventory.clone();
    moved["slots"][0]["object_id"] = json!("MSFT");
    for changed in [fewer, moved] {
        let refused = build::register(&mut client, &changed).await.unwrap_err();
        assert!(refused.contains("different inventory"), "{refused}");
    }
    assert_eq!(count(&client, "SELECT count(*) FROM core.expected_slot").await, 6);
    scratch.drop().await;
}

/// The exit test of the publish step: a whole day the engine published on the droplet goes
/// through core into an empty database, and what a reader finds there is, slot for slot, what
/// the bundles carried.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_full_day_from_the_engines_store_publishes_whole() {
    let Some(dir) = std::env::var_os("VYREON_CORE_EXIT_DIR").map(PathBuf::from) else {
        eprintln!("skip: VYREON_CORE_EXIT_DIR is not set (a directory written by `vyreon-core engine-store export`)");
        return;
    };
    let scratch = Scratch::new("exit").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    let mut exports: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(".export.json")).collect();
    exports.sort();
    assert!(!exports.is_empty(), "no *.export.json in {}", dir.display());
    for export in exports {
        let export: Value = serde_json::from_slice(&std::fs::read(&export).unwrap()).unwrap();
        let date = export["represented_date"].as_str().unwrap();
        client.query_one("SELECT vyreon.ensure_months($1::text::date, $1::text::date)", &[&date]).await.unwrap();
        for file in export["inventories"].as_array().unwrap() {
            let inventory: Value = serde_json::from_slice(&std::fs::read(dir.join(file.as_str().unwrap())).unwrap()).unwrap();
            build::register(&mut client, &inventory).await.unwrap();
        }
        for file in export["bundles"].as_array().unwrap() {
            let path = dir.join(file.as_str().unwrap());
            let started = std::time::Instant::now();
            let published = publish_file(&mut client, &path).await.unwrap();
            eprintln!("{published:?} in {:.1} s", started.elapsed().as_secs_f64());
            // the bundle's own account of itself, read independently of the publish
            let (mut header, mut scalars) = (None, None);
            read_bundle(std::io::BufReader::new(std::fs::File::open(&path).unwrap()), &mut |item| {
                match item {
                    Item::Header(h) => header = Some(*h),
                    Item::Scalars(s) => scalars = Some(s),
                    Item::Slot(_) => {}
                }
                Ok(())
            })
            .unwrap();
            let (header, scalars) = (header.unwrap(), scalars.unwrap());
            assert_eq!(published.slots as usize, header.slots.len());
            assert_eq!(published.streams as usize, header.slots.iter().filter(|s| s.stream_root_sha256.is_some()).count());
            assert_eq!((published.reader_scalars as usize, published.unusual_scores as usize), (scalars.reader_scalar.len(), scalars.unusual_score.len()));
            let listed: Vec<Value> = header.slots.iter().map(|s| json!({"measurement_id": s.measurement_id, "object_id": s.object_id, "body_sha256": s.body_sha256})).collect();
            let found: i64 = client
                .query_one(
                    "SELECT count(*) FROM vyreon.published_measurement m
                         JOIN jsonb_to_recordset($1::text::jsonb) AS b(measurement_id text, object_id text, body_sha256 text)
                         USING (measurement_id, object_id, body_sha256)
                     WHERE m.represented_date = $2::text::date AND m.profile_revision = $3
                       AND encode(sha256(convert_to(m.body_text || E'\\n', 'UTF8')), 'hex') = m.body_sha256",
                    &[&Value::Array(listed).to_string(), &date, &header.profile_revision],
                )
                .await
                .unwrap()
                .get(0);
            assert_eq!(found as usize, header.slots.len(), "{date} {}", header.profile_revision);
        }
    }
    scratch.drop().await;
}
