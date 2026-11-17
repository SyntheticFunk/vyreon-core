//! Publishing through core, with bundles built from the real slice of 2025-12-10 (AAPL, six
//! sensors, base and derived revisions, from the droplet's store) and the six rows of the real
//! inventory that cover it. Run with `cargo test -- --ignored`.
//!
//! `a_full_day_from_the_engines_store_publishes_whole` is the exit test of the publish step: it
//! runs when `VYREON_CORE_EXIT_DIR` names a directory written by `vyreon-core engine-store export`.
use test_support::{Scratch, fixtures};
use test_support::slice::{BASE, DERIVED, ETHA_UNLISTED, SLICE, base_inventory_with_etha, bundle, inventories, members_except, parts, temp};
use core_contract::{FIELD_OBJECT, Header, Item, Scalars, SlotFiles, read_bundle, sha256_hex};
use core_db::publish::{EXPECTED_COUNT, Error, Published, publish_file, publish_file_vouched};
use core_db::{build, migrate, universe};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio_postgres::Client;

/// A bundle written entry by entry, without the writer's checks: for bundles that are wrong.
fn raw_bundle(name: &str, header: &Header, scalars: &Scalars, files: &[SlotFiles]) -> PathBuf {
    raw_bundle_and(name, header, scalars, files, &[])
}

/// The same, with entries after the last slot's.
fn raw_bundle_and(name: &str, header: &Header, scalars: &Scalars, files: &[SlotFiles], after: &[(&str, &[u8])]) -> PathBuf {
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
    for (name, bytes) in after {
        entry(name.to_string(), bytes);
    }
    tar.into_inner().unwrap().finish().unwrap();
    path
}

/// The engine's trading calendar: a day is published only on one of its sessions.
async fn calendar(client: &mut Client) {
    core_db::work::load_calendar(client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
}

/// The universe of the 64 from the frozen Intrinio responses.
fn universe_document() -> Value {
    universe::read_responses(&fixtures().join("engine-v1-PROFILE.json"), &fixtures().join("intrinio-universe")).unwrap()
}

async fn ready(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    calendar(&mut client).await;
    universe::store(&mut client, &universe_document()).await.unwrap();
    for inventory in inventories() {
        assert!(build::register(&mut client, &inventory).await.unwrap().new);
    }
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    (scratch, client)
}

/// Published by hand and vouched for (C-042): these tests send derived bundles that name no base
/// revision and read history, straight after the base they were computed from.
async fn publish(client: &mut Client, path: &Path) -> Result<Published, Error> {
    let result = publish_file_vouched(client, path).await;
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
    case("no_listed_set", "does not state the members listed on 2025-12-10 (listed_members)", &|h, _, _| h.listed_members = None);
    case("listed_twice", "listed_members names AAPL twice", &|h, _, _| h.listed_members.as_mut().unwrap().push("AAPL".into()));
    case("bad_date", "date", &|h, _, _| h.represented_date = "2025-13-45".into());
    case("a_word_for_a_date", "is not a date written YYYY-MM-DD", &|h, _, _| h.represented_date = "yesterday".into());
    // a Saturday (audit 2026-11-14, core.md 12: it used to publish where no reader looks)
    case("not_a_session", "2025-12-13 is not a trading session of the calendar", &|h, _, _| h.represented_date = "2025-12-13".into());
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
    // every slot there, then an entry the contract does not have: the reader fails after the last
    // slot, and nothing is written (audit 2026-11-14, core.md 3: it used to commit, then refuse)
    let stray = raw_bundle_and("stray", &header, &scalars, &files, &[("stray.json", b"{}")]);
    let refused = publish(&mut client, &stray).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("unexpected entry stray.json after the last slot")), "{refused}");
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
    calendar(&mut client).await;
    universe::store(&mut client, &universe_document()).await.unwrap();
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
            // the export names no base revision (the store's ids are not core's): vouched for
            let published = publish_file_vouched(&mut client, &path).await.unwrap();
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

/// C-031: a based revision may say it reads its own day only. Core keeps that with the revision;
/// a republished base of the same day still supersedes it (it was computed from the old base),
/// and the flag means nothing without a base. That a later day's day-local revision survives an
/// earlier day's republish needs two real days; the multi-day block proof covers it.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_local_revision_is_kept_as_one_and_falls_with_its_own_days_base() {
    let (scratch, mut client) = ready("daylocal").await;
    let (header, scalars, files) = parts(BASE);
    let mut alone = header.clone();
    alone.day_local = true;
    let refused = publish(&mut client, &bundle("daylocal-alone", &alone, &scalars, &files)).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("day_local without base_profile_revision")), "{refused}");
    publish(&mut client, &bundle("daylocal-base", &header, &scalars, &files)).await.unwrap();
    let (mut dh, ds, df) = parts(DERIVED);
    dh.day_local = true;
    let derived = publish(&mut client, &bundle("daylocal-derived", &dh, &ds, &df)).await.unwrap();
    assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.day_revision WHERE day_local AND revision_id = {}", derived.revision_id)).await, 1);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision WHERE day_local").await, 1);
    let again = publish(&mut client, &bundle("daylocal-base2", &header, &scalars, &files)).await.unwrap();
    assert_eq!(again.superseded, 2, "the old base and its day's day-local revision");
    scratch.drop().await;
}

