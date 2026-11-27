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

/// ARKK's PCR-UPATH-I01 of 2024-08-15 as phase 1 published it (revision 183, read-only from the cluster's
/// database 2026-11-25): withheld whole, its roles' reason ENGINE_FAULT, no terminal reason.
pub const ENGINE_FAULT_BODY_ARKK: &str = include_str!("../fixtures/engine-fault-body-2024-08-15-ARKK-PCR-UPATH-I01.json");

/// Seven bodies of 2026-11-24/25 as the nightly published them (revisions 218 and 256, read-only from the
/// cluster's database 2026-11-26, each `body_text` verbatim: its SHA-256 with a line end is the stored
/// `body_sha256`), one of each schema that states a withholding outside `measurement.value` and one that
/// states none: `[{measurement_id, object_id, represented_date, body_sha256, body_text}]`. BABA's CDD-CAPVAL01
/// and TSM's CDD-EVENTS01 (`H5-COMPANY-OWNER-RESULT@1`), FIELD-OPTBR01 (a FIELD body), AAPL's M0-PRINT-SIZE of
/// 11-24 (`H4-PRINT-MEASUREMENT@1`, withheld) and SPY's of 11-25 (supported), DBA's SAI-ACT01-INTRADAY
/// (`H5-DIRECT-OWNER-MEASUREMENT@1`, its `array_ref_coordinates`), EMB's VS-V03-CURRENT (`H4-V03-MEASUREMENT@1`).
pub const CENSUS_BODIES_2026_11_25: &str = include_str!("../fixtures/census-bodies-2026-11-25.json");

/// IBIT's M0-GAMMA of 2024-08-15 as phase 1 published it (revision 183): withheld whole, its result id
/// ending `/engine-fault` like every withheld slot's, its reason the provider's (EMPTY_DELIVERED_POPULATION:
/// IBIT's options were not listed yet).
pub const PROVIDER_ABSENCE_BODY_IBIT: &str = include_str!("../fixtures/provider-absence-body-2024-08-15-IBIT-M0-GAMMA.json");

/// That body's coordinates view as the frozen engine (93a04676, engine D-512 item 1) projects it
/// (`vyreon_coordinates::project`, run on the body above): a body withheld whole states its terminal
/// beside its empty coordinates (C-081).
pub const COORDINATES_WITHHELD_WHOLE_IBIT: &str = include_str!("../fixtures/coordinates-2024-08-15-IBIT-M0-GAMMA-withheld-whole.json");

/// A real reference-check report (C-054): the suite's report of 2025-12-16 from the engine's B0
/// census (`python3 -m refcheck run` on the B0 panel's day, 2026-10-25), with its per-comparison
/// `results` and `per_object` rows left out (as a worker leaves them out past core's limits),
/// gzipped; the engine's `tests/fixtures/refcheck` holds the same bytes. 91,066 comparisons
/// checked, 3,958 failed (on B0's build, before the decisions since), 384
/// failure classes over 108 measurements.
pub const REFCHECK_REPORT_2025_12_16: &[u8] = include_bytes!("../fixtures/refcheck-report-2025-12-16-b0-slim.json.gz");

/// The document of core's re-run of 2024-07-10's reference check (report 33, job 23174, revisions 144
/// and 145, 2026-11-25), as its worker sent it: the account line, then the report with only its
/// `dump`, `date`, `summary` and `failure_classes` kept, gzipped. The suite read core's dump with its
/// keys sorted (C-080) and found 3,332 of its slots "not published": 18,567 comparisons checked, 3,380
/// failed.
pub const REFCHECK_RERUN_2024_07_10_UNREAD: &[u8] = include_bytes!("../fixtures/refcheck-rerun-2024-07-10-unread-slim.json.gz");

/// The engine's two day holds as its worker reports a held day to core (`fail`'s message): `stop:
/// <date> not published: ` and the hold (vyreon-cli `days.rs` `why`, `exports.rs` `hold_final`, at
/// engine-v1 and the worker branch of 2026-11-21). No held day's real message is kept anywhere core
/// can read, so the counts are chosen for the test (the thresholds are the engine's defaults, a
/// tenth); core reads only the text's shape. The slots named are real: the slice's AAPL slots, and
/// XLB's SAI-EXQ01, the engine's real faulted slot of 2025-12-10 (`ENGINE_FAULT_BODY_XLB`).
pub fn provider_input_hold(day: &str) -> String {
    format!("stop: {day} not published: held: 412 of 3904 final slots lack a provider input (more than 0.1): AAPL--VS-Q01 AAPL--VS-IVS01 ...")
}

pub fn engine_fault_hold(day: &str) -> String {
    format!("stop: {day} not published: held: 400 of 3904 final slots are withheld for an engine fault (more than 0.1): XLB--SAI-EXQ01 ...")
}

/// The engine's two other holds for its own faults, as its worker reports them (the same `stop: <date>
/// not published: ` before them; engine worker branch of 2026-11-21): the executor's exit 1 (R7.1,
/// vyreon-cli `day.rs`, `Stop::Held("intent <name>: the executor held the day (too many faulted
/// slots; <its log>)")`), and the day-local derive's (D-103, vyreon-cli `derive/local.rs`, an error
/// `"<date> held: N of M objects' day-local derived records faulted (more than X); <inherited>:
/// <objects>"`, not a `Stop::Held`). As above, no real message of either is kept where core can read
/// it: the counts, the log's path and the objects are chosen for the test; core reads the shape.
pub fn executor_fault_hold(day: &str) -> String {
    format!("stop: {day} not published: held: intent a: the executor held the day (too many faulted slots; /work/runs/worker-{day}/logs/intent-a.err)")
}

pub fn day_local_fault_hold(day: &str) -> String {
    format!(
        "stop: {day} not published: {day} held: 9 of 64 objects' day-local derived records faulted (more than 0.1); \
         0 slots withheld by inheritance from an engine fault, 12 from a provider absence (not counted by the hold): [\"XLB\", \"XLE\"]"
    )
}

/// Two real lines of the refused day's fill log (`WORKER_LOG_REFUSED_DAY`, GLD's and PLTR's
/// option captures stopped by the provider's HTTP 502), as a two-line stop message: a stop that is
/// not one of the engine's holds.
pub fn provider_502_stop() -> String {
    WORKER_LOG_REFUSED_DAY
        .lines()
        .filter(|l| l.contains("HttpStatus(502)"))
        .take(2)
        .map(|l| l.split_once(':').expect("a numbered line").1.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

/// C-074: the engine's republish plans as its inventory declares them (engine D-462,
/// `vyreon-worker inventory` of the eng-scope branch): every family's and measurement's recompute
/// set, stages and input categories from the engine's sensor graph, and its reference timings.
pub const REPUBLISH_PLANS: &str = include_str!("../fixtures/engine-republish-plans.json");

/// `inventory` with the engine's republish plans declared, as a D-462 build registers.
pub fn with_republish(mut inventory: serde_json::Value) -> serde_json::Value {
    inventory["republish"] = serde_json::from_str(REPUBLISH_PLANS).expect("the engine's plans");
    inventory
}
