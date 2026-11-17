//! The daily runs' gate (C-051, amended by audit 2026-11-17 round v, 9). A Kubernetes CronJob runs
//! on schedule whatever core holds, and a worker pod on the autoscaled pool brings up a node even
//! when it then finds nothing to claim. So the scheduled CronJob (`daily`, on the core node: no
//! worker toleration) runs only `vyreon-core daily start-worker`: it asks core's database whether a
//! daily job is waiting (`core_db::daily::waiting`) and, only then, creates the worker Job from the
//! template of the suspended CronJob `daily-worker`, as `kubectl create job --from=cronjob/...`
//! does, unless a worker Job of the daily runs is still running.
//!
//! The Kubernetes API is called with the pod's service account (Role `daily-gate`: get the one
//! CronJob, list and create Jobs, in its namespace; deploy/k8s/40-daily.yaml). The token is a
//! credential: it is never printed or put in an error.
use serde_json::{Value, json};

/// The label every Job of the daily runs carries (the worker CronJob's job template), by which a
/// running one is found.
pub const DAILY_LABEL: (&str, &str) = ("work", "daily");
const SERVICE_ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";

/// The Kubernetes API, as the pod's service account.
pub struct Kube {
    base: String,
    namespace: String,
    token: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for Kube {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Kube {{ base: {}, namespace: {}, token: (held, not shown) }}", self.base, self.namespace)
    }
}

/// What the gate did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// A Job of the daily runs is still running: none started.
    AlreadyRunning(String),
    /// The worker Job created, by its name.
    Created(String),
}

impl Kube {
    /// `base` is the API server (`https://kubernetes.default.svc`, or a local test server); `ca`
    /// the PEM of the cluster's certificate authority (None: the built-in roots, for a test).
    pub fn new(base: &str, namespace: &str, token: &str, ca: Option<&[u8]>) -> Result<Kube, String> {
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none());
        if let Some(pem) = ca {
            let cert = reqwest::Certificate::from_pem(pem).map_err(|e| format!("the cluster's CA: {e}"))?;
            builder = builder.tls_built_in_root_certs(false).add_root_certificate(cert);
        }
        let http = builder.build().map_err(|e| e.to_string())?;
        Ok(Kube { base: base.trim_end_matches('/').to_owned(), namespace: namespace.to_owned(), token: token.trim().to_owned(), http })
    }

    /// From inside a pod: the service account's token, namespace and CA.
    pub fn in_cluster() -> Result<Kube, String> {
        let read = |name: &str| std::fs::read(format!("{SERVICE_ACCOUNT}/{name}")).map_err(|e| format!("{SERVICE_ACCOUNT}/{name}: {e} (not in a pod with a service account?)"));
        let token = String::from_utf8(read("token")?).map_err(|_| "the service account's token is not text".to_string())?;
        let namespace = String::from_utf8(read("namespace")?).map_err(|_| "the namespace is not text".to_string())?;
        Kube::new("https://kubernetes.default.svc", namespace.trim(), &token, Some(&read("ca.crt")?))
    }

    async fn call(&self, request: reqwest::RequestBuilder, what: &str) -> Result<Value, String> {
        let response = request.bearer_auth(&self.token).send().await.map_err(|e| format!("{what}: {}", e.without_url()))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            // a Kubernetes Status says why in `message`
            return Err(format!("{what}: {} {}", status.as_u16(), body["message"].as_str().unwrap_or("")));
        }
        Ok(body)
    }

    fn batch(&self, rest: &str) -> String {
        format!("{}/apis/batch/v1/namespaces/{}/{rest}", self.base, self.namespace)
    }
}

/// The names of the Jobs in a Job list that have not finished (no `Complete` or `Failed`
/// condition that is `True`).
pub fn unfinished(jobs: &Value) -> Vec<String> {
    let done = |job: &Value| {
        job["status"]["conditions"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| matches!(c["type"].as_str(), Some("Complete" | "Failed")) && c["status"] == "True"))
    };
    jobs["items"].as_array().map_or_else(Vec::new, |items| {
        items.iter().filter(|j| !done(j)).filter_map(|j| j["metadata"]["name"].as_str().map(str::to_owned)).collect()
    })
}

/// The Job a CronJob's template makes, named `<cronjob>-<generated>`, with the template's labels
/// and annotations (and `cronjob.kubernetes.io/instantiate: manual`, as kubectl marks a Job
/// created from a CronJob by hand).
pub fn job_from(cronjob: &Value) -> Result<Value, String> {
    let name = cronjob["metadata"]["name"].as_str().ok_or("the CronJob has no name")?;
    let template = &cronjob["spec"]["jobTemplate"];
    if !template["spec"].is_object() {
        return Err(format!("the CronJob {name} has no job template"));
    }
    let mut annotations = template["metadata"]["annotations"].as_object().cloned().unwrap_or_default();
    annotations.insert("cronjob.kubernetes.io/instantiate".into(), json!("manual"));
    let mut labels = template["metadata"]["labels"].as_object().cloned().unwrap_or_default();
    labels.insert(DAILY_LABEL.0.into(), json!(DAILY_LABEL.1));
    Ok(json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {"generateName": format!("{name}-"), "labels": labels, "annotations": annotations},
        "spec": template["spec"],
    }))
}

/// Starts the daily runs' worker Job from `cronjob`'s template, unless one is still running.
pub async fn start(kube: &Kube, cronjob: &str) -> Result<Started, String> {
    let selector = format!("{}={}", DAILY_LABEL.0, DAILY_LABEL.1);
    let jobs = kube.call(kube.http.get(kube.batch("jobs")).query(&[("labelSelector", selector.as_str())]), "listing the daily runs' Jobs").await?;
    if let Some(running) = unfinished(&jobs).into_iter().next() {
        return Ok(Started::AlreadyRunning(running));
    }
    let template = kube.call(kube.http.get(kube.batch(&format!("cronjobs/{cronjob}"))), &format!("reading the CronJob {cronjob}")).await?;
    let job = job_from(&template)?;
    let made = kube.call(kube.http.post(kube.batch("jobs")).json(&job), "creating the worker Job").await?;
    Ok(Started::Created(made["metadata"]["name"].as_str().unwrap_or("(no name returned)").to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_unfinished_until_it_completes_or_fails() {
        let jobs = json!({"items": [
            {"metadata": {"name": "daily-worker-a"}, "status": {"conditions": [{"type": "Complete", "status": "True"}]}},
            {"metadata": {"name": "daily-worker-b"}, "status": {"conditions": [{"type": "Failed", "status": "True"}]}},
            {"metadata": {"name": "daily-worker-c"}, "status": {"active": 1}},
            {"metadata": {"name": "daily-worker-d"}, "status": {"conditions": [{"type": "Suspended", "status": "True"}]}},
        ]});
        assert_eq!(unfinished(&jobs), ["daily-worker-c", "daily-worker-d"]);
        assert!(unfinished(&json!({"items": []})).is_empty());
    }

    #[test]
    fn the_token_is_never_shown() {
        let k = Kube::new("http://127.0.0.1:1", "vyreon", "a-service-account-token", None).unwrap();
        assert!(!format!("{k:?}").contains("a-service-account-token"));
    }
}
