//! The universe for the 64 from real Intrinio responses (`fixtures/intrinio-universe`, fetched
//! 2026-11-11) and the engine's profile at engine-v1. Run with `cargo test -- --ignored`.
mod common;
use common::Scratch;
use core_db::{migrate, universe};
use std::path::PathBuf;
use tokio_postgres::Client;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn document() -> serde_json::Value {
    universe::read_responses(&fixtures().join("engine-v1-PROFILE.json"), &fixtures().join("intrinio-universe")).unwrap()
}

/// (asset key, ticker) of the profile's members on a day, in order.
async fn members_on(client: &Client, day: &str) -> Vec<(String, Option<String>)> {
    client
        .query("SELECT asset_key, ticker FROM core.members_on('FREE_REPORT_64', $1::text::date)", &[&day])
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

fn ticker_of<'a>(members: &'a [(String, Option<String>)], key: &str) -> Option<&'a str> {
    members.iter().find(|(k, _)| k == key).unwrap().1.as_deref()
}

#[test]
fn the_responses_read_into_64_members_and_their_records() {
    let doc = document();
    assert_eq!(doc["profile_id"], "FREE_REPORT_64");
    let members = doc["members"].as_array().unwrap();
    assert_eq!(members.len(), 64);
    assert_eq!(members[0]["asset_key"], "SPY");
    assert_eq!(members.iter().filter(|m| m["company"] == true).count(), 17);
    let securities = doc["securities"].as_array().unwrap();
    let by_id = |id: &str| securities.iter().find(|s| s["security_id"] == id).unwrap();
    // Meta today, its FB years, and the unrelated ETFs that have used both tickers
    assert_eq!(by_id("sec_XO82JO")["ticker"], "META");
    assert_eq!(by_id("sec_XO82JO")["share_class_figi"], "BBG001SQCQC5");
    assert_eq!(by_id("sec_gQ8B8y")["chain_of"], "sec_XO82JO");
    assert_eq!(by_id("sec_gQ8B8y")["last_price_date"], "2022-06-08");
    assert!(by_id("sec_ge1K5q")["chain_of"].is_null());
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn each_member_has_its_ticker_on_a_date() {
    let scratch = Scratch::new("universe").await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    let doc = document();
    let loaded = universe::store(&mut client, &doc).await.unwrap();
    assert_eq!(loaded.members, 64);
    // 64 members' own spans, plus FB's and QQQQ's years, which split QQQ's own range in two
    assert_eq!(loaded.spans, 67);
    // loading again replaces, it does not add
    assert_eq!(universe::store(&mut client, &doc).await.unwrap(), loaded);

    // today every member trades under its own key
    let today = members_on(&client, "2026-11-10").await;
    assert_eq!(today.len(), 64);
    assert!(today.iter().all(|(key, ticker)| ticker.as_deref() == Some(key.as_str())));
    assert_eq!(today[0].0, "SPY");

    // Meta traded as FB to 2022-06-08
    assert_eq!(ticker_of(&members_on(&client, "2022-06-08").await, "META"), Some("FB"));
    assert_eq!(ticker_of(&members_on(&client, "2022-06-09").await, "META"), Some("META"));
    assert_eq!(ticker_of(&members_on(&client, "2021-09-27").await, "META"), Some("FB"));
    // IBIT and ETHA are not listed before 2024-01-11 and 2024-07-23
    let early = members_on(&client, "2024-01-10").await;
    assert_eq!(ticker_of(&early, "IBIT"), None);
    assert_eq!(early.iter().filter(|(_, t)| t.is_none()).map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["IBIT", "ETHA"]);
    assert_eq!(ticker_of(&members_on(&client, "2024-01-11").await, "IBIT"), Some("IBIT"));
    assert_eq!(ticker_of(&members_on(&client, "2024-07-22").await, "ETHA"), None);
    assert_eq!(ticker_of(&members_on(&client, "2024-07-23").await, "ETHA"), Some("ETHA"));
    // the plan's claim: from 2024-07-23 the 64 are all listed under today's tickers
    let changes: i64 = client
        .query_one(
            "SELECT count(*) FROM core.ticker_span t JOIN core.member m USING (security_id)
             WHERE lower(t.during) > '2024-07-23' OR (upper(t.during) IS NOT NULL AND upper(t.during) > '2024-07-23')",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(changes, 0);
    // the history listings are empty for DBA, SLV and UUP: their own record still gives a span
    assert_eq!(ticker_of(&members_on(&client, "2021-09-27").await, "DBA"), Some("DBA"));

    // other securities have used members' tickers inside the option history: the ticker alone does not name the asset
    let reused: Vec<(String, String)> = client
        .query(
            "SELECT s.ticker, s.security_id FROM core.security s JOIN core.member m ON m.asset_key = s.ticker AND m.security_id <> s.security_id
             WHERE s.chain_of IS NULL AND s.last_price_date >= (SELECT first_date FROM core.source_coverage WHERE source = 'option_eod')
             ORDER BY 1, 2",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(reused, [("IBIT".to_owned(), "sec_zrB4o8".to_owned()), ("META".to_owned(), "sec_ge1K5q".to_owned())]);
    scratch.drop().await;
}
