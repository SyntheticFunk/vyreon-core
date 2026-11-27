//! C-078 (owner, 2026-11-24: automation debt 3): the spend against the owner's ceiling, from
//! DigitalOcean's own billing figures. Core reads them every hour with a read-only token
//! (`core_api::spend`: `/v2/customers/my/balance`'s month-to-date usage and `/v2/customers/my/invoices`'
//! finished months) and keeps each reading (`core.spend_reading`, 0060). The spend since the owner's start
//! (`VYREON_CORE_SPEND_SINCE`) is every month from the start's: a finished month's invoice (or, before it
//! is issued, the highest month-to-date figure core read in that month), the current month's
//! month-to-date figure, less the start month's usage before the start (`VYREON_CORE_SPEND_BASELINE_USD`,
//! the month-to-date figure the owner read at the start). It is the whole account's (the standing core
//! node, database volume, registry, Spaces, the engine droplet and the web server as well as the worker
//! nodes): what the owner's running total has counted since 2026-11-21.
//!
//! Each morning (with the digest, 09:00 MT) one line goes to the progress channel: the spend so far, its
//! rate per day and where that rate reaches the ceiling (`line`). The watch's `SPEND` problem alerts at
//! 80% and at 100% of the ceiling (`VYREON_CORE_SPEND_CEILING_USD`). Without a reading (no token given,
//! a refusal) the line says so plainly and no problem opens or resolves.
use crate::watch::{Found, Open, ZONE, one_line};
use serde_json::Value;
use tokio_postgres::GenericClient;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// What the spend watch is told (`VYREON_CORE_SPEND_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpendWatch {
    /// The start of the spend counted (RFC 3339, or a date: its midnight UTC).
    pub since: String,
    /// The start month's usage before `since` (DigitalOcean's month-to-date figure at the start).
    pub baseline_usd: f64,
    /// The owner's ceiling for the spend since `since`.
    pub ceiling_usd: f64,
}

/// One reading of DigitalOcean's figures.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The month `month_to_date_usd` is of, 'YYYY-MM' (its `generated_at`, UTC).
    pub month: String,
    pub month_to_date_usd: f64,
    pub generated_at: Option<String>,
    /// The finished months' invoices: ('YYYY-MM', USD).
    pub invoices: Vec<(String, f64)>,
}

/// The share of the ceiling at which `SPEND` alerts first.
pub const WARN_SHARE: f64 = 0.8;
/// A reading older than this is no evidence of the spend now (core reads every hour).
pub const STALE_HOURS: f64 = 3.0;
/// DigitalOcean's figure (`generated_at`) older than this is no evidence either: it updates its usage a
/// few times a day, so a day without a new figure is DigitalOcean's own gap (release-3 audit, Codex 7).
pub const FIGURE_STALE_HOURS: f64 = 24.0;
/// Readings are kept this many days (a month's highest figure stands in for its invoice until it is issued).
pub const KEEP_DAYS: f64 = 120.0;

fn usd(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
    .filter(|x: &f64| x.is_finite())
}

/// A reading from DigitalOcean's two answers (`balance`: `/v2/customers/my/balance`; `invoices`:
/// `/v2/customers/my/invoices`). `month_now` ('YYYY-MM', UTC) is the month when the answer states no
/// `generated_at`. An answer without the month-to-date figure is no reading.
pub fn from_answers(balance: &Value, invoices: &Value, month_now: &str) -> Result<Reading, String> {
    let month_to_date_usd = usd(&balance["month_to_date_usage"]).ok_or("DigitalOcean's balance has no month_to_date_usage")?;
    let generated_at = balance["generated_at"].as_str().map(str::to_owned);
    let month = generated_at.as_deref().filter(|g| g.len() >= 7 && g.as_bytes()[4] == b'-').map_or(month_now, |g| &g[..7]).to_owned();
    // (release-3 audit, Gemini 20) an invoice whose month or amount cannot be read makes no reading, never a
    // month counted short
    let invoices = invoices["invoices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| match (i["invoice_period"].as_str().filter(|p| p.len() == 7), usd(&i["amount"])) {
            (Some(p), Some(u)) => Ok((p.to_owned(), u)),
            _ => Err("DigitalOcean's invoices hold one whose invoice_period or amount cannot be read".to_owned()),
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Reading { month, month_to_date_usd, generated_at, invoices })
}

