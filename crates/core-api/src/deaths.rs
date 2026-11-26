//! C-091: a worker that stopped before its job was done is said on the alerts channel with its cause
//! (`core_db::deaths`). Every `every`, the stops not said yet are read; for each lease that expired (its worker
//! lost), its pod's status is read from the Kubernetes API (the worker's name is its pod's; the service account
//! may `get` pods in its namespace, `deploy/k8s/20-core.yaml`): killed for memory, evicted, gone, or still running.
//! A pod that cannot be read leaves the lease's own cause. Off the cluster, the lease's cause alone.
use crate::App;
use crate::backups::Kube;
use std::sync::Arc;
use std::time::Duration;

pub fn spawn(app: Arc<App>, kube: Option<Kube>, every: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Ok(mut client) = app.pool.get().await else {
                eprintln!("vyreon-core: worker stops: no database connection");
                continue;
            };
            let client: &mut tokio_postgres::Client = &mut client;
            if let Err(e) = say(client, kube.as_ref()).await {
                eprintln!("vyreon-core: worker stops: {e}");
            }
        }
    })
}

/// Says the stops not said yet, their causes read from their pods where `kube` can.
pub async fn say(client: &mut tokio_postgres::Client, kube: Option<&Kube>) -> Result<usize, String> {
    let pending = core_db::deaths::pending(&*client).await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let mut said = Vec::new();
    for d in pending {
        let pod = match (kube, d.outcome.as_str()) {
            (Some(k), "EXPIRED") => match k.pod(&d.worker).await {
                Ok(p) => core_db::deaths::pod_cause(p.as_ref()),
                Err(e) => {
                    eprintln!("vyreon-core: worker stops: the pod of a lost lease cannot be read: {e}");
                    None
                }
            },
            _ => None,
        };
        let cause = core_db::deaths::cause(&d, pod.as_deref());
        said.push((d, cause));
    }
    let tx = client.transaction().await.map_err(|e| e.to_string())?;
    core_db::deaths::say(&tx, &said).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(said.len())
}
