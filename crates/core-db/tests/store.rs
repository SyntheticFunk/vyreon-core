//! The store schema against a real PostgreSQL server, with a frozen slice of a real published day
//! (`fixtures/droplet-2025-12-10-aapl.json`: AAPL on 2025-12-10 from the droplet's store, six
//! sensors across the base and derived revisions, exported 2026-11-10). The slice's revisions get
//! core's own ids; publishing is a plain state change here, the publish path has its own tests.
//! Run with `cargo test -- --ignored`.
use test_support::Scratch;
use core_db::migrate;
use std::collections::HashMap;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, Error};

use test_support::slice::SLICE;

async fn migrated(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    (scratch, client)
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

fn state(e: &Error) -> Option<&SqlState> {
    e.code()
}

/// Registers the builds the slice's revisions name.
async fn register_builds(client: &Client) {
    client
        .execute(
            "INSERT INTO core.engine_build (engine_build)
             SELECT DISTINCT engine_build FROM jsonb_to_recordset($1::text::jsonb->'day_revision') AS r(engine_build text)",
            &[&SLICE],
        )
        .await
        .unwrap();
}

/// Inserts the slice's revisions as STAGED and returns (the slice's id, core's id) pairs, base first.
async fn stage_revisions(client: &Client) -> Vec<(i64, i64)> {
    register_builds(client).await;
    let rows = client
        .query(
            "SELECT revision_id, base_revision FROM jsonb_to_recordset($1::text::jsonb->'day_revision')
                 AS r(revision_id bigint, base_revision bigint) ORDER BY base_revision NULLS FIRST, revision_id",
            &[&SLICE],
        )
        .await
        .unwrap();
    let mut ids: HashMap<i64, i64> = HashMap::new();
    let mut out = Vec::new();
    for row in rows {
        let (old, base): (i64, Option<i64>) = (row.get(0), row.get(1));
        let new: i64 = client
            .query_one(
                "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, base_revision)
                 SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, $3
                 FROM jsonb_to_recordset($1::text::jsonb->'day_revision')
                     AS r(revision_id bigint, represented_date date, profile_id text, profile_revision text, engine_build text, snapshot_cutoff timestamptz)
                 WHERE revision_id = $2 RETURNING revision_id",
                &[&SLICE, &old, &base.map(|b| ids[&b])],
            )
            .await
            .unwrap()
            .get(0);
        ids.insert(old, new);
        out.push((old, new));
    }
    out
}

/// Inserts the slice's measurement rows of one revision, dated `shift` days from the revision's day.
async fn insert_measurements(client: &Client, old: i64, new: i64, shift: i32) -> Result<u64, Error> {
    client
        .execute(
            "INSERT INTO vyreon.measurement (revision_id, measurement_id, object_id, represented_date, slot_state, science_version,
                 method_version, body_sha256, body_text, body, receipt, carried_from_revision)
             SELECT d.revision_id, m.measurement_id, m.object_id, d.represented_date + $4::int, m.slot_state, m.science_version,
                    m.method_version, m.body_sha256, m.body_text, m.body, m.receipt, NULL
             FROM jsonb_to_recordset($1::text::jsonb->'measurement')
                 AS m(revision_id bigint, measurement_id text, object_id text, slot_state text, science_version text,
                      method_version text, body_sha256 text, body_text text, body jsonb, receipt jsonb)
             JOIN vyreon.day_revision d ON d.revision_id = $3
             WHERE m.revision_id = $2",
            &[&SLICE, &old, &new, &shift],
        )
        .await
}

