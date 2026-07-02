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

    // ADR-0046: the scaffolded core crate is a thin composition shell whose
    // `nmp-*` dependencies are git-rev pins (consumers pin NMP by git rev).
    // `nmp upgrade` repoints each pin at the new release tag — there is no
    // `nmp gen modules` step and no generated raw `apps/` tree.
    let app_core = fs::read_to_string(root.join("crates/demoapp-core/Cargo.toml")).unwrap();
    assert!(
        app_core.contains("nmp-core = { git = ")
            && app_core.contains("tag = \"v0.2.0\"")
            && app_core.contains("package = \"nmp-core\""),
        "upgrade must repoint nmp-core to the v0.2.0 git tag:\n{app_core}"
    );
    assert!(
        app_core.contains("package = \"nmp-native-runtime\"")
            && app_core.contains("package = \"nmp-substrate\"")
            && app_core.contains("package = \"nmp-nip50\"")
            && app_core.contains("package = \"nmp-nip51\"")
            && app_core.contains("package = \"nmp-nip17\"")
            && app_core.contains("package = \"nmp-content\"")
            && !app_core.contains("package = \"nmp-defaults\""),
        "upgrade must repoint explicit owner crates and must not restore nmp-defaults:\n{app_core}"
    );
    assert!(
        !root.join("apps").exists(),
        "ADR-0046: upgrade must not produce a generated apps/ tree"
    );
}
