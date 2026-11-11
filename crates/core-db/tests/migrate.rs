//! The migration runner against a real PostgreSQL server. `VYREON_CORE_TEST_PG` is a connection
//! string to a disposable server whose role may create databases; every test creates its own
//! database and drops it. Run with `cargo test -- --ignored`.
use core_db::migrate::{self, Error, Migration};
use tokio_postgres::{Client, Config, NoTls};

const NEEDS: &str = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)";

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(connection);
    client
}

/// A fresh database for one test: its config and the admin client that drops it.
struct Scratch {
    admin: Client,
    name: String,
    config: Config,
}

impl Scratch {
    async fn new(test: &str) -> Scratch {
        let admin_config: Config = std::env::var("VYREON_CORE_TEST_PG").expect(NEEDS).parse().expect("VYREON_CORE_TEST_PG");
        let admin = connect(&admin_config).await;
        let name = format!("core_test_{}_{test}", std::process::id());
        // one statement per call: CREATE/DROP DATABASE cannot run inside a multi-statement batch
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {name}")).await.unwrap();
        admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
        let mut config = admin_config.clone();
        config.dbname(&name);
        Scratch { admin, name, config }
    }

    async fn client(&self) -> Client {
        connect(&self.config).await
    }

    async fn drop(self) {
        self.admin.batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name)).await.unwrap();
    }
}

fn m(version: i32, name: &str, sql: &str) -> Migration {
    Migration { version, name: name.to_owned(), sql: sql.to_owned() }
}

async fn ledger(client: &Client) -> Vec<(i32, String)> {
    client.query("SELECT version, name FROM core.schema_migration ORDER BY version", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_embedded_migrations_apply_and_a_second_run_changes_nothing() {
    let scratch = Scratch::new("embedded").await;
    let mut client = scratch.client().await;
    let set = migrate::embedded().unwrap();
    let first = migrate::apply(&mut client, &set).await.unwrap();
    assert_eq!(first.applied.len(), set.len());
    assert_eq!(first.unchanged, 0);
    let second = migrate::apply(&mut client, &set).await.unwrap();
    assert!(second.applied.is_empty());
    assert_eq!(second.unchanged, set.len());
    assert_eq!(ledger(&client).await.len(), set.len());
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_later_run_applies_only_the_new_migrations() {
    let scratch = Scratch::new("later").await;
    let mut client = scratch.client().await;
    let one = [m(1, "a", "CREATE TABLE core.a (x int)")];
    migrate::apply(&mut client, &one).await.unwrap();
    let two = [one[0].clone(), m(2, "b", "CREATE TABLE core.b (x int)")];
    let report = migrate::apply(&mut client, &two).await.unwrap();
    assert_eq!(report.applied, [(2, "b".to_owned())]);
    assert_eq!(report.unchanged, 1);
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn concurrent_runners_apply_each_migration_once() {
    let scratch = Scratch::new("concurrent").await;
    let set = vec![m(1, "a", "CREATE TABLE core.a (x int); SELECT pg_sleep(0.3)"), m(2, "b", "CREATE TABLE core.b (x int)")];
    let mut runs = Vec::new();
    for _ in 0..4 {
        let (mut client, set) = (scratch.client().await, set.clone());
        runs.push(tokio::spawn(async move { migrate::apply(&mut client, &set).await }));
    }
    let mut applied = 0;
    for run in runs {
        applied += run.await.unwrap().unwrap().applied.len();
    }
    assert_eq!(applied, 2, "each migration applied by exactly one runner");
    assert_eq!(ledger(&scratch.client().await).await.len(), 2);
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_changed_applied_migration_is_refused() {
    let scratch = Scratch::new("changed").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &[m(1, "a", "CREATE TABLE core.a (x int)")]).await.unwrap();
    let edited = [m(1, "a", "CREATE TABLE core.a (x bigint)")];
    assert!(matches!(migrate::apply(&mut client, &edited).await, Err(Error::Changed { version: 1, .. })));
    let renamed = [m(1, "z", "CREATE TABLE core.a (x int)")];
    assert!(matches!(migrate::apply(&mut client, &renamed).await, Err(Error::Changed { version: 1, .. })));
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn an_older_binary_refuses_a_newer_database() {
    let scratch = Scratch::new("unknown").await;
    let mut client = scratch.client().await;
    let two = [m(1, "a", "CREATE TABLE core.a (x int)"), m(2, "b", "CREATE TABLE core.b (x int)")];
    migrate::apply(&mut client, &two).await.unwrap();
    assert!(matches!(migrate::apply(&mut client, &two[..1]).await, Err(Error::Unknown { version: 2, .. })));
    scratch.drop().await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_failing_migration_rolls_back_whole_and_stops_the_run() {
    let scratch = Scratch::new("failing").await;
    let mut client = scratch.client().await;
    let set = [
        m(1, "a", "CREATE TABLE core.a (x int)"),
        m(2, "b", "CREATE TABLE core.b (x int); SELECT 1 / 0"),
        m(3, "c", "CREATE TABLE core.c (x int)"),
    ];
    assert!(matches!(migrate::apply(&mut client, &set).await, Err(Error::Failed { version: 2, .. })));
    assert_eq!(ledger(&client).await, [(1, "a".to_owned())]);
    let tables: i64 = client.query_one("SELECT count(*) FROM pg_tables WHERE schemaname = 'core' AND tablename IN ('b', 'c')", &[]).await.unwrap().get(0);
    assert_eq!(tables, 0, "the failed migration left nothing behind and the next did not run");
    // the lock was released: a corrected set applies on the same session
    let fixed = [set[0].clone(), m(2, "b", "CREATE TABLE core.b (x int)"), set[2].clone()];
    assert_eq!(migrate::apply(&mut client, &fixed).await.unwrap().applied.len(), 2);
    scratch.drop().await;
}
