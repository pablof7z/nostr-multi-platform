//! Install coverage for the app-level native component host entries.

mod helpers;

use helpers::{nmp, TempDir};
use std::fs;

#[test]
fn add_swiftui_component_host_installs_host_and_dependencies() {
    let tmp = TempDir::new("swiftui-component-host");

    let out = nmp(tmp.path(), &["add", "component", "swiftui/component-host"]);
    assert!(
        out.status.success(),
        "nmp add component swiftui/component-host failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NmpComponentHost/NmpComponentHost.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrUser/NostrProfileHost.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbedHostEnvironment.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.swift")
        .exists());
    assert!(!tmp
        .path()
        .join("Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/user-avatar\""));
    assert!(lock.contains("id = \"swiftui/content-kind-registry\""));
    assert!(lock.contains("id = \"swiftui/component-host\""));
    assert!(lock.contains("NmpComponentHost.swift"));
}

#[test]
fn add_compose_component_host_installs_provider_and_dependencies() {
    let tmp = TempDir::new("compose-component-host");

    let out = nmp(tmp.path(), &["add", "component", "compose/component-host"]);
    assert!(
        out.status.success(),
        "nmp add component compose/component-host failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NmpComponentHost/NmpComponentHostProvider.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrUser/NostrProfileHost.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbeddedEvent.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.kt")
        .exists());
    assert!(!tmp
        .path()
        .join("Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/user-avatar\""));
    assert!(lock.contains("id = \"compose/content-kind-registry\""));
    assert!(lock.contains("id = \"compose/component-host\""));
    assert!(lock.contains("NmpComponentHostProvider.kt"));
}

#[test]
fn add_component_host_with_fixture_installs_conformance_rows() {
    let swift = TempDir::new("swiftui-component-host-fixture");
    let swift_out = nmp(
        swift.path(),
        &[
            "add",
            "component",
            "swiftui/component-host",
            "--with",
            "fixture",
        ],
    );
    assert!(
        swift_out.status.success(),
        "nmp add component swiftui/component-host --with fixture failed: {}",
        String::from_utf8_lossy(&swift_out.stderr)
    );
    let swift_fixture = swift
        .path()
        .join("Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.swift");
    let swift_body = fs::read_to_string(swift_fixture).unwrap();
    assert!(swift_body.contains("refs.profile"));
    assert!(swift_body.contains("refs.event"));
    assert!(swift_body.contains("refs.event.envelopes"));
    assert!(swift_body.contains("NmpComponentHostConformanceHarness"));

    let compose = TempDir::new("compose-component-host-fixture");
    let compose_out = nmp(
        compose.path(),
        &[
            "add",
            "component",
            "compose/component-host",
            "--with",
            "fixture",
        ],
    );
    assert!(
        compose_out.status.success(),
        "nmp add component compose/component-host --with fixture failed: {}",
        String::from_utf8_lossy(&compose_out.stderr)
    );
    let compose_fixture = compose
        .path()
        .join("Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.kt");
    let compose_body = fs::read_to_string(compose_fixture).unwrap();
    assert!(compose_body.contains("refs.profile"));
    assert!(compose_body.contains("refs.event"));
    assert!(compose_body.contains("refs.event.envelopes"));
    assert!(compose_body.contains("NmpComponentHostConformanceHarness"));
}
