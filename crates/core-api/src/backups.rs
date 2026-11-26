//! C-068 round 2 (audit opsurface `round2-core.md` 2, 3): core reads the backup plugin's own record
//! through the Kubernetes API, every few minutes, and keeps it (`core.backup_evidence`) for the watch:
//! the database's Cluster (whether it archives its WAL through the Barman Cloud plugin) and the
//! plugin's ObjectStore (its recovery window for the server). PostgreSQL's own signs are not
//! evidence: CloudNativePG reports a WAL file archived with no archiver configured, and
//! `pg_backup_stop` writes its history file before the copy reaches the bucket.
//!
//! In the cluster: the pod's service account (`vyreon-core`, a Role that may only `get` these two
//! kinds in its namespace, `deploy/k8s/20-core.yaml`), its token read at each request (Kubernetes
//! rotates it) and the cluster's CA, from the standard mount. Nothing is written to Kubernetes.
use crate::App;
use core_db::watch::{self, BackupEvidence};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The standard service-account mount (Kubernetes, "Accessing the Kubernetes API from a Pod").
pub const SERVICE_ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
/// The API server's in-cluster name; its certificate is signed by the mounted CA.
pub const API: &str = "https://kubernetes.default.svc";

/// The in-cluster API, as this pod's service account.
pub struct Kube {
    http: reqwest::Client,
    token: PathBuf,
    namespace: String,
}

impl Kube {
    /// From the service-account mount; an error says what is missing (no token is ever printed).
    pub fn in_cluster() -> Result<Kube, String> {
        let dir = PathBuf::from(SERVICE_ACCOUNT);
        let ca = std::fs::read(dir.join("ca.crt")).map_err(|e| format!("no service-account CA ({e}): is the pod's token mounted?"))?;
        let ca = reqwest::Certificate::from_pem(&ca).map_err(|e| format!("the service-account CA: {e}"))?;
        let namespace = std::fs::read_to_string(dir.join("namespace")).map_err(|e| format!("no service-account namespace ({e})"))?.trim().to_owned();
        let http = reqwest::Client::builder()
            .add_root_certificate(ca)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Kube { http, token: dir.join("token"), namespace })
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let token = std::fs::read_to_string(&self.token).map_err(|e| format!("no service-account token ({e})"))?;
        let r = self.http.get(format!("{API}{path}")).bearer_auth(token.trim()).send().await.map_err(|e| format!("GET {path}: {}", e.without_url()))?;
        let status = r.status();
        if !status.is_success() {
            return Err(format!("GET {path}: {status}"));
        }
        r.json().await.map_err(|e| format!("GET {path}: {}", e.without_url()))
    }

    /// C-091: the Pod `name` of this namespace (a worker's pod: its name is the worker's), or None when
    /// there is none (404).
    pub async fn pod(&self, name: &str) -> Result<Option<Value>, String> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
            return Err("not a pod's name".into());
        }
        let path = format!("/api/v1/namespaces/{}/pods/{name}", self.namespace);
        let token = std::fs::read_to_string(&self.token).map_err(|e| format!("no service-account token ({e})"))?;
        let r = self.http.get(format!("{API}{path}")).bearer_auth(token.trim()).send().await.map_err(|e| format!("GET {path}: {}", e.without_url()))?;
        match r.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            s if s.is_success() => r.json().await.map(Some).map_err(|e| format!("GET {path}: {}", e.without_url())),
            s => Err(format!("GET {path}: {s}")),
        }
    }

    /// The plugin's record for the Cluster `cluster` of this namespace.
    pub async fn backups(&self, cluster: &str) -> Result<BackupEvidence, String> {
        let ns = &self.namespace;
        let c = self.get(&format!("/apis/postgresql.cnpg.io/v1/namespaces/{ns}/clusters/{cluster}")).await?;
        let store = match BackupEvidence::from_objects(cluster, &c, None).object_store {
            Some(name) => Some(self.get(&format!("/apis/barmancloud.cnpg.io/v1/namespaces/{ns}/objectstores/{name}")).await?),
            None => None,
        };
        Ok(BackupEvidence::from_objects(cluster, &c, store.as_ref()))
    }
}

/// Reads the plugin's record for `cluster` every `every` and keeps it, or why it could not be read
/// (a missing mount, a refusal), so the watch says the backups are unverified rather than nothing.
pub fn spawn(app: Arc<App>, kube: Result<Kube, String>, cluster: String, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let read = match &kube {
                Ok(k) => k.backups(&cluster).await,
                Err(e) => Err(e.clone()),
            };
            let Ok(mut client) = app.pool.get().await else {
                eprintln!("vyreon-core: backups: no database connection");
                continue;
            };
            if let Err(e) = &read {
                eprintln!("vyreon-core: backups: the plugin's record cannot be read: {e}");
            }
            let client: &mut tokio_postgres::Client = &mut client;
            if let Err(e) = watch::put_backup_evidence(&*client, read.as_ref().map_err(String::as_str)).await {
                eprintln!("vyreon-core: backups: {e}");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    /// Off the cluster (no service-account mount) the reader says what is missing, so the watch keeps
    /// the backups unverified with that reason, never silent.
    #[test]
    fn without_a_service_account_the_reason_is_kept() {
        if std::path::Path::new(super::SERVICE_ACCOUNT).exists() {
            return;
        }
        let e = super::Kube::in_cluster().err().unwrap();
        assert!(e.starts_with("no service-account CA"), "{e}");
    }
}
