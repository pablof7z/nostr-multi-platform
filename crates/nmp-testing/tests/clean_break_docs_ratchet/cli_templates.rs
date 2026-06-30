use std::path::{Path, PathBuf};

const CLI_TEMPLATE_INPUTS: &[&str] = &[
    "crates/nmp-cli/templates/README.md.tmpl",
    "crates/nmp-cli/templates/app_cargo.toml.tmpl",
    "crates/nmp-cli/templates/lib.rs.tmpl",
    "crates/nmp-cli/templates/nmp.toml.tmpl",
    "crates/nmp-cli/templates/shell.rs.tmpl",
    "crates/nmp-cli/templates/workspace_cargo.toml.tmpl",
];

const FORBIDDEN: &[&str] = &[
    "nmp-defaults",
    "nmp_defaults",
    "default bundle",
    "defaults bundle",
];

#[test]
fn cli_templates_do_not_reintroduce_defaults_bundle_vocabulary() {
    let root = repo_root();
    let mut violations = Vec::new();

    for rel in CLI_TEMPLATE_INPUTS {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|err| panic!("read {rel}: {err}"));
        for token in FORBIDDEN {
            if text.contains(token) {
                violations.push(format!(
                    "{rel}: scaffold template must not contain `{token}`"
                ));
            }
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}
