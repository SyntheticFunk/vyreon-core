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

/// Two real tape checks (C-058, C-059): the verify jobs' documents for AAPL's cache of 2025-12-01
/// as core's cluster stored them (`core.tape_check` 748 and 1768, read 2026-11-20), each with the
/// tape reader's verdict inside: FAIL by the reader of build 3b85b4f6 ("conditional_last_rejected
/// reader 38, cache 39"), then PASS by the corrected reader of cdf58be2 on the same recomputed
/// bytes. Besides digests, counts and versions they carry only AAPL's day volume (46,633,227).
pub const TAPE_CHECK_AAPL_FAIL: &str = include_str!("../fixtures/tape-check-AAPL-2025-12-01-fail.json");
pub const TAPE_CHECK_AAPL_PASS: &str = include_str!("../fixtures/tape-check-AAPL-2025-12-01-pass.json");

/// A real tape check (`TAPE_CHECK_AAPL_FAIL` or `_PASS`) bound to the cache a test holds: its
/// asset, session, method version and digests (the recomputed cache's, and its verdict's
/// `cache_sha256`) set to the test's; every other field as the cluster stored it. Only these
/// fields change: a test's cache is AAPL's of 2025-12-10 (`TAPE_AAPL`), for which no real check
/// exists.
pub fn tape_check_bound(real: &str, asset: &str, session: &str, method: &str, cache_sha256: &str) -> serde_json::Value {
    let mut c: serde_json::Value = serde_json::from_str(real).expect("a frozen tape check");
    c["asset"] = asset.into();
    c["session"] = session.into();
    c["method_version"] = method.into();
    c["recomputed_sha256"] = cache_sha256.into();
    let v = &mut c["verdict"];
    v["asset"] = asset.into();
    v["session"] = session.into();
    v["method_version"] = method.into();
    v["cache_sha256"] = cache_sha256.into();
    c
}

/// The verdict inside a real tape check bound as `tape_check_bound` binds it: what a tape worker
/// sends beside the cache it stored (C-058's document is this object).
pub fn tape_verdict_bound(real: &str, asset: &str, session: &str, method: &str, cache_sha256: &str) -> serde_json::Value {
    tape_check_bound(real, asset, session, method, cache_sha256)["verdict"].take()
}

/// The real macro capture of 2025-12-03 (the engine's frozen `h3-macro/tests/fixtures/
/// macro-2025-12-03`, fetched 2026-11-16) packed by the engine as core keeps it (`vyreon-worker
/// pack-capture`, engine pass-fx13; the engine's test pins the same SHA-256, 96709c90…). Its
/// identity is the SHA-256 of its manifest.
pub const MACRO_CAPTURE: &[u8] = include_bytes!("../fixtures/input-capture-macro-2025-12-03.tar.gz");
pub const MACRO_CAPTURE_IDENTITY: &str = "efc96bf3d914609ebb5cb1d2c6837976cb355071b5e7b5e5f4417ea2024a4bae";

/// A real worker's log of a refused day (job 53, attempt 2, day 2024-07-16, a cluster worker on
/// 2026-11-16): the lines of its fill log that name an error, with their line numbers (the longest
/// cut at 500 characters by whoever kept the excerpt), then the log's tail. Checked before it was
/// frozen: no credential in it.
pub const WORKER_LOG_REFUSED_DAY: &str = include_str!("../fixtures/worker-fill-log-2024-07-16-refused.txt");

/// A real faulted slot: the engine's containment body for XLB's SAI-EXQ01 on 2025-12-10 (every
/// role WITHHELD for ENGINE_FAULT, its result id `SAI-EXQ01/XLB/2025-12-10/engine-fault`), from
/// the engine's frozen exports of that day (`vyreon-cli/tests/fixtures/exports-2025-12-10`, engine
/// 9ac5010c), copied 2026-11-17. Its SHA-256 is the one its export index states (326760d7…).
pub const ENGINE_FAULT_BODY_XLB: &str = include_str!("../fixtures/engine-fault-body-2025-12-10-XLB-SAI-EXQ01.json");

/// A real reference-check report (C-054): the suite's report of 2025-12-16 from the engine's B0
/// census (`python3 -m refcheck run` on the B0 panel's day, 2026-10-25), with its per-comparison
/// `results` and `per_object` rows left out (as a worker leaves them out past core's limits),
/// gzipped; the engine's `tests/fixtures/refcheck` holds the same bytes. 91,066 comparisons
/// checked, 3,958 failed (on B0's build, before the decisions since), 384
/// failure classes over 108 measurements.
pub const REFCHECK_REPORT_2025_12_16: &[u8] = include_bytes!("../fixtures/refcheck-report-2025-12-16-b0-slim.json.gz");
