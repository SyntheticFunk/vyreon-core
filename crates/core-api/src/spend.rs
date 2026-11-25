//! C-078 (owner, 2026-11-24: automation debt 3): core reads DigitalOcean's billing figures every hour
//! and keeps them (`core_db::spend`), for the watch's spend line and its `SPEND` alerts. The token is a
//! read-only one the owner makes (a custom-scoped DigitalOcean token with `billing:read` only), kept in
//! the secret `vyreon-core-do-billing` and read from the file `VYREON_CORE_DO_TOKEN_FILE` names at each
//! read (never printed, never logged). Absent, the reading says so and the line says the spend is not
//! known; nothing fails.
use crate::App;
use core_db::spend::{self, Reading};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// DigitalOcean's API.
pub const API: &str = "https://api.digitalocean.com";
/// How often the figures are read (DigitalOcean updates its month-to-date usage a few times a day).
pub const EVERY: Duration = Duration::from_secs(3600);

/// The billing reader: the token's file, read at each request.
pub struct Billing {
    http: reqwest::Client,
    token: Option<PathBuf>,
    api: String,
}

impl Billing {
    /// `token`: the file the setting names (None: not set). `api`: DigitalOcean's base URL (`API`; a test's own).
    pub fn new(token: Option<PathBuf>, api: &str) -> Result<Billing, String> {
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
        Ok(Billing { http, token, api: api.trim_end_matches('/').to_owned() })
    }

    async fn get(&self, token: &str, path: &str) -> Result<Value, String> {
        let r = self.http.get(format!("{}{path}", self.api)).bearer_auth(token).send().await.map_err(|e| format!("GET {path}: {}", e.without_url()))?;
        let status = r.status();
        if !status.is_success() {
            // the body may echo the request; only the status is kept
            return Err(format!("GET {path}: DigitalOcean answered {status}{}", if status.as_u16() == 401 || status.as_u16() == 403 { " (the token is refused: is it valid, with billing:read?)" } else { "" }));
        }
        r.json().await.map_err(|e| format!("GET {path}: {}", e.without_url()))
    }

    /// One reading, or why there is none.
    pub async fn read(&self, month_now: &str) -> Result<Reading, String> {
        let Some(file) = &self.token else {
            return Err("no DigitalOcean token: VYREON_CORE_DO_TOKEN_FILE is not set (the owner makes a billing:read token, deploy/k8s/README.md, Spend)".into());
        };
        let token = match std::fs::read_to_string(file) {
            Ok(t) if !t.trim().is_empty() => t.trim().to_owned(),
            _ => return Err("no DigitalOcean token: the secret vyreon-core-do-billing is not made (deploy/k8s/README.md, Spend)".into()),
        };
        let balance = self.get(&token, "/v2/customers/my/balance").await?;
        let invoices = self.get(&token, "/v2/customers/my/invoices?per_page=200").await?;
        spend::from_answers(&balance, &invoices, month_now)
    }
}

/// Reads the figures every `every` (the first at once) and keeps each reading, or why there was none.
pub fn spawn(app: Arc<App>, billing: Billing, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Ok(client) = app.pool.get().await else {
                eprintln!("vyreon-core: spend: no database connection");
                continue;
            };
            let month: String = match client.query_one("SELECT to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM')", &[]).await {
                Ok(r) => r.get(0),
                Err(e) => {
                    eprintln!("vyreon-core: spend: {e}");
                    continue;
                }
            };
            let read = billing.read(&month).await;
            if let Err(e) = &read {
                eprintln!("vyreon-core: spend: {e}");
            }
            if let Err(e) = spend::put_reading(&**client, read.as_ref().map_err(String::as_str)).await {
                eprintln!("vyreon-core: spend: {e}");
            }
        }
    })
}