async fn insert_streams_and_scalars(client: &Client, old: i64, new: i64) {
    client
        .execute(
            "INSERT INTO vyreon.measurement_stream (revision_id, measurement_id, object_id, represented_date, root_sha256, root_bytes,
                 events_sha256, event_count, byte_length, events_gzip)
             SELECT d.revision_id, s.measurement_id, s.object_id, d.represented_date, s.root_sha256, s.root_bytes, s.events_sha256,
                    s.event_count, s.byte_length, s.events_gzip
             FROM jsonb_to_recordset($1::text::jsonb->'measurement_stream')
                 AS s(revision_id bigint, measurement_id text, object_id text, root_sha256 text, root_bytes bytea,
                      events_sha256 text, event_count bigint, byte_length bigint, events_gzip bytea)
             JOIN vyreon.day_revision d ON d.revision_id = $3
             WHERE s.revision_id = $2",
            &[&SLICE, &old, &new],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO vyreon.reader_scalar (revision_id, object_id, measurement_id, role_id, represented_date, value)
             SELECT d.revision_id, s.object_id, s.measurement_id, s.role_id, d.represented_date, s.value
             FROM jsonb_to_recordset($1::text::jsonb->'reader_scalar')
                 AS s(revision_id bigint, object_id text, measurement_id text, role_id text, value double precision)
             JOIN vyreon.day_revision d ON d.revision_id = $3
             WHERE s.revision_id = $2",
            &[&SLICE, &old, &new],
        )
        .await
        .unwrap();
}

