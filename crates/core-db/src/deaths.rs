//! C-091 (owner, 2026-11-26): a worker that stopped before its job was done is said on the alerts channel, with its
//! cause. On 2026-11-26 five of eight day-block pods were killed for memory or evicted by their node between 12:40 and
//! 13:37 MT and no alert said so; the progress post read "nothing broken". Each lease that ended EXPIRED (its worker
//! lost: killed, evicted, its node gone, or core not answering) or FAILED (the worker's own stop) is said once
//! (`core.lease_death`), within `SAY_WITHIN_HOURS`, all those of one minute in one post. Its cause is the pod's own
//! status where core could read it (the service reads it from the Kubernetes API: `core_api::deaths`), else what the
//! lease says. A day the input plan refused is DAY_REFUSED's; a cancellation or a withdrawal is no death. This
//! replaces C-089's nightly-only post: the nightly's stops are said the same way, named as the nightly.
use crate::notice;
use crate::watch::{ZONE, one_line, plain_cause};
use tokio_postgres::GenericClient;

/// A stop older than this when first seen is not said (a core down for longer says nothing stale).
pub const SAY_WITHIN_HOURS: f64 = 6.0;

fn db(e: tokio_postgres::Error) -> String {
    e.as_db_error().map_or_else(|| e.to_string(), |d| d.message().to_owned())
}

/// A worker's stop not said yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Death {
    pub lease: i64,
    /// The worker's name: its pod's (VYREON_WORKER_NAME).
    pub worker: String,
    pub kind: String,
    /// A daily session's job (priority 1).
    pub daily: bool,
    /// The sessions the lease was given (from where it began).
    pub first: String,
    pub last: String,
    /// The day it stopped on, where known.
    pub day: Option<String>,
    /// EXPIRED or FAILED.
    pub outcome: String,
    pub failure: Option<String>,
    /// Hours of work since the lease's last published day (or its grant).
    pub lost_hours: f64,
    /// The job's state now: QUEUED (it starts again by itself), FAILED, DONE or CANCELLED.
    pub state: String,
    /// When it ended, HH:MM in the owner's zone.
    pub at: String,
}

/// The stops to say.
pub async fn pending(c: &impl GenericClient) -> Result<Vec<Death>, String> {
    let rows = c
        .query(
            "SELECT l.lease_id, l.worker, j.kind, j.priority = 1, COALESCE(l.start_session, j.first_session)::text, COALESCE(l.job_last, j.last_session)::text,
                    COALESCE(l.stopped_on, substring(l.failed_stage FROM '^day ([0-9]{4}-[0-9]{2}-[0-9]{2})$')::date)::text,
                    l.outcome, l.failure,
                    GREATEST(extract(epoch FROM l.ended_at - GREATEST(l.granted_at,
                        (SELECT max(d.published_at) FROM core.lease_revision lr JOIN vyreon.day_revision d USING (revision_id)
                         WHERE lr.lease_id = l.lease_id AND d.day_local AND d.published_at <= l.ended_at))), 0)::float8 / 3600,
                    j.state, to_char(l.ended_at AT TIME ZONE $1, 'HH24:MI')
             FROM core.lease l JOIN core.job j USING (job_id)
             WHERE l.outcome IN ('EXPIRED', 'FAILED') AND l.ended_at > now() - make_interval(secs => $2::float8 * 3600)
               AND strpos(COALESCE(l.failure, ''), 'refused by the input plan') = 0
               AND NOT EXISTS (SELECT FROM core.lease_death x WHERE x.lease_id = l.lease_id)
             ORDER BY l.ended_at, l.lease_id",
            &[&ZONE, &SAY_WITHIN_HOURS],
        )
        .await
        .map_err(db)?;
    Ok(rows
        .iter()
        .map(|r| Death {
            lease: r.get(0),
            worker: r.get(1),
            kind: r.get(2),
            daily: r.get(3),
            first: r.get(4),
            last: r.get(5),
            day: r.get(6),
            outcome: r.get(7),
            failure: r.get(8),
            lost_hours: r.get(9),
            state: r.get(10),
            at: r.get(11),
        })
        .collect())
}

/// A stop's cause in plain words: the pod's (`pod`, as `pod_cause` reads it) where known, else the lease's.
pub fn cause(d: &Death, pod: Option<&str>) -> String {
    if let Some(p) = pod {
        return p.to_owned();
    }
    match d.outcome.as_str() {
        "FAILED" => format!("the worker stopped: {}", one_line(&plain_cause(d.failure.as_deref().unwrap_or("no reason given")), 160)),
        _ => "lost contact with core (cause not known)".to_owned(),
    }
}

