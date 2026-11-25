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
