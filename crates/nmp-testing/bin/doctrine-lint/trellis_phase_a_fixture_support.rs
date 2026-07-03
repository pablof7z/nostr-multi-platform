//! Receipt-render fixture for #2858 Trellis Phase A gates.

use std::path::Path;
use std::process::Command;

const RECEIPT_FIXTURE_MAIN: &str = r#"
use nmp_devtools::{
    receipts_from_trellis_commands, TrellisReceiptPayload, XrayInterestDescriptor,
    XrayProjectionContext, XrayReason, XrayReasonCode, XrayTimestamp,
    XrayTransactionMarker,
};
use trellis_core::{Graph, ResourceKey, ResourcePlan};

#[derive(Clone)]
struct DemoCommand(&'static str);

impl TrellisReceiptPayload for DemoCommand {
    fn interest_descriptor(&self) -> Option<XrayInterestDescriptor> {
        Some(XrayInterestDescriptor::new(
            format!("interest:{}", self.0),
            "home-feed",
            format!("authors={}", self.0),
            "active-follow-timeline",
        ))
    }
}

fn main() {
    let mut graph = Graph::<DemoCommand>::new_with_command_type();
    let mut tx = graph.begin_transaction().unwrap();
    let scope = tx.create_scope("home-feed").unwrap();
    let mut plan = ResourcePlan::new();
    plan.open(
        ResourceKey::new("profile-feed:alice".to_string()),
        scope,
        DemoCommand("alice"),
    );
    plan.close(ResourceKey::new("profile-feed:bob".to_string()), scope);

    let context = XrayProjectionContext::new(
        "app.feed.home",
        "home-feed",
        "owner:timeline",
        XrayReason::new(XrayReasonCode::FeedSessionSync),
    );
    let receipts = receipts_from_trellis_commands(
        &context,
        XrayTransactionMarker::new(41, 7),
        XrayTimestamp::new(1_777_000_000_000),
        plan.commands(),
    );
    let rendered = serde_json::to_string_pretty(&receipts).unwrap();

    for forbidden in ["trellis", "ResourcePlan", "ResourceKey", "NodeId("] {
        if rendered.to_ascii_lowercase().contains(&forbidden.to_ascii_lowercase()) {
            panic!("rendered receipt leaked {forbidden}: {rendered}");
        }
    }
}
"#;

pub(crate) fn run_receipt_render_fixture(root: &Path) {
    let fixture = root.join("target/doctrine-lint/devtools-receipt-render-fixture");
    let target = root.join("target/doctrine-lint/devtools-receipt-render-target");
    write_receipt_fixture(root, &fixture);

    let manifest = fixture.join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", &target)
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("receipt render fixture must spawn cargo");

    assert!(
        output.status.success(),
        "receipt render fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_receipt_fixture(root: &Path, fixture: &Path) {
    let _ = std::fs::remove_dir_all(fixture);
    std::fs::create_dir_all(fixture.join("src")).expect("create receipt fixture");
    std::fs::write(fixture.join("Cargo.toml"), receipt_fixture_manifest(root))
        .expect("write receipt fixture manifest");
    std::fs::write(fixture.join("src/main.rs"), RECEIPT_FIXTURE_MAIN)
        .expect("write receipt fixture main");
}

fn receipt_fixture_manifest(root: &Path) -> String {
    format!(
        r#"[package]
name = "nmp-devtools-render-fixture"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
nmp-devtools = {{ path = "{}/crates/nmp-devtools" }}
serde_json = "1"
trellis-core = "0.2.1"

[workspace]
"#,
        root.display()
    )
}