/// What stopped: "a block (Oct 1 – Nov 20)", "the nightly run of Nov 25".
pub fn what(d: &Death) -> String {
    let day = |s: &str| crate::plain::day(s).unwrap_or_else(|| s.to_owned());
    let on = d.day.as_deref().unwrap_or(&d.first);
    match (d.kind.as_str(), d.daily) {
        ("DAY_BLOCK", true) => format!("the nightly run of {}", day(on)),
        ("DAY_BLOCK", false) if d.first == d.last => format!("a block ({})", day(&d.first)),
        ("DAY_BLOCK", false) => format!("a block ({} – {})", day(&d.first), day(&d.last)),
        ("TAPE", true) => format!("the nightly tape capture of {}", day(on)),
        ("TAPE", false) => format!("a tape capture of {}", day(on)),
        ("ORDERED_PASS", true) => format!("the nightly history pass of {}", day(on)),
        ("ORDERED_PASS", false) => format!("the history pass of {}", day(on)),
        ("REPUBLISH", _) => format!("a recompute of {}", day(on)),
        _ => format!("a reference re-check of {}", day(on)),
    }
}

/// `what`, short, for a list: a backfill block by its days.
fn short_what(d: &Death) -> String {
    let day = |s: &str| crate::plain::day(s).unwrap_or_else(|| s.to_owned());
    match (d.kind.as_str(), d.daily) {
        ("DAY_BLOCK", false) if d.first == d.last => day(&d.first),
        ("DAY_BLOCK", false) => format!("{} – {}", day(&d.first), day(&d.last)),
        _ => what(d),
    }
}

fn next(state: &str, many: bool) -> &'static str {
    match (state, many) {
        ("QUEUED" | "LEASED", false) => "it starts again by itself",
        ("QUEUED" | "LEASED", true) => "they start again by themselves",
        ("FAILED", _) => "set aside after its tries (a separate alert says so)",
        _ => "nothing left to redo",
    }
}

fn lost(hours: f64) -> Option<String> {
    (hours >= 0.1).then(|| if hours < 1.0 { format!("about {} min of work to redo", (hours * 60.0).round()) } else { format!("about {:.1} h of work to redo", hours) })
}

/// The post for the stops `said` (each with its cause), one for all.
pub fn text(said: &[(Death, String)]) -> Option<String> {
    let tail = "Published numbers not affected. The coordinator is on it.";
    match said {
        [] => None,
        [(d, cause)] => {
            let on = match (&d.day, d.kind.as_str()) {
                (Some(day), "DAY_BLOCK") if !d.daily && d.first != d.last => format!(" on {}", crate::plain::day(day).unwrap_or_else(|| day.clone())),
                _ => String::new(),
            };
            let lost = lost(d.lost_hours).map(|l| format!("; {l}")).unwrap_or_default();
            Some(format!("⚠️ Problem: {}{on} stopped at {}: {cause}{lost}; {}. {tail}", what(d), d.at, next(&d.state, false)))
        }
        many => {
            let mut causes: Vec<(String, usize)> = Vec::new();
            for (_, c) in many {
                match causes.iter_mut().find(|(k, _)| k == c) {
                    Some((_, n)) => *n += 1,
                    None => causes.push((c.clone(), 1)),
                }
            }
            let causes: Vec<String> = causes.iter().map(|(c, n)| format!("{n} {c}")).collect();
            let names: Vec<String> = many.iter().map(|(d, _)| short_what(d)).collect();
            let hours: f64 = many.iter().map(|(d, _)| d.lost_hours).sum();
            let lost = lost(hours).map(|l| format!("; {l}")).unwrap_or_default();
            let all_again = many.iter().all(|(d, _)| matches!(d.state.as_str(), "QUEUED" | "LEASED"));
            Some(format!(
                "⚠️ Problem: {} workers stopped since {}: {} ({}){lost}; {}. {tail}",
                many.len(),
                many[0].0.at,
                causes.join(", "),
                crate::progress::names_max(&names, 4),
                if all_again { next("QUEUED", true) } else { "those with tries left start again by themselves" }
            ))
        }
    }
}