/// Keeps a reading, or why there was none (one row a read; rows older than `KEEP_DAYS` go).
pub async fn put_reading(c: &impl GenericClient, read: Result<&Reading, &str>) -> Result<(), String> {
    match read {
        Ok(r) => {
            let invoices: Value = r.invoices.iter().map(|(p, u)| serde_json::json!({"period": p, "usd": u})).collect();
            c.execute(
                "INSERT INTO core.spend_reading (month, month_to_date_usd, generated_at, invoices) VALUES ($1, $2, $3::text::timestamptz, $4::text::jsonb)",
                &[&r.month, &r.month_to_date_usd, &r.generated_at, &invoices.to_string()],
            )
            .await
            .map_err(db)?;
        }
        Err(e) => {
            c.execute("INSERT INTO core.spend_reading (error) VALUES ($1)", &[&one_line(e, 300)]).await.map_err(db)?;
        }
    }
    c.execute("DELETE FROM core.spend_reading WHERE read_at < now() - make_interval(secs => $1::float8 * 86400)", &[&KEEP_DAYS]).await.map_err(db)?;
    Ok(())
}

/// The spend, as known at `now`.
#[derive(Debug, Clone, PartialEq)]
pub enum Spend {
    /// No usable reading, and why (said plainly in the line; no problem opens or resolves).
    Unread(String),
    Known(Known),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    /// USD since the start, as of DigitalOcean's figure (`asof`).
    pub spent: f64,
    /// USD since the start now: `spent` and, at `rate`, the time since DigitalOcean's figure (its usage
    /// is updated a few times a day: release-3 audit, Codex 7); what the alerts and the recompute gate use.
    pub now: f64,
    pub ceiling: f64,
    /// The start's date, UTC ("Nov 21"; the months counted are UTC months).
    pub since: String,
    /// DigitalOcean's figure's time in the owner's zone ("Nov 24 00:30").
    pub asof: String,
    /// The same, as a timestamp (UTC text): work finished after it is not billed in `spent` yet.
    pub asof_at: String,
    /// C-100: how old DigitalOcean's figure is at the time the line is said, in hours.
    pub figure_hours: f64,
    /// USD a day, and over what ("over the last 24 h", "since Nov 21"); None before a rate can be told.
    pub rate: Option<(f64, String)>,
    /// At `rate`, when the ceiling is reached: (its date in the owner's zone, days from now); None
    /// without a rate or once passed.
    pub reached: Option<(String, f64)>,
}

/// The spend since `w.since` as of reading `id`: None when a finished month's total is not known.
async fn spent_at(c: &impl GenericClient, w: &SpendWatch, id: i64) -> Result<Result<f64, String>, String> {
    let rows = c
        .query(
            "WITH r AS (SELECT * FROM core.spend_reading WHERE reading_id = $1),
                  m AS (SELECT to_char(g, 'YYYY-MM') AS month
                        FROM r, generate_series(date_trunc('month', $2::text::timestamptz AT TIME ZONE 'UTC'), to_date(r.month, 'YYYY-MM'), interval '1 month') g)
             SELECT m.month,
                    CASE WHEN m.month = r.month THEN r.month_to_date_usd
                         -- (release-3 audit, Gemini 20) every invoice of the month, summed
                         ELSE COALESCE((SELECT sum((i->>'usd')::float8) FROM jsonb_array_elements(r.invoices) i WHERE i->>'period' = m.month),
                                       (SELECT max(o.month_to_date_usd) FROM core.spend_reading o
                                        WHERE o.month = m.month AND o.error IS NULL AND o.read_at <= r.read_at)) END
             FROM m, r ORDER BY 1",
            &[&id, &w.since],
        )
        .await
        .map_err(db)?;
    let mut total = -w.baseline_usd;
    for r in &rows {
        match r.get::<_, Option<f64>>(1) {
            Some(u) => total += u,
            None => return Ok(Err(format!("{}'s total is not known yet (no invoice, and no reading in that month)", r.get::<_, String>(0)))),
        }
    }
    if rows.is_empty() {
        return Ok(Err("the newest reading is of a month before the start".into()));
    }
    Ok(Ok(total))
}

/// A reading's time: DigitalOcean's `generated_at`, else when core read it.
const AT: &str = "COALESCE(generated_at, read_at)";