/// Audit 2026-11-15 (core.md) 1, C-042: by hand there is no lease to say what a bundle read. A
/// revision that reads history is refused (a day it read may have been replaced since), and one
/// that reads its day only must name the base revision it was computed from; a base needs
/// neither. The same bundles are taken when the hand vouches for them.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn by_hand_a_based_revision_names_its_base_and_one_that_reads_history_is_refused() {
    let (scratch, mut client) = ready("byhand").await;
    let strict = async |client: &mut Client, name: &str, (h, s, f): (&Header, &Scalars, &[SlotFiles])| {
        let path = bundle(name, h, s, f);
        let result = publish_file(client, &path).await;
        std::fs::remove_file(&path).unwrap();
        result
    };
    let (header, scalars, files) = parts(BASE);
    let base = strict(&mut client, "byhand-base", (&header, &scalars, &files)).await.unwrap();

    // reads history: refused, whether or not it names its base
    let (dh, ds, df) = parts(DERIVED);
    assert!(!dh.day_local && dh.base_revision.is_none());
    let mut named = dh.clone();
    named.base_revision = Some(base.revision_id);
    for (name, h) in [("byhand-history", &dh), ("byhand-history-named", &named)] {
        let refused = strict(&mut client, name, (h, &ds, &df)).await.unwrap_err();
        assert!(matches!(&refused, Error::Refused(m) if m.contains("reads history is published by the ordered pass")), "{refused}");
    }
    // reads its day only: refused until it names the base revision it read
    let mut local = dh.clone();
    local.day_local = true;
    let refused = strict(&mut client, "byhand-local", (&local, &ds, &df)).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("names the base revision it read")), "{refused}");
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, 1, "a refusal leaves nothing");
    local.base_revision = Some(base.revision_id);
    let derived = strict(&mut client, "byhand-local-named", (&local, &ds, &df)).await.unwrap();
    assert_eq!(states(&client).await.last().unwrap(), &(derived.revision_id, "PUBLISHED".to_owned(), Some(base.revision_id)));

    // vouched for, the bundle that reads history and names no base is taken
    let vouched = publish(&mut client, &bundle("byhand-vouched", &dh, &ds, &df)).await.unwrap();
    assert_eq!(states(&client).await.last().unwrap(), &(vouched.revision_id, "PUBLISHED".to_owned(), Some(base.revision_id)));
    scratch.drop().await;
}

/// A stored revision's recorded listed members, and the slots they make it expected to hold.
async fn recorded(client: &Client, revision: i64) -> (Vec<String>, i64) {
    let listed = client.query_one("SELECT listed_members FROM vyreon.day_revision WHERE revision_id = $1", &[&revision]).await.unwrap().get(0);
    (listed, client.query_one(EXPECTED_COUNT, &[&revision, &FIELD_OBJECT]).await.unwrap().get(0))
}

/// Migration 0013 on a database that already holds a published day: before it, every object of a
/// build's inventory was expected on every day, so the day records exactly those (profile order)
/// and is expected to hold what it was before.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_day_published_before_the_listed_set_keeps_its_whole_inventory() {
    let scratch = Scratch::new("listed_migration").await;
    let mut client = scratch.client().await;
    let all = migrate::embedded().unwrap();
    let before: Vec<_> = all.iter().filter(|m| m.version < 13).cloned().collect();
    migrate::apply(&mut client, &before).await.unwrap();
    universe::store(&mut client, &universe_document()).await.unwrap();
    build::register(&mut client, &base_inventory_with_etha()).await.unwrap();
    let revision: i64 = client
        .query_one(
            "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state, published_at)
             SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, 'PUBLISHED', now()
             FROM jsonb_to_recordset($1::text::jsonb->'day_revision')
                 AS r(represented_date date, profile_id text, profile_revision text, engine_build text, snapshot_cutoff timestamptz, base_revision bigint)
             WHERE base_revision IS NULL RETURNING revision_id",
            &[&SLICE],
        )
        .await
        .unwrap()
        .get(0);
    let through_13: Vec<_> = all.iter().filter(|m| m.version <= 13).cloned().collect();
    assert_eq!(migrate::apply(&mut client, &through_13).await.unwrap().applied, [(13, "listed_members".to_owned())]);
    assert_eq!(recorded(&client, revision).await, (vec!["ETHA".to_owned(), "AAPL".to_owned()], 8));
    let nullable: String = client
        .query_one("SELECT is_nullable FROM information_schema.columns WHERE table_schema = 'vyreon' AND table_name = 'day_revision' AND column_name = 'listed_members'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(nullable, "NO");
    scratch.drop().await;
}

