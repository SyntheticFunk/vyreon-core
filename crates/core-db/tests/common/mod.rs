//! What the database tests share: a fresh database per test on the disposable server named by
//! `VYREON_CORE_TEST_PG` (a role that may create databases).
#![allow(dead_code)]
use tokio_postgres::{Client, Config, NoTls};

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