/// The spend at `now` from the readings core keeps.
pub async fn spend(c: &impl GenericClient, w: &SpendWatch, now: &str) -> Result<Spend, String> {
    let newest = c
        .query_opt(
            &format!(
                "SELECT reading_id, error, extract(epoch FROM $1::text::timestamptz - read_at)::float8 / 3600,
                        extract(epoch FROM $1::text::timestamptz - {AT})::float8 / 3600
                 FROM core.spend_reading WHERE read_at <= $1::text::timestamptz ORDER BY read_at DESC, reading_id DESC LIMIT 1"
            ),
            &[&now],
        )
        .await
        .map_err(db)?;
    let Some(newest) = newest else {
        return Ok(Spend::Unread("core has not read DigitalOcean's billing yet".into()));
    };
    if let Some(e) = newest.get::<_, Option<String>>(1) {
        return Ok(Spend::Unread(e));
    }
    let (id, read_hours, figure_hours): (i64, f64, f64) = (newest.get(0), newest.get(2), newest.get(3));
    if read_hours > STALE_HOURS {
        return Ok(Spend::Unread(format!("core's newest reading is {} old", crate::watch::age((read_hours * 3600.0) as i64))));
    }
    // (release-3 audit, Codex 7) DigitalOcean's figure itself, read again each hour, is no fresher for it
    if figure_hours > FIGURE_STALE_HOURS {
        return Ok(Spend::Unread(format!("DigitalOcean's newest figure is {} old", crate::watch::age((figure_hours * 3600.0) as i64))));
    }
    let spent = match spent_at(c, w, id).await? {
        Ok(s) => s,
        Err(why) => return Ok(Spend::Unread(why)),
    };
    // (release-3 audit, Codex 14) the rate over the figure nearest to 24 h before this one (by DigitalOcean's
    // times, 20 to 48 h before), else since the start
    let row = c
        .query_one(
            &format!(
                "WITH r AS (SELECT *, {AT} AS at FROM core.spend_reading WHERE reading_id = $1),
                      e AS (SELECT o.reading_id, extract(epoch FROM r.at - {o_at})::float8 / 86400 AS days
                            FROM core.spend_reading o, r
                            WHERE o.error IS NULL AND {o_at} <= r.at - interval '20 hours' AND {o_at} >= r.at - interval '48 hours'
                              AND {o_at} >= $2::text::timestamptz
                            ORDER BY abs(extract(epoch FROM (r.at - interval '24 hours') - {o_at})), o.reading_id DESC LIMIT 1)
                 SELECT to_char($2::text::timestamptz AT TIME ZONE 'UTC', 'Mon FMDD'), to_char(r.at AT TIME ZONE $3, 'Mon FMDD HH24:MI'),
                        extract(epoch FROM r.at - $2::text::timestamptz)::float8 / 86400, (SELECT reading_id FROM e), (SELECT days FROM e),
                        extract(epoch FROM $4::text::timestamptz - r.at)::float8 / 86400, r.at::text
                 FROM r",
                o_at = "COALESCE(o.generated_at, o.read_at)"
            ),
            &[&id, &w.since, &ZONE, &now],
        )
        .await
        .map_err(db)?;
    let (since, asof, days_since, earlier, earlier_days, lag_days, asof_at): (String, String, f64, Option<i64>, Option<f64>, f64, String) =
        (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4), row.get(5), row.get(6));
    let rate = match (earlier, earlier_days) {
        (Some(e), Some(d)) if d > 0.0 => match spent_at(c, w, e).await? {
            Ok(before) => Some(((spent - before) / d, format!("over the last {:.0} h", d * 24.0))),
            Err(_) => None,
        },
        _ => None,
    }
    .or_else(|| (days_since >= 1.0 / 24.0).then(|| (spent / days_since, format!("since {since}"))));
    let now_spent = spent + rate.as_ref().map_or(0.0, |(r, _)| r.max(0.0) * lag_days.max(0.0));
    let reached = match rate {
        Some((r, _)) if r > 0.0 && now_spent < w.ceiling_usd => {
            let days = (w.ceiling_usd - now_spent) / r;
            let date: String = c
                .query_one(
                    "SELECT to_char(($1::text::timestamptz + make_interval(secs => $2::float8 * 86400)) AT TIME ZONE $3, 'Mon FMDD')",
                    &[&now, &days.min(3650.0), &ZONE],
                )
                .await
                .map_err(db)?
                .get(0);
            Some((date, days))
        }
        _ => None,
    };
    Ok(Spend::Known(Known { spent, now: now_spent, ceiling: w.ceiling_usd, since, asof, asof_at, figure_hours, rate, reached }))
}

