//! The daily CronJob's gate (C-051, audit 2026-11-17 round v, 9) against a local stand-in for the
//! Kubernetes API: the worker Job is created from the suspended CronJob's template, labelled as the
//! daily runs', with the service account's token; nothing is created while a daily Job runs. The
//! CronJob document is the job template of deploy/k8s/40-daily.yaml, reduced to what the gate reads.
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use core_api::gate::{self, Kube, Started};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Api {
    jobs: Vec<Value>,
    created: Vec<Value>,
    seen: Vec<String>,
}

type Shared = State<Arc<Mutex<Api>>>;

fn authorized(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer the-service-account-token")
}

fn cronjob() -> Value {
    json!({
        "apiVersion": "batch/v1", "kind": "CronJob",
        "metadata": {"name": "daily-worker", "namespace": "vyreon", "uid": "0d1e"},
        "spec": {"suspend": true, "schedule": "45 7 * * 2-6", "jobTemplate": {
            "metadata": {"labels": {"work": "daily"}},
            "spec": {"completions": 3, "parallelism": 1, "completionMode": "Indexed", "backoffLimitPerIndex": 1, "ttlSecondsAfterFinished": 604800,
                     "template": {"spec": {"restartPolicy": "Never", "nodeSelector": {"vyreon": "worker"}, "containers": [{"name": "ordered-pass", "image": "w"}]}}}
        }}
    })
}

async fn list_jobs(State(api): Shared, headers: HeaderMap, Path(ns): Path<String>, Query(q): Query<HashMap<String, String>>) -> (StatusCode, Json<Value>) {
    let mut api = api.lock().unwrap();
    api.seen.push(format!("list {ns} {}", q.get("labelSelector").cloned().unwrap_or_default()));
    if !authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"kind": "Status", "message": "Unauthorized"})));
    }
    (StatusCode::OK, Json(json!({"kind": "JobList", "items": api.jobs.clone()})))
}

async fn create_job(State(api): Shared, headers: HeaderMap, Path(ns): Path<String>, Json(job): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut api = api.lock().unwrap();
    api.seen.push(format!("create {ns}"));
    if !authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"kind": "Status", "message": "Unauthorized"})));
    }
    let mut made = job.clone();
    made["metadata"]["name"] = json!(format!("{}x7k2p", job["metadata"]["generateName"].as_str().unwrap()));
    api.created.push(job);
    (StatusCode::CREATED, Json(made))
}

async fn get_cronjob(State(api): Shared, headers: HeaderMap, Path((ns, name)): Path<(String, String)>) -> (StatusCode, Json<Value>) {
    api.lock().unwrap().seen.push(format!("get {ns} {name}"));
    if !authorized(&headers) || name != "daily-worker" {
        return (StatusCode::FORBIDDEN, Json(json!({"kind": "Status", "message": format!("cronjobs.batch \"{name}\" is forbidden")})));
    }
    (StatusCode::OK, Json(cronjob()))
}

#[tokio::test]
async fn the_worker_job_is_made_from_the_template_only_when_none_runs() {
    let api: Arc<Mutex<Api>> = Arc::default();
    let router = Router::new()
        .route("/apis/batch/v1/namespaces/{ns}/jobs", get(list_jobs).post(create_job))
        .route("/apis/batch/v1/namespaces/{ns}/cronjobs/{name}", get(get_cronjob))
        .with_state(api.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let kube = Kube::new(&base, "vyreon", "the-service-account-token\n", None).unwrap();

    // no daily Job running: one created from the template, labelled as the daily runs'
    assert_eq!(gate::start(&kube, "daily-worker").await.unwrap(), Started::Created("daily-worker-x7k2p".into()));
    let created = api.lock().unwrap().created.clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["metadata"]["generateName"], "daily-worker-");
    assert_eq!(created[0]["metadata"]["labels"], json!({"work": "daily"}));
    assert_eq!(created[0]["metadata"]["annotations"], json!({"cronjob.kubernetes.io/instantiate": "manual"}));
    assert_eq!(created[0]["spec"], cronjob()["spec"]["jobTemplate"]["spec"]);
    assert_eq!(api.lock().unwrap().seen, ["list vyreon work=daily", "get vyreon daily-worker", "create vyreon"]);

    // a daily Job still running (yesterday's, past the morning): none created
    api.lock().unwrap().jobs = vec![
        json!({"metadata": {"name": "daily-worker-old"}, "status": {"conditions": [{"type": "Complete", "status": "True"}]}}),
        json!({"metadata": {"name": "daily-worker-x7k2p"}, "status": {"active": 1}}),
    ];
    assert_eq!(gate::start(&kube, "daily-worker").await.unwrap(), Started::AlreadyRunning("daily-worker-x7k2p".into()));
    assert_eq!(api.lock().unwrap().created.len(), 1);

    // what the API refuses is said, without the token
    api.lock().unwrap().jobs.clear();
    let refused = gate::start(&kube, "another").await.unwrap_err();
    assert!(refused.contains("403") && refused.contains("forbidden") && !refused.contains("the-service-account-token"), "{refused}");
    let wrong = Kube::new(&base, "vyreon", "not-the-token", None).unwrap();
    assert!(gate::start(&wrong, "daily-worker").await.unwrap_err().contains("401"));
}
