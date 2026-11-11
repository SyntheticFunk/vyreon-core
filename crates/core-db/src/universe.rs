//! The point-in-time universe: who a profile's members are and which ticker each traded under on
//! a date (C-017). Built from saved Intrinio responses and the engine's profile file:
//!
//!   <dir>/manifest.json                          {"fetched_at": "..."}
//!   <dir>/security/<TICKER>.json                 /securities/{ticker}
//!   <dir>/history-by-identifier/<id>.json        /securities/history-by-identifier/{id}
//!   <dir>/history-by-ticker/<TICKER>.json        /securities/history-by-ticker/{ticker}
//!
//! The provider's records are stored as given; the ticker spans are derived in the database
//! (migration 0004 explains the provider's shapes).
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use tokio_postgres::Client;

/// What a load stored.
#[derive(Debug, PartialEq, Eq)]
pub struct Loaded {
    pub members: u64,
    pub securities: u64,
    pub spans: u64,
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn text<'a>(v: &'a Value, key: &str, at: &str) -> Result<&'a str, String> {
    v[key].as_str().ok_or_else(|| format!("{at}: no \"{key}\""))
}

fn texts(v: &Value, key: &str, at: &str) -> Result<Vec<String>, String> {
    let list = v[key].as_array().ok_or_else(|| format!("{at}: no \"{key}\""))?;
    list.iter().map(|x| x.as_str().map(str::to_owned).ok_or_else(|| format!("{at}: \"{key}\" holds a non-string"))).collect()
}

/// A history listing's records.
fn listing(path: &Path) -> Result<Vec<Value>, String> {
    read(path)?["securities"].as_array().cloned().ok_or_else(|| format!("{}: no \"securities\"", path.display()))
}

/// One provider record as a core.security row. `chain_of` is set on a member's earlier tickers.
fn row(record: &Value, chain_of: Option<&str>, fetched_at: &str, at: &str) -> Result<Value, String> {
    let field = |key: &str| record.get(key).cloned().unwrap_or(Value::Null);
    Ok(json!({
        "security_id": text(record, "id", at)?,
        "ticker": text(record, "ticker", at)?,
        "name": field("name"),
        // the security record says "code", "first_stock_price"; the history listings say
        // "security_code", "first_price_date"
        "code": record.get("code").or(record.get("security_code")).cloned().unwrap_or(Value::Null),
        "company_id": field("company_id"),
        "cik": field("cik"),
        "figi": field("figi"),
        "composite_figi": field("composite_figi"),
        "share_class_figi": field("share_class_figi"),
        "first_price_date": record.get("first_stock_price").or(record.get("first_price_date")).cloned().unwrap_or(Value::Null),
        "last_price_date": record.get("last_stock_price").or(record.get("last_price_date")).cloned().unwrap_or(Value::Null),
        "merged_into": field("merged_security_id"),
        "delisted": field("delisted"),
        "chain_of": chain_of,
        "fetched_at": fetched_at,
    }))
}

/// Reads the engine's profile file and the saved responses into one document:
/// `{"profile_id", "members": [...], "securities": [...]}`.
pub fn read_responses(profile: &Path, dir: &Path) -> Result<Value, String> {
    let p = read(profile)?;
    let at = profile.display().to_string();
    let profile_id = text(&p, "profile_id", &at)?;
    let ordered = texts(&p, "ordered_security_ids", &at)?;
    let companies = texts(&p, "company_security_ids", &at)?;
    let mut group_of = BTreeMap::new();
    for g in p["groups"].as_array().ok_or_else(|| format!("{at}: no \"groups\""))? {
        for member in texts(g, "members", &at)? {
            if group_of.insert(member.clone(), text(g, "group", &at)?.to_owned()).is_some() {
                return Err(format!("{at}: {member} is in two groups"));
            }
        }
    }
    let manifest = read(&dir.join("manifest.json"))?;
    let fetched_at = text(&manifest, "fetched_at", "manifest.json")?;

    // by id; a member's own record wins over a listing's copy of it, a chain record over a ticker reuse
    let mut securities: BTreeMap<String, (u8, Value)> = BTreeMap::new();
    let mut put = |rank: u8, v: Value| {
        let id = v["security_id"].as_str().unwrap_or_default().to_owned();
        if securities.get(&id).is_none_or(|(held, _)| rank < *held) {
            securities.insert(id, (rank, v));
        }
    };
    let mut members = Vec::new();
    for (i, key) in ordered.iter().enumerate() {
        let path = dir.join("security").join(format!("{key}.json"));
        let own = read(&path)?;
        let at = path.display().to_string();
        let id = text(&own, "id", &at)?.to_owned();
        if text(&own, "ticker", &at)? != key {
            return Err(format!("{at}: the record's ticker is {}, not {key}", own["ticker"]));
        }
        put(0, row(&own, None, fetched_at, &at)?);
        let path = dir.join("history-by-identifier").join(format!("{id}.json"));
        for record in listing(&path)? {
            if record["id"].as_str() != Some(id.as_str()) {
                put(1, row(&record, Some(&id), fetched_at, &path.display().to_string())?);
            }
        }
        let path = dir.join("history-by-ticker").join(format!("{key}.json"));
        for record in listing(&path)? {
            put(2, row(&record, None, fetched_at, &path.display().to_string())?);
        }
        let group = group_of.get(key).ok_or_else(|| format!("{}: {key} is in no group", profile.display()))?;
        members.push(json!({"asset_key": key, "ordinal": i as i64 + 1, "security_id": id, "group_name": group, "company": companies.contains(key)}));
    }
    let mut doc = Map::new();
    doc.insert("profile_id".into(), json!(profile_id));
    doc.insert("members".into(), Value::Array(members));
    doc.insert("securities".into(), Value::Array(securities.into_values().map(|(_, v)| v).collect()));
    Ok(Value::Object(doc))
}

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| format!("{} ({})", d.message(), d.detail().unwrap_or("no detail")))
}