/// "$1,234.56".
pub fn dollars(x: f64) -> String {
    let cents = (x.abs() * 100.0).round() as i64;
    format!("{}${}.{:02}", if x < 0.0 && cents > 0 { "-" } else { "" }, crate::progress::thousands(cents / 100), cents % 100)
}

/// C-100: DigitalOcean's figure older than this has its age said beside it (it refreshes about once a day).
pub const FIGURE_AGE_SAID_HOURS: f64 = 3.0;

/// The morning's line for the progress channel.
pub fn line(s: &Spend) -> String {
    match s {
        Spend::Unread(why) => format!("💵 Spend: not known ({}); the ceiling is not watched meanwhile", one_line(why, 200)),
        Spend::Known(k) => {
            let share = if k.ceiling > 0.0 { 100.0 * k.now / k.ceiling } else { 0.0 };
            let lagged = (k.now - k.spent).abs() >= 0.005;
            let mut out = format!(
                "💵 Spend since {}: {}{} of the {} ceiling ({share:.0}%)",
                k.since,
                if lagged { "≈" } else { "" },
                dollars(k.now),
                dollars(k.ceiling)
            );
            // C-100 (the line of 2026-11-27: "-$1.62 a day over the last 23 h", DigitalOcean's figure 13 h old and
            // its month-to-date lower than the reading before): the figure's age is said once it is over
            // `FIGURE_AGE_SAID_HOURS`, and a rate below nothing is not a rate
            let old = if k.figure_hours > FIGURE_AGE_SAID_HOURS { format!(", {:.0} h old", k.figure_hours) } else { String::new() };
            if lagged {
                out.push_str(&format!(" ({} at DigitalOcean's figure of {} MT{old}, then at the rate)", dollars(k.spent), k.asof));
            }
            match &k.rate {
                Some((r, _)) if *r < 0.0 => out.push_str("; rate not known (DigitalOcean's figure fell between its readings)"),
                Some((r, over)) => out.push_str(&format!("; {} a day {over}", dollars(*r))),
                None => {}
            }
            match &k.reached {
                Some((date, days)) if *days < 365.0 => out.push_str(&format!("; at that rate the ceiling is reached about {date} (in {days:.0} days)")),
                Some(_) => out.push_str("; at that rate the ceiling is more than a year away"),
                None if k.now >= k.ceiling => out.push_str(&format!("; {} past the ceiling", dollars(k.now - k.ceiling))),
                None => {}
            }
            if !lagged {
                out.push_str(&format!(" (DigitalOcean's figures, whole account, of {} MT{old})", k.asof));
            }
            out
        }
    }
}