/// The members core's universe lists on a day, in order.
async fn listed_on(client: &Client, day: &str) -> Vec<String> {
    client
        .query("SELECT asset_key FROM core.members_on('FREE_REPORT_64', $1::text::date) WHERE ticker IS NOT NULL ORDER BY ordinal", &[&day])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

/// C-036: a member has slots only on the sessions it is listed. The base build's inventory here
/// holds the slice's four sensors for ETHA as well as AAPL (the real inventory's rows). ETHA's
/// first session is 2024-07-23, so on 2024-07-01 it was not listed and IBIT was. Core's checks
/// never read a body's date, so the slice's real AAPL bodies of 2025-12-10 are sent under
/// 2024-07-01 (as `january` is above); nothing in them is changed.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_member_not_listed_on_the_day_has_no_slots_and_the_day_keeps_who_was_listed() {
    let scratch = Scratch::new("listed").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    calendar(&mut client).await;
    universe::store(&mut client, &universe_document()).await.unwrap();
    build::register(&mut client, &base_inventory_with_etha()).await.unwrap();
    client.query_one("SELECT vyreon.ensure_months('2024-07-01', '2024-07-31')", &[]).await.unwrap();
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap();
    assert_eq!(listed_on(&client, ETHA_UNLISTED).await, members_except(&["ETHA"]));
    assert_eq!(listed_on(&client, "2025-12-10").await, members_except(&[]));

    let (mut header, scalars, files) = parts(BASE);
    header.represented_date = ETHA_UNLISTED.into();
    header.listed_members = Some(members_except(&["ETHA"]));

    // the worker and core must agree on who is listed
    for (name, listed, expect) in [
        ("claims-etha", members_except(&[]), "listed only by the bundle: ETHA; only by core: none"),
        ("drops-ibit", members_except(&["ETHA", "IBIT"]), "listed only by the bundle: none; only by core: IBIT"),
    ] {
        let mut h = header.clone();
        h.listed_members = Some(listed);
        let refused = publish(&mut client, &bundle(name, &h, &scalars, &files)).await.unwrap_err();
        assert!(
            matches!(&refused, Error::Refused(m) if m.contains("the worker and core disagree about the members listed on 2024-07-01") && m.contains(expect)),
            "{name}: {refused}"
        );
    }
    // a slot of the member not listed is refused
    let (mut h, mut f) = (header.clone(), files.clone());
    let q01 = h.slots.iter().position(|s| s.measurement_id == "VS-Q01").unwrap();
    let mut etha = h.slots[q01].clone();
    etha.object_id = "ETHA".into();
    h.slots.push(etha);
    f.push(f[q01].clone());
    let refused = publish(&mut client, &bundle("etha-slot", &h, &scalars, &f)).await.unwrap_err();
    assert!(matches!(&refused, Error::Refused(m) if m.contains("1 for objects not listed on 2024-07-01 (VS-Q01/ETHA)")), "{refused}");
    // on a day ETHA is listed its slots are expected, so the same AAPL-only bundle falls short
    let refused = publish(&mut client, &bundle("etha-listed", &parts(BASE).0, &scalars, &files)).await.unwrap_err();
    assert!(
        matches!(&refused, Error::Refused(m) if m.contains("4 missing (M0-ANALYTICS/ETHA, M0-GAMMA-CHANGE/ETHA, VS-IVS01/ETHA, VS-Q01/ETHA)")),
        "{refused}"
    );
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.day_revision").await, 0);

    // accepted with exactly the listed members' slots; the day records who was listed
    let published = publish(&mut client, &bundle("etha-unlisted", &header, &scalars, &files)).await.unwrap();
    assert_eq!((published.slots, published.streams), (4, 2));
    assert_eq!(recorded(&client, published.revision_id).await, (members_except(&["ETHA"]), 4));

    // the universe is loaded again with a different member list (AAPL left out): today's answer
    // changes, the stored day keeps the set it was published with and what it was expected to hold
    let mut doc = universe_document();
    doc["members"].as_array_mut().unwrap().retain(|m| m["asset_key"] != "AAPL");
    universe::store(&mut client, &doc).await.unwrap();
    assert_eq!(listed_on(&client, ETHA_UNLISTED).await, members_except(&["ETHA", "AAPL"]));
    assert_eq!(recorded(&client, published.revision_id).await, (members_except(&["ETHA"]), 4));
    scratch.drop().await;
}

