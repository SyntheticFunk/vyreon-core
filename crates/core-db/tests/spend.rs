//! C-078: the spend against the owner's ceiling, from DigitalOcean's own billing figures as core keeps
//! them. The figures are the account's real ones as the owner's running total recorded them
//! (MILESTONES: month to date $180.41 at 2026-11-21 06:09 UTC, the start; $269.04 at 2026-11-24 06:42 UTC)
//! and the shapes are DigitalOcean's (`/v2/customers/my/balance`, `/v2/customers/my/invoices`). Run with
//! `cargo test -- --ignored`.
use core_db::spend::{self, Reading, Spend, SpendWatch};
use core_db::watch::{self, Settings};
use core_db::{migrate, work};
use serde_json::json;
use test_support::Scratch;
use tokio_postgres::Client;

fn watch_of(ceiling: f64) -> SpendWatch {
    SpendWatch { since: "2026-11-21T06:09:00Z".into(), baseline_usd: 180.41, ceiling_usd: ceiling }
}

fn settings(w: SpendWatch) -> Settings {
    Settings {
        profile: "FREE_REPORT_64".into(),
        deadlines_from: None,
        volume_bytes: None,
        deadline_hour: watch::DEADLINE_HOUR,
        daily: None,
        archive: None,
        refcheck_from: None,
        backup_days: None,
        spend: Some(w),
        recompute: None,
        refcheck_rerun_hours: None, phase: None,
    }
}

/// A reading of DigitalOcean's answers, kept as read at `at`.
async fn reading(client: &Client, at: &str, month_to_date: &str, generated: &str, invoices: serde_json::Value) {
    let balance = json!({"month_to_date_balance": month_to_date, "account_balance": "0.00", "month_to_date_usage": month_to_date, "generated_at": generated});
    let r: Reading = spend::from_answers(&balance, &json!({"invoices": invoices}), &generated[..7]).unwrap();
    spend::put_reading(client, Ok(&r)).await.unwrap();
    client.execute("UPDATE core.spend_reading SET read_at = $1::text::timestamptz WHERE reading_id = (SELECT max(reading_id) FROM core.spend_reading)", &[&at]).await.unwrap();
}

