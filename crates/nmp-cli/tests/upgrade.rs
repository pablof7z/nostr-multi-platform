mod helpers;

use helpers::{nmp, TempDir};
use std::fs;

#[test]
fn upgrade_switches_manifest_to_versioned_nmp_release() {
    let tmp = TempDir::new("upgrade");
    let root = tmp.path().join("demoapp");

    let init = nmp(
        tmp.path(),
        &["init", "demoapp", "--path", root.to_str().unwrap()],
    );
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let upgrade = nmp(&root, &["upgrade", "--to", "0.2.0"]);
    assert!(
        upgrade.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&upgrade.stderr)
    );

    let manifest = fs::read_to_string(root.join("nmp.toml")).unwrap();
    assert!(manifest.contains("dependency_mode = \"version\""));
    assert!(manifest.contains("version = \"0.2.0\""));

    let app_core = fs::read_to_string(root.join("crates/demoapp-core/Cargo.toml")).unwrap();
    assert!(app_core.contains("nmp-core = \"0.2.0\""));

    let gen = nmp(&root, &["gen", "modules"]);
    assert!(
        gen.status.success(),
        "gen failed: {}",
        String::from_utf8_lossy(&gen.stderr)
    );

    let cargo_toml =
        fs::read_to_string(root.join("apps/demoapp/nmp-app-demoapp/Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("nmp-core = \"0.2.0\""));
    assert!(cargo_toml.contains("nmp-ffi = \"0.2.0\""));
    assert!(cargo_toml.contains("demoapp_core = { package = \"demoapp-core\", path = \""));
}

#[test]
fn doctor_reports_dependency_policy() {
    let tmp = TempDir::new("doctor");
    let root = tmp.path().join("demoapp");

    let init = nmp(
        tmp.path(),
        &[
            "init",
            "demoapp",
            "--path",
            root.to_str().unwrap(),
            "--nmp-version",
            "0.3.0",
        ],
    );
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let doctor = nmp(&root, &["doctor"]);
    assert!(
        doctor.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let stdout = String::from_utf8_lossy(&doctor.stdout);
    assert!(stdout.contains("nmp dependency mode: version"));
    assert!(stdout.contains("nmp version: 0.3.0"));
}
