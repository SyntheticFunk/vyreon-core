//! What the tests of every crate share: a fresh database per test on the disposable server named
//! by `VYREON_CORE_TEST_PG` (a role that may create databases), and the frozen real data under
//! `fixtures/` (each file's origin is stated where it is used).
use tokio_postgres::{Client, Config, NoTls};

pub mod slice;

const NEEDS: &str = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)";

pub async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(connection);
    client
}

/// A fresh database for one test: its config and the admin client that drops it.
pub struct Scratch {
    admin: Client,
    name: String,
    pub config: Config,
}

impl Scratch {
    pub async fn new(test: &str) -> Scratch {
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

    pub async fn client(&self) -> Client {
        connect(&self.config).await
    }

    pub async fn drop(self) {
        self.admin.batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name)).await.unwrap();
    }
}

/// The directory of frozen real data.
pub fn fixtures() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The engine's trading sessions at engine-v1 (its `list_sessions`, read on the droplet 2026-11-11).
pub const CALENDAR: &str = include_str!("../fixtures/engine-v1-sessions.json");

/// AAPL's reduced tape of 2025-12-10 from the droplet, packed as its four files.
/// A bundle the engine wrote (contract 2, `vyreon-worker bundle`, engine b9046fb5 + contract 2)
/// from its retained real exports of 2025-12-10: AAPL's five slots without an event stream, each
/// with its coordinates view.
pub const ENGINE_BUNDLE_AAPL: &[u8] = include_bytes!("../fixtures/engine-bundle-2025-12-10-aapl.tar.gz");

pub const TAPE_AAPL: &[u8] = include_bytes!("../fixtures/tape-cache-AAPL-2025-12-10.tar.gz");
