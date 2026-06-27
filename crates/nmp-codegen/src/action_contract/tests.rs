//! #1939 — fail-closed gates for the neutral action contract.

use super::*;
use crate::action_builders::registry::{ACTION_BUILDERS, PUBLISH_BUILDERS};

#[test]
fn namespaces_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for c in ACTION_CONTRACT {
        assert!(
            seen.insert(c.namespace),
            "duplicate action namespace {:?}",
            c.namespace
        );
    }
}

#[test]
fn schema_files_match_contract_identity() {
    let root = repo_root();
    for c in ACTION_CONTRACT {
        let text = std::fs::read_to_string(root.join(c.schema_path))
            .unwrap_or_else(|err| panic!("read {}: {err}", c.schema_path));
        assert!(
            text.contains(&format!("file_identifier {:?};", c.file_identifier)),
            "{} must declare file_identifier {:?}",
            c.schema_path,
            c.file_identifier
        );
        assert!(
            text.contains(&format!("root_type {};", c.root_type)),
            "{} must declare root_type {}",
            c.schema_path,
            c.root_type
        );
    }
}

#[test]
fn generated_builders_match_contract() {
    let builder_namespaces: std::collections::BTreeSet<&str> =
        ACTION_BUILDERS.iter().map(|b| b.namespace).collect();
    let contract_generated: std::collections::BTreeSet<&str> = ACTION_CONTRACT
        .iter()
        .filter(|c| {
            matches!(
                c.builder_support,
                BuilderSupport::GeneratedFlatTable | BuilderSupport::GeneratedBookmarkItemTable
            )
        })
        .map(|c| c.namespace)
        .collect();
    assert_eq!(
        builder_namespaces, contract_generated,
        "ACTION_BUILDERS must equal contract rows with generated host builders"
    );
    for builder in ACTION_BUILDERS {
        let contract = contract_for(builder.namespace);
        assert!(
            matches!(
                contract.builder_support,
                BuilderSupport::GeneratedFlatTable | BuilderSupport::GeneratedBookmarkItemTable
            ),
            "builder namespace {} has non-generated contract support {:?}",
            builder.namespace,
            contract.builder_support
        );
    }
}

#[test]
fn publish_union_builder_is_contract_declared() {
    let contract = contract_for(PUBLISH_NAMESPACE);
    assert_eq!(
        contract.builder_support,
        BuilderSupport::GeneratedPublishUnion
    );
    assert!(
        !PUBLISH_BUILDERS.is_empty(),
        "publish union contract requires at least one generated publish builder"
    );
}

#[test]
fn typed_exemptions_are_empty_until_explicitly_tracked() {
    assert!(
        typed_dispatch_exemption_namespaces().is_empty(),
        "new JSON-only exemptions must carry a tracked issue in ACTION_CONTRACT"
    );
}

#[test]
fn report_is_compact_and_lists_every_namespace() {
    let report = render_action_contract_report();
    for c in ACTION_CONTRACT {
        assert!(
            report.contains(&format!("`{}`", c.namespace)),
            "report missing {}",
            c.namespace
        );
    }
}

fn repo_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(std::path::Path::parent)
        .expect("nmp-codegen lives under <repo>/crates/nmp-codegen")
        .to_path_buf()
}