/// Stores a document from `read_responses` in one transaction: the provider records are upserted,
/// the profile's member list is replaced, and the members' ticker spans are derived again. A
/// member without a dated listing is refused.
pub async fn store(client: &mut Client, doc: &Value) -> Result<Loaded, String> {
    let profile = doc["profile_id"].as_str().ok_or("no profile_id")?.to_owned();
    let text = doc.to_string();
    let tx = client.transaction().await.map_err(db)?;
    let securities = tx
        .execute(
            "INSERT INTO core.security (security_id, ticker, name, code, company_id, cik, figi, composite_figi, share_class_figi,
                 first_price_date, last_price_date, merged_into, delisted, chain_of, fetched_at)
             SELECT security_id, ticker, name, code, company_id, cik, figi, composite_figi, share_class_figi,
                    first_price_date, last_price_date, merged_into, delisted, chain_of, fetched_at
             FROM jsonb_to_recordset($1::text::jsonb->'securities')
                 AS s(security_id text, ticker text, name text, code text, company_id text, cik text, figi text, composite_figi text,
                      share_class_figi text, first_price_date date, last_price_date date, merged_into text, delisted boolean,
                      chain_of text, fetched_at timestamptz)
             ON CONFLICT (security_id) DO UPDATE SET
                 (ticker, name, code, company_id, cik, figi, composite_figi, share_class_figi, first_price_date, last_price_date,
                  merged_into, delisted, chain_of, fetched_at)
               = (EXCLUDED.ticker, EXCLUDED.name, EXCLUDED.code, EXCLUDED.company_id, EXCLUDED.cik, EXCLUDED.figi, EXCLUDED.composite_figi,
                  EXCLUDED.share_class_figi, EXCLUDED.first_price_date, EXCLUDED.last_price_date, EXCLUDED.merged_into, EXCLUDED.delisted,
                  EXCLUDED.chain_of, EXCLUDED.fetched_at)",
            &[&text],
        )
        .await
        .map_err(db)?;
    tx.execute("DELETE FROM core.ticker_span WHERE security_id IN (SELECT security_id FROM core.member WHERE profile_id = $1)", &[&profile]).await.map_err(db)?;
    tx.execute("DELETE FROM core.member WHERE profile_id = $1", &[&profile]).await.map_err(db)?;
    let members = tx
        .execute(
            "INSERT INTO core.member (profile_id, asset_key, ordinal, security_id, group_name, company)
             SELECT $2, asset_key, ordinal, security_id, group_name, company
             FROM jsonb_to_recordset($1::text::jsonb->'members')
                 AS m(asset_key text, ordinal integer, security_id text, group_name text, company boolean)",
            &[&text, &profile],
        )
        .await
        .map_err(db)?;
    let spans = tx
        .execute(
            "WITH own AS (
                 SELECT s.security_id, s.ticker, daterange(s.first_price_date, CASE WHEN s.delisted THEN s.last_price_date + 1 END) AS r
                 FROM core.security s JOIN core.member m USING (security_id)
                 WHERE m.profile_id = $1 AND s.first_price_date IS NOT NULL),
             earlier AS (
                 SELECT c.chain_of AS security_id, c.security_id AS record_id, c.ticker,
                        daterange(c.first_price_date, c.last_price_date, '[]') AS r
                 FROM core.security c JOIN core.member m ON m.security_id = c.chain_of
                 WHERE m.profile_id = $1 AND c.first_price_date IS NOT NULL AND c.last_price_date IS NOT NULL)
             INSERT INTO core.ticker_span (security_id, ticker, record_id, during)
             SELECT security_id, ticker, record_id, r FROM earlier
             UNION ALL
             SELECT o.security_id, o.ticker, o.security_id, u.r
             FROM own o,
                  unnest(datemultirange(o.r) - COALESCE((SELECT range_agg(e.r) FROM earlier e WHERE e.security_id = o.security_id),
                                                        '{}'::datemultirange)) AS u(r)",
            &[&profile],
        )
        .await
        .map_err(db)?;
    let unlisted: Vec<String> = tx
        .query(
            "SELECT asset_key FROM core.member m WHERE profile_id = $1
                 AND NOT EXISTS (SELECT FROM core.ticker_span t WHERE t.security_id = m.security_id) ORDER BY ordinal",
            &[&profile],
        )
        .await
        .map_err(db)?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if !unlisted.is_empty() {
        return Err(format!("members without a dated listing: {}", unlisted.join(" ")));
    }
    tx.commit().await.map_err(db)?;
    Ok(Loaded { members, securities, spans })
}