/// One of the slice's derived sensors as a profile of its own under a build of its own (the rows,
/// bodies, receipts and scalars are the slice's; only the build and profile names are made here):
/// C-031 splits the derived sensors into a day-local profile (DLM-MAX-PAIN is one) and a history
/// profile (DLM-IV30 is one).
async fn derived_profile(client: &mut Client, measurement: &str, name: &str, day_local: bool) -> (Header, Scalars, Vec<SlotFiles>) {
    let (mut h, mut s, mut f) = parts(DERIVED);
    let mut inventory = inventories().remove(1);
    let profile = format!("{}-{name}", h.profile_revision);
    inventory["engine_build"] = json!(format!("{}-{name}", h.engine_build));
    inventory["slots"].as_array_mut().unwrap().retain(|row| row["measurement_id"] == measurement);
    for row in inventory["slots"].as_array_mut().unwrap() {
        row["profile_revision"] = json!(profile);
    }
    build::register(client, &inventory).await.unwrap();
    let keep: Vec<bool> = h.slots.iter().map(|slot| slot.measurement_id == measurement).collect();
    let mut i = 0;
    h.slots.retain(|_| (keep[i], i += 1).0);
    let mut i = 0;
    f.retain(|_| (keep[i], i += 1).0);
    s.reader_scalar.retain(|x| x.measurement_id == measurement);
    (h.engine_build, h.profile_revision, h.day_local) = (inventory["engine_build"].as_str().unwrap().to_owned(), profile, day_local);
    (h, s, f)
}

/// Audit 2026-11-14 (core.md) 2: a day-local revision published again takes down the revisions
/// that read history on its day (unusualness reads every reader value of the day) and on every
/// later day, as a base does; a history revision published again takes down the later days' ones.
/// Day-local revisions of other days and every base stay. Published here by hand, without jobs,
/// so nothing derives them again and a notice says so.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_replaced_day_local_or_history_revision_takes_down_what_read_it() {
    let (scratch, mut client) = ready("cascade").await;
    let day_local = derived_profile(&mut client, "DLM-MAX-PAIN", "DAY-LOCAL", true).await;
    let history = derived_profile(&mut client, "DLM-IV30", "HISTORY", false).await;
    let days = ["2025-12-10", "2025-12-11"];
    let send = async |client: &mut Client, (h, s, f): &(Header, Scalars, Vec<SlotFiles>), day: &str| {
        let mut h = h.clone();
        h.represented_date = day.into();
        publish(client, &bundle(&format!("cascade-{}-{day}", h.profile_revision), &h, s, f)).await.unwrap()
    };
    let base = parts(BASE);
    let mut ids = Vec::new();
    for day in days {
        for profile in [&base, &day_local, &history] {
            ids.push(send(&mut client, profile, day).await.revision_id);
        }
    }
    let published = async |client: &Client| -> Vec<i64> {
        client.query("SELECT revision_id FROM vyreon.day_revision WHERE state = 'PUBLISHED' ORDER BY 1", &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
    };
    assert_eq!(published(&client).await, ids);
    // [base 10th, day-local 10th, history 10th, base 11th, day-local 11th, history 11th]
    let again = send(&mut client, &day_local, days[0]).await;
    assert_eq!(again.superseded, 3, "the old day-local revision, and the history revisions of the 10th and the 11th");
    assert_eq!(published(&client).await, [ids[0], ids[3], ids[4], again.revision_id]);
    let said: String = client.query_one("SELECT message FROM core.notice WHERE NOT post ORDER BY notice_id DESC LIMIT 1", &[]).await.unwrap().get(0);
    assert!(said.contains("no job derives these again, schedule them: 2025-12-10, 2025-12-11"), "{said}");
    // and it needs the owner: a problem, posted once, open until each day has its job (C-050)
    let open = client.query("SELECT kind, summary FROM core.problem WHERE resolved_at IS NULL", &[]).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].get::<_, String>(0), "NOT_DERIVED");
    assert_eq!(open[0].get::<_, String>(1), "A republish left 2 days that no job derives again: 2025-12-10, 2025-12-11");
    let posted: Vec<String> = client.query("SELECT message FROM core.notice WHERE post", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(posted, ["🔴 A republish left 2 days that no job derives again: 2025-12-10, 2025-12-11\nTo see more: `vyreon-core log 20`"]);

    // the history derived again in order; then the 10th's history alone again takes the 11th's down
    let tenth = send(&mut client, &history, days[0]).await;
    let eleventh = send(&mut client, &history, days[1]).await;
    assert_eq!((tenth.superseded, eleventh.superseded), (0, 0));
    let tenth_again = send(&mut client, &history, days[0]).await;
    assert_eq!(tenth_again.superseded, 2, "the 10th's own previous history revision and the 11th's");
    assert_eq!(published(&client).await, [ids[0], ids[3], ids[4], again.revision_id, tenth_again.revision_id]);
    scratch.drop().await;
}
