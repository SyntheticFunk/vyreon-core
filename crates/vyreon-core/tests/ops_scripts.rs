//! The operator's scripts under deploy/k8s/ops, run against stubbed docker and kubectl (no registry, no
//! cluster): each script's test lives beside it in `deploy/k8s/ops/tests`.

fn run(test: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/k8s/ops/tests").join(test);
    let out = std::process::Command::new("bash").arg(&path).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}\n{said}\n{}", path.display(), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("all passed"), "{said}");
}

/// C-080: `release.sh --core` verifies an identical-content rebuild by its platform manifests, compares
/// the digest `docker push` reported, and resumes with `--from N` from its run file.
#[test]
fn release_core() {
    run("release-core.sh");
}

/// 2026-11-26: `apply-core.sh` applies 20-core.yaml with the running core image in place of its placeholder,
/// and refuses, applying nothing, while a placeholder would remain (the raw apply took core down 20 minutes).
#[test]
fn apply_core() {
    run("apply-core.sh");
}

/// 2026-11-25: `release.sh` step 11 no longer stops on a kind whose work was cancelled before a pod claimed it
/// (the re-run watch's cancelled REFCHECK jobs), and still stops on one claimable with no claim.
#[test]
fn release_verify() {
    run("release-verify.sh");
}

/// C-081 (engine D-480: the IP01 registry view's embedded bytes changed; each home must hold the new one,
/// and a worker refuses a home whose copy differs): every worker home on the cluster (`VYREON_HOME`, the
/// `/work` mount) is an `emptyDir`, so each pod materialises the view its build carries and none keeps an
/// older one.
#[test]
fn every_worker_home_on_the_cluster_is_empty_when_its_pod_starts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/k8s");
    let mut homes = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines {
            let Some(name) = line.trim().strip_prefix("- {name: ").and_then(|l| l.strip_suffix(", mountPath: /work}")) else { continue };
            homes += 1;
            let volume = lines.iter().position(|l| l.trim() == format!("- name: {name}")).unwrap_or_else(|| panic!("{}: no volume {name}", path.display()));
            assert_eq!(lines[volume + 1].trim(), "emptyDir: {}", "{}: the home {name} is not an emptyDir", path.display());
        }
    }
    assert!(homes >= 12, "{homes} homes found");
}
