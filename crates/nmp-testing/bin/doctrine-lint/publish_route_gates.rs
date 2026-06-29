//! Publish route provenance ratchets.
//!
//! These are repository-shape gates rather than syntax rules: #2371 requires
//! explicit publish routes to carry visible provenance and forbids generated app
//! builders from mapping "caller passed relays" to anonymous manual override.

use std::fs;

use super::workspace_root;

fn read(path: &str) -> String {
    let root = workspace_root();
    fs::read_to_string(root.join(path)).unwrap_or_else(|err| panic!("read {path}: {err}"))
}

#[test]
fn publish_route_class_has_no_default_and_no_public_manual_helper() {
    let target = read("crates/nmp-core/src/publish/action/target.rs");
    assert!(
        !target.contains("impl Default for PublishRouteClass"),
        "PublishRouteClass must not default to manual_override"
    );
    assert!(
        !target.contains("fn manual_override("),
        "PublishTarget must not expose a production manual_override helper"
    );
}

#[test]
fn generated_publish_builders_require_named_explicit_route_provenance() {
    for path in [
        "crates/nmp-codegen/tests/fixtures/app_action_builders/generated/ActionBuilders.generated.swift",
        "crates/nmp-codegen/tests/fixtures/app_action_builders/generated/ActionBuilders.kt",
        "web/packages/runtime-web/src/actionBuilders.generated.ts",
    ] {
        let text = read(path);
        assert!(
            text.contains("PublishTargetSelection"),
            "{path} must expose a typed publish target selection"
        );
        assert!(
            !text.contains("relays: [String]? = nil")
                && !text.contains("relays: List<String>? = null")
                && !text.contains("relays: string[] | null = null"),
            "{path} must not accept anonymous optional relay-list publish targets"
        );
        assert!(
            !text.contains("create(string: \"manual_override\")")
                && !text.contains("createString(\"manual_override\")"),
            "{path} must not silently encode manual_override"
        );
    }
}