async fn open(client: &Client) -> Vec<(String, String)> {
    client.query("SELECT key, summary FROM core.problem WHERE kind = 'SPEND' AND resolved_at IS NULL ORDER BY key", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect()
}

async fn ready(test: &str) -> (Scratch, Client) {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    (scratch, client)
}

/// The spend since the start: November's month-to-date less the start's, then (in December) November's
/// invoice (or, before it is issued, the highest figure read in November) plus December's month to date.
/// Its rate over the last day and where that rate reaches the ceiling; alerts at 80% and 100%, kept while
/// the figures cannot be read; the morning's line once a day on the progress channel; no token said plainly.
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn the_spend_since_the_start_is_watched_against_the_ceiling() {
    let (scratch, mut client) = ready("c078_spend").await;
    let w = watch_of(500.0);
    // nothing read yet
    let now = "2026-11-24 07:00:00+00";
    assert_eq!(spend::spend(&client, &w, now).await.unwrap(), Spend::Unread("core has not read DigitalOcean's billing yet".into()));
    // no token: said, never a failure
    spend::put_reading(&client, Err("no DigitalOcean token: the secret vyreon-core-do-billing is not made (deploy/k8s/README.md, Spend)")).await.unwrap();
    client.execute("UPDATE core.spend_reading SET read_at = '2026-11-23 06:50:00+00'", &[]).await.unwrap();
    let line = spend::line(&spend::spend(&client, &w, "2026-11-23 07:00:00+00").await.unwrap());
    assert_eq!(line, "💵 Spend: not known (no DigitalOcean token: the secret vyreon-core-do-billing is not made (deploy/k8s/README.md, Spend)); the ceiling is not watched meanwhile");

    // the real figures: DigitalOcean's of 20 h, 24 h (read twice: an hourly read of the same figure) and 0 h
    // before; the rate is over the one nearest 24 h (release-3 audit, Codex 14)
    reading(&client, "2026-11-23 06:40:00+00", "238.10", "2026-11-23T06:30:00Z", json!([])).await;
    reading(&client, "2026-11-23 07:40:00+00", "238.10", "2026-11-23T06:30:00Z", json!([])).await;
    reading(&client, "2026-11-23 10:40:00+00", "243.00", "2026-11-23T10:30:00Z", json!([])).await;
    reading(&client, "2026-11-24 06:42:00+00", "269.04", "2026-11-24T06:30:00Z", json!([])).await;
    let Spend::Known(k) = spend::spend(&client, &w, now).await.unwrap() else { panic!("known") };
    assert!((k.spent - 88.63).abs() < 1e-9, "{}", k.spent);
    let (rate, over) = k.rate.clone().unwrap();
    assert!((rate - 30.94).abs() < 1e-9, "{rate}");
    assert_eq!(over, "over the last 24 h");
    // the spend now: DigitalOcean's figure of 06:30 and half an hour at that rate (Codex 7)
    assert!((k.now - (88.63 + 30.94 / 48.0)).abs() < 1e-9, "{}", k.now);
    assert_eq!(k.reached.as_ref().map(|(d, _)| d.as_str()), Some("Dec 7"));
    assert_eq!(
        spend::line(&Spend::Known(k)),
        "💵 Spend since Nov 21: ≈$89.27 of the $500.00 ceiling (18%) ($88.63 at DigitalOcean's figure of Nov 24 00:30 MT, then at the rate); $30.94 a day over the last 24 h; at that rate the ceiling is reached about Dec 7 (in 13 days)"
    );
    // core's reading stale is no evidence; nor is DigitalOcean's figure, however freshly read again
    assert!(matches!(spend::spend(&client, &w, "2026-11-24 12:00:00+00").await.unwrap(), Spend::Unread(ref why) if why.contains("newest reading is 5 h old")));
    reading(&client, "2026-11-25 10:00:00+00", "269.04", "2026-11-24T06:30:00Z", json!([])).await;
    assert!(matches!(spend::spend(&client, &w, "2026-11-25 10:05:00+00").await.unwrap(), Spend::Unread(ref why) if why.contains("DigitalOcean's newest figure is 27 h old")));
    client.execute("DELETE FROM core.spend_reading WHERE read_at = '2026-11-25 10:00:00+00'", &[]).await.unwrap();

    // the morning's line: once, at 09:00 MT, on the progress channel
    let s = settings(w.clone());
    watch::tick(&mut client, &s, "2026-11-24 08:00:00+00").await.unwrap();
    let lines = "SELECT count(*) FROM core.notice WHERE channel = 'progress' AND message LIKE '💵 Spend%'";
    assert_eq!(client.query_one(lines, &[]).await.unwrap().get::<_, i64>(0), 0, "not before 09:00 MT");
    reading(&client, "2026-11-24 14:50:00+00", "277.00", "2026-11-24T14:30:00Z", json!([])).await;
    watch::tick(&mut client, &s, "2026-11-24 15:01:00+00").await.unwrap();
    watch::tick(&mut client, &s, "2026-11-24 15:02:00+00").await.unwrap();
    // C-091 (owner, 2026-11-26): no spend line on the progress channel any more; the spend near the ceiling is
    // SPEND, a spend not known is the digest's "Needs you"
    assert_eq!(client.query_one(lines, &[]).await.unwrap().get::<_, i64>(0), 0, "no progress line");
    assert!(open(&client).await.is_empty(), "18% alerts nothing");

    // a lower ceiling: 80% alerts; past it, 100% too; both kept while the figures cannot be read
    let s = settings(watch_of(110.0));
    watch::check(&mut client, &s, "2026-11-24 15:03:00+00").await.unwrap();
    let alerts = open(&client).await;
    assert_eq!(alerts.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["80%"]);
    assert!(alerts[0].1.starts_with("Spend since Nov 21 is $97."), "{}", alerts[0].1);
    assert!(alerts[0].1.contains("88% of the $110.00 ceiling; $"), "{}", alerts[0].1);
    let s = settings(watch_of(90.0));
    watch::check(&mut client, &s, "2026-11-24 15:04:00+00").await.unwrap();
    assert_eq!(open(&client).await.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["100%", "80%"]);
    let posted: String = client.query_one("SELECT message FROM core.notice WHERE channel = 'alerts' AND post AND message LIKE '🔴 Needs you: Spend%past%'", &[]).await.unwrap().get(0);
    // C-091: the owner's own action, one line, no command
    assert!(posted.contains("past the $90.00 ceiling") && posted.ends_with(": decide whether to go on: raise the ceiling, or stop the workers.") && !posted.contains('`'), "{posted}");
    spend::put_reading(&client, Err("GET /v2/customers/my/balance: DigitalOcean answered 401 Unauthorized")).await.unwrap();
    client.execute("UPDATE core.spend_reading SET read_at = '2026-11-24 15:30:00+00' WHERE error IS NOT NULL AND read_at > '2026-11-24'", &[]).await.unwrap();
    watch::check(&mut client, &s, "2026-11-24 15:31:00+00").await.unwrap();
    assert_eq!(open(&client).await.len(), 2, "unread, nothing resolves");

    // December: November's invoice not issued yet, its highest figure stands in; then the invoice
    reading(&client, "2026-12-01 03:00:00+00", "4.10", "2026-12-01T02:30:00Z", json!([])).await;
    let Spend::Known(k) = spend::spend(&client, &w, "2026-12-01 03:10:00+00").await.unwrap() else { panic!("known") };
    assert!((k.spent - (277.00 - 180.41 + 4.10)).abs() < 1e-9, "{}", k.spent);
    reading(&client, "2026-12-01 04:00:00+00", "4.60", "2026-12-01T03:30:00Z", json!([{"invoice_uuid": "x", "amount": "301.17", "invoice_period": "2026-11"}])).await;
    let Spend::Known(k) = spend::spend(&client, &w, "2026-12-01 04:10:00+00").await.unwrap() else { panic!("known") };
    assert!((k.spent - (301.17 - 180.41 + 4.60)).abs() < 1e-9, "{}", k.spent);
    scratch.drop().await;
}