/// The watch's `SPEND` problems: 80% and 100% of the ceiling, each its own (key "80%", "100%"). Unread,
/// the open ones stay as they are.
pub async fn found(c: &impl GenericClient, w: &SpendWatch, now: &str, open: &[Open]) -> Result<Vec<Found>, String> {
    let s = spend(c, w, now).await?;
    let Spend::Known(k) = &s else {
        return Ok(open
            .iter()
            .filter(|o| o.kind == "SPEND")
            .map(|o| Found { kind: "SPEND", key: o.key.clone(), summary: o.summary.clone(), command: o.command.clone(), detail: None })
            .collect());
    };
    let pace = match (&k.rate, &k.reached) {
        (Some((r, _)), Some((date, days))) if *days < 365.0 => format!("; {} a day, the ceiling about {date}", dollars(*r)),
        (Some((r, _)), _) => format!("; {} a day", dollars(*r)),
        _ => String::new(),
    };
    let mut out = Vec::new();
    if k.now >= WARN_SHARE * k.ceiling {
        out.push(Found {
            kind: "SPEND",
            key: "80%".into(),
            summary: format!("Spend since {} is {}, {:.0}% of the {} ceiling{pace}", k.since, dollars(k.now), 100.0 * k.now / k.ceiling, dollars(k.ceiling)),
            command: "vyreon-core spend".into(),
            detail: None,
        });
    }
    if k.now >= k.ceiling {
        out.push(Found {
            kind: "SPEND",
            key: "100%".into(),
            summary: format!("Spend since {} is {}: past the {} ceiling{pace}", k.since, dollars(k.now), dollars(k.ceiling)),
            command: "vyreon-core spend".into(),
            detail: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// DigitalOcean's answers as its API documents them (amounts as strings; `generated_at` UTC).
    #[test]
    fn a_reading_is_read_from_digitaloceans_answers() {
        let balance = json!({"month_to_date_balance": "269.04", "account_balance": "0.00", "month_to_date_usage": "269.04", "generated_at": "2026-11-24T06:42:00Z"});
        let invoices = json!({"invoices": [{"invoice_uuid": "x", "amount": "212.50", "invoice_period": "2026-10", "updated_at": "2026-11-01T08:00:00Z"}],
                              "invoice_preview": {"amount": "269.04", "invoice_period": "2026-11"}});
        let r = from_answers(&balance, &invoices, "2026-12").unwrap();
        assert_eq!(r, Reading { month: "2026-11".into(), month_to_date_usd: 269.04, generated_at: Some("2026-11-24T06:42:00Z".into()), invoices: vec![("2026-10".into(), 212.5)] });
        assert!(from_answers(&json!({"account_balance": "1"}), &json!({}), "2026-11").unwrap_err().contains("month_to_date_usage"));
        assert_eq!(from_answers(&json!({"month_to_date_usage": 3.5}), &json!({}), "2026-12").unwrap().month, "2026-12");
    }

    #[test]
    fn the_line_says_the_spend_its_rate_and_where_it_leads() {
        let k = Known {
            spent: 123.4,
            now: 123.4,
            ceiling: 500.0,
            since: "Nov 21".into(),
            asof: "Nov 24 08:12".into(),
            asof_at: "2026-11-24 15:12:00+00".into(),
            figure_hours: 0.8,
            rate: Some((30.0, "over the last 24 h".into())),
            reached: Some(("Dec 7".into(), 12.6)),
        };
        assert_eq!(
            line(&Spend::Known(k.clone())),
            "💵 Spend since Nov 21: $123.40 of the $500.00 ceiling (25%); $30.00 a day over the last 24 h; at that rate the ceiling is reached about Dec 7 (in 13 days) (DigitalOcean's figures, whole account, of Nov 24 08:12 MT)"
        );
        // DigitalOcean's figure hours old: the spend now at the rate since, said as such
        let lagged = Known { now: 138.4, ..k.clone() };
        assert!(line(&Spend::Known(lagged)).starts_with("💵 Spend since Nov 21: ≈$138.40 of the $500.00 ceiling (28%) ($123.40 at DigitalOcean's figure of Nov 24 08:12 MT, then at the rate)"));
        // C-100: the line of 2026-11-27 (DigitalOcean's figure of Nov 26 21:43 MT, read hourly since; its
        // month-to-date had fallen from 384.41 to 376.38): it read "-$1.62 a day over the last 23 h"
        let real = Known {
            spent: 195.97,
            now: 195.97,
            ceiling: 750.0,
            asof: "Nov 26 21:43".into(),
            asof_at: "2026-11-27 03:43:30+00".into(),
            figure_hours: 12.9,
            rate: Some((-1.62, "over the last 23 h".into())),
            reached: None,
            ..k.clone()
        };
        assert_eq!(
            line(&Spend::Known(real)),
            "💵 Spend since Nov 21: $195.97 of the $750.00 ceiling (26%); rate not known (DigitalOcean's figure fell between its readings) (DigitalOcean's figures, whole account, of Nov 26 21:43 MT, 13 h old)"
        );
        let lagged_old = Known { now: 138.4, figure_hours: 5.2, ..k.clone() };
        assert!(line(&Spend::Known(lagged_old)).contains("($123.40 at DigitalOcean's figure of Nov 24 08:12 MT, 5 h old, then at the rate)"));
        let past = Known { spent: 512.0, now: 512.0, rate: None, reached: None, ..k };
        assert!(line(&Spend::Known(past)).contains("(102%); $12.00 past the ceiling"));
        assert_eq!(
            line(&Spend::Unread("no DigitalOcean token: the secret vyreon-core-do-billing is not made".into())),
            "💵 Spend: not known (no DigitalOcean token: the secret vyreon-core-do-billing is not made); the ceiling is not watched meanwhile"
        );
        // (release-3 audit, Gemini 20) an invoice that cannot be read makes no reading
        let bad = json!({"invoices": [{"amount": "n/a", "invoice_period": "2026-10"}]});
        assert!(from_answers(&json!({"month_to_date_usage": "1"}), &bad, "2026-11").unwrap_err().contains("cannot be read"));
    }
}