/// The whole slice staged: both revisions with their rows.
async fn stage_slice(client: &Client) -> Vec<(i64, i64)> {
    let revisions = stage_revisions(client).await;
    for &(old, new) in &revisions {
        insert_measurements(client, old, new, 0).await.unwrap();
        insert_streams_and_scalars(client, old, new).await;
    }
    revisions
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn months_are_created_once_with_exact_bounds() {
    let (scratch, client) = migrated("months").await;
    let made: i32 = client.query_one("SELECT vyreon.ensure_months('2025-11-15', '2026-01-02')", &[]).await.unwrap().get(0);
    assert_eq!(made, 12, "three months of four tables");
    let again: i32 = client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2025-12-31')", &[]).await.unwrap().get(0);
    assert_eq!(again, 0);
    let parts: Vec<(String, String)> = client
        .query(
            "SELECT c.relname, pg_get_expr(c.relpartbound, c.oid) FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid
             WHERE i.inhparent = 'vyreon.measurement'::regclass ORDER BY 1",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let expect = |name: &str, from: &str, to: &str| (name.to_owned(), format!("FOR VALUES FROM ('{from}') TO ('{to}')"));
    assert_eq!(
        parts,
        [
            expect("measurement_y2025m11", "2025-11-01", "2025-12-01"),
            expect("measurement_y2025m12", "2025-12-01", "2026-01-01"),
            expect("measurement_y2026m01", "2026-01-01", "2026-02-01"),
        ]
    );
    // the gzip column keeps its storage in a partition, and a partition has its parent's key and foreign keys
    let storage: i8 = client
        .query_one("SELECT attstorage FROM pg_attribute WHERE attrelid = 'vyreon.measurement_stream_y2025m12'::regclass AND attname = 'events_gzip'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(storage as u8 as char, 'e');
    let constraints = count(&client, "SELECT count(*) FROM pg_constraint WHERE conrelid = 'vyreon.measurement_stream_y2025m12'::regclass AND contype IN ('p', 'f')").await;
    assert_eq!(constraints, 2);
    for bad in ["SELECT vyreon.ensure_months('2026-01-01', '2025-12-01')", "SELECT vyreon.ensure_months('2025-01-01', '20225-01-01')", "SELECT vyreon.ensure_months(NULL, '2025-01-01')"] {
        assert!(client.query_one(bad, &[]).await.is_err(), "{bad}");
    }
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn concurrent_callers_create_each_month_once() {
    let (scratch, _client) = migrated("months_concurrent").await;
    let mut calls = Vec::new();
    for _ in 0..4 {
        let client = scratch.client().await;
        calls.push(tokio::spawn(async move {
            let made: i32 = client.query_one("SELECT vyreon.ensure_months('2025-03-01', '2025-08-31')", &[]).await.unwrap().get(0);
            made
        }));
    }
    let mut made = 0;
    for call in calls {
        made += call.await.unwrap();
    }
    assert_eq!(made, 24);
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_real_day_slice_is_stored_in_its_month_and_reads_back_exact() {
    let (scratch, client) = migrated("slice").await;
    client.query_one("SELECT vyreon.ensure_months('2025-11-01', '2026-01-31')", &[]).await.unwrap();
    let revisions = stage_slice(&client).await;
    assert_eq!(revisions.len(), 2);
    for (table, rows) in [("measurement", 6), ("measurement_stream", 2), ("reader_scalar", 4)] {
        assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.{table}")).await, rows, "{table}");
        assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.{table}_y2025m12")).await, rows, "{table} partition");
    }
    // the stored text still hashes to its digest (the engine hashes the body file: the text and
    // its final newline), agrees with the JSONB, and each stream's root hashes to its name
    let exact = count(
        &client,
        "SELECT count(*) FROM vyreon.measurement
         WHERE encode(sha256(convert_to(body_text || E'\\n', 'UTF8')), 'hex') = body_sha256 AND body_text::jsonb = body",
    )
    .await;
    assert_eq!(exact, 6);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.measurement_stream WHERE encode(sha256(root_bytes), 'hex') = root_sha256").await, 2);
    let same_text = count(
        &client,
        &format!(
            "SELECT count(*) FROM vyreon.measurement m JOIN jsonb_to_recordset('{}'::jsonb->'measurement') AS f(measurement_id text, body_text text)
                 USING (measurement_id, body_text)",
            SLICE.replace('\'', "''")
        ),
    )
    .await;
    assert_eq!(same_text, 6);

    // staged revisions are invisible; published ones are read through the views by the reader role only
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.published_measurement").await, 0);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.accepted_stream").await, 0);
    client.execute("UPDATE vyreon.day_revision SET state = 'PUBLISHED', published_at = now()", &[]).await.unwrap();
    client.batch_execute("SET ROLE vyreon_reader").await.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.published_measurement WHERE represented_date = '2025-12-10'").await, 6);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.accepted_measurement WHERE object_id = 'AAPL'").await, 6);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.accepted_stream").await, 2);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.accepted_reader_scalar").await, 4);
    for table in ["vyreon.measurement", "vyreon.day_revision", "vyreon.measurement_y2025m12", "core.schema_migration"] {
        let refused = client.query_one(&format!("SELECT count(*) FROM {table}"), &[]).await.unwrap_err();
        assert_eq!(state(&refused), Some(&SqlState::INSUFFICIENT_PRIVILEGE), "{table}");
    }
    client.batch_execute("RESET ROLE").await.unwrap();

    // a date filter on the view reaches only that month's partition
    let plan: String = client
        .query("EXPLAIN SELECT body_text FROM vyreon.published_measurement WHERE represented_date = '2025-12-10'", &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plan.contains("measurement_y2025m12"), "{plan}");
    assert!(!plan.contains("measurement_y2025m11") && !plan.contains("measurement_y2026m01"), "{plan}");

    // a second published revision of the same day and profile is refused
    let second = client
        .execute("INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, state)
                  SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, 'PUBLISHED' FROM vyreon.day_revision WHERE base_revision IS NULL", &[])
        .await
        .unwrap_err();
    assert_eq!(state(&second), Some(&SqlState::UNIQUE_VIOLATION));
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_revision_needs_a_registered_build() {
    let (scratch, client) = migrated("build").await;
    let insert = "INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff)
                  SELECT represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff
                  FROM jsonb_to_recordset($1::text::jsonb->'day_revision')
                      AS r(represented_date date, profile_id text, profile_revision text, engine_build text, snapshot_cutoff timestamptz, base_revision bigint)
                  WHERE base_revision IS NULL";
    let unregistered = client.execute(insert, &[&SLICE]).await.unwrap_err();
    assert_eq!(state(&unregistered), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    register_builds(&client).await;
    assert_eq!(client.execute(insert, &[&SLICE]).await.unwrap(), 1);
    // admission carries its evidence, and only an admitted build can be withdrawn
    for bad in ["UPDATE core.engine_build SET admitted_at = now()", "UPDATE core.engine_build SET withdrawn_at = now(), withdrawal = 'x'"] {
        assert_eq!(state(&client.execute(bad, &[]).await.unwrap_err()), Some(&SqlState::CHECK_VIOLATION), "{bad}");
    }
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_row_cannot_leave_its_revisions_day_and_goes_with_its_revision() {
    let (scratch, client) = migrated("integrity").await;
    let revisions = stage_revisions(&client).await;
    let (base_old, base) = revisions[0];
    let (derived_old, derived) = revisions[1];

    // no partition for the month: refused, not parked in a default partition
    let no_month = insert_measurements(&client, base_old, base, 0).await.unwrap_err();
    assert_eq!(state(&no_month), Some(&SqlState::CHECK_VIOLATION));
    client.query_one("SELECT vyreon.ensure_months('2025-12-01', '2026-01-31')", &[]).await.unwrap();
    // another day of the same month, and a day of another month whose partition exists: both refused
    for shift in [1, 30] {
        let wrong_day = insert_measurements(&client, base_old, base, shift).await.unwrap_err();
        assert_eq!(state(&wrong_day), Some(&SqlState::FOREIGN_KEY_VIOLATION), "shift {shift}");
    }
    // a derived revision's base is a revision of the same day
    let other_day = client
        .execute("INSERT INTO vyreon.day_revision (represented_date, profile_id, profile_revision, engine_build, snapshot_cutoff, base_revision)
                  SELECT represented_date + 1, profile_id, profile_revision, engine_build, snapshot_cutoff, base_revision
                  FROM vyreon.day_revision WHERE revision_id = $1", &[&derived])
        .await
        .unwrap_err();
    assert_eq!(state(&other_day), Some(&SqlState::FOREIGN_KEY_VIOLATION));

    for &(old, new) in &revisions {
        insert_measurements(&client, old, new, 0).await.unwrap();
        insert_streams_and_scalars(&client, old, new).await;
    }
    // a stream needs its measurement
    let orphan = client
        .execute("INSERT INTO vyreon.measurement_stream (revision_id, measurement_id, object_id, represented_date, root_sha256, root_bytes,
                      events_sha256, event_count, byte_length, events_gzip)
                  SELECT $1, measurement_id, object_id, represented_date, root_sha256, root_bytes, events_sha256, event_count, byte_length, events_gzip
                  FROM vyreon.measurement_stream", &[&derived])
        .await
        .unwrap_err();
    assert_eq!(state(&orphan), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    // a base revision cannot be deleted from under its derived revision
    let held = client.execute("DELETE FROM vyreon.day_revision WHERE revision_id = $1", &[&base]).await.unwrap_err();
    assert_eq!(state(&held), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    // deleting a revision removes its rows and nothing else
    assert_eq!(insert_measurements(&client, derived_old, derived, 0).await.unwrap_err().code(), Some(&SqlState::UNIQUE_VIOLATION));
    client.execute("DELETE FROM vyreon.day_revision WHERE revision_id = $1", &[&derived]).await.unwrap();
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.measurement").await, 4);
    assert_eq!(count(&client, "SELECT count(*) FROM vyreon.reader_scalar").await, 1);
    client.execute("DELETE FROM vyreon.day_revision WHERE revision_id = $1", &[&base]).await.unwrap();
    for table in ["measurement", "measurement_stream", "reader_scalar"] {
        assert_eq!(count(&client, &format!("SELECT count(*) FROM vyreon.{table}")).await, 0, "{table}");
    }
    scratch.drop().await;
}