/// Says the stops `said` (each with its cause) in one alert, and records each as said, in the caller's
/// transaction.
pub async fn say(c: &impl GenericClient, said: &[(Death, String)]) -> Result<(), String> {
    for (d, cause) in said {
        c.execute("INSERT INTO core.lease_death (lease_id, cause) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[&d.lease, cause]).await.map_err(db)?;
    }
    if let Some(t) = text(said) {
        notice::alert(c, &t).await?;
    }
    Ok(())
}

/// The cause a pod's status gives (the Kubernetes API's Pod; None: the pod is gone), in plain words; None when
/// the status says nothing about why (the lease's own cause is said then).
pub fn pod_cause(pod: Option<&serde_json::Value>) -> Option<String> {
    let Some(pod) = pod else {
        return Some("its pod is gone (its node was removed, or the pod deleted)".into());
    };
    let status = &pod["status"];
    let reason = status["reason"].as_str().unwrap_or("");
    if reason == "Evicted" {
        let message = status["message"].as_str().unwrap_or("").to_ascii_lowercase();
        return Some(if message.contains("memory") { "evicted by its node (the node ran low on memory)".into() } else { "evicted by its node".into() });
    }
    if reason == "DeadlineExceeded" {
        return Some("stopped at its time limit".into());
    }
    let containers = status["containerStatuses"].as_array().cloned().unwrap_or_default();
    let terminated: Vec<&serde_json::Value> =
        containers.iter().flat_map(|c| [&c["state"]["terminated"], &c["lastState"]["terminated"]]).filter(|t| t.is_object()).collect();
    if terminated.iter().any(|t| t["reason"].as_str() == Some("OOMKilled")) {
        return Some("killed for lack of memory".into());
    }
    if let Some(t) = terminated.iter().find(|t| t["exitCode"].as_i64().is_some_and(|c| c != 0)) {
        return Some(format!("the worker exited with an error (code {})", t["exitCode"].as_i64().unwrap_or(0)));
    }
    if status["phase"].as_str() == Some("Running") {
        return Some("lost contact with core while still running".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn death(kind: &str, daily: bool, outcome: &str, state: &str) -> Death {
        Death {
            lease: 1,
            worker: "day-block-x".into(),
            kind: kind.into(),
            daily,
            first: "2024-10-01".into(),
            last: "2024-11-20".into(),
            day: Some("2024-10-14".into()),
            outcome: outcome.into(),
            failure: Some("stop: 2024-10-14 not published: h5-surface: the lattice solver did not converge".into()),
            lost_hours: 2.46,
            state: state.into(),
            at: "12:40".into(),
        }
    }

    /// The pod statuses of 2026-11-26's dead blocks, as the Kubernetes API gives them: killed for memory (its
    /// container's last state), evicted by the node, gone, still running (core was down), a time limit.
    #[test]
    fn a_pods_status_names_the_cause() {
        let oom = json!({"status": {"phase": "Failed", "containerStatuses": [{"name": "worker", "state": {"terminated": {"reason": "OOMKilled", "exitCode": 137}}}]}});
        assert_eq!(pod_cause(Some(&oom)).as_deref(), Some("killed for lack of memory"));
        let evicted = json!({"status": {"phase": "Failed", "reason": "Evicted", "message": "The node was low on resource: memory. Threshold quantity: 100Mi, available: 9Ki."}});
        assert_eq!(pod_cause(Some(&evicted)).as_deref(), Some("evicted by its node (the node ran low on memory)"));
        assert_eq!(pod_cause(None).as_deref(), Some("its pod is gone (its node was removed, or the pod deleted)"));
        assert_eq!(pod_cause(Some(&json!({"status": {"phase": "Running", "containerStatuses": [{"state": {"running": {}}}]}}))).as_deref(), Some("lost contact with core while still running"));
        assert_eq!(pod_cause(Some(&json!({"status": {"phase": "Failed", "reason": "DeadlineExceeded"}}))).as_deref(), Some("stopped at its time limit"));
        let error = json!({"status": {"phase": "Failed", "containerStatuses": [{"state": {"terminated": {"reason": "Error", "exitCode": 2}}}]}});
        assert_eq!(pod_cause(Some(&error)).as_deref(), Some("the worker exited with an error (code 2)"));
        assert_eq!(pod_cause(Some(&json!({"status": {"phase": "Pending"}}))), None);
    }

    /// One stop: what, where, when, why, the work lost and what happens next, in plain words; several in one
    /// minute: one post, the causes counted.
    #[test]
    fn a_stop_is_one_plain_post() {
        let block = death("DAY_BLOCK", false, "EXPIRED", "QUEUED");
        assert_eq!(
            text(&[(block.clone(), "killed for lack of memory".into())]).unwrap(),
            "⚠️ Problem: a block (Oct 1 – Nov 20) on Oct 14 stopped at 12:40: killed for lack of memory; about 2.5 h of work to redo; it starts again by itself. Published numbers not affected. The coordinator is on it."
        );
        let nightly = Death { daily: true, first: "2026-11-25".into(), last: "2026-11-25".into(), day: Some("2026-11-25".into()), lost_hours: 0.0, ..block.clone() };
        assert_eq!(
            text(&[(nightly.clone(), cause(&nightly, None))]).unwrap(),
            "⚠️ Problem: the nightly run of Nov 25 stopped at 12:40: lost contact with core (cause not known); it starts again by itself. Published numbers not affected. The coordinator is on it."
        );
        let failed = death("DAY_BLOCK", false, "FAILED", "FAILED");
        assert_eq!(cause(&failed, None), "the worker stopped: h5-surface: the lattice solver did not converge");
        let five: Vec<(Death, String)> = (0..5)
            .map(|i| (Death { lease: i, first: format!("2024-0{}-01", i + 1), last: format!("2024-0{}-20", i + 1), ..block.clone() }, if i < 4 { "killed for lack of memory".to_owned() } else { "evicted by its node".to_owned() }))
            .collect();
        assert_eq!(
            text(&five).unwrap(),
            "⚠️ Problem: 5 workers stopped since 12:40: 4 killed for lack of memory, 1 evicted by its node (Jan 1 – Jan 20, Feb 1 – Feb 20, Mar 1 – Mar 20, Apr 1 – Apr 20 and 1 more); about 12.3 h of work to redo; they start again by themselves. Published numbers not affected. The coordinator is on it."
        );
        assert_eq!(text(&[]), None);
    }
}
