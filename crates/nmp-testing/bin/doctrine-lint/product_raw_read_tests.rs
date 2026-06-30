//! Smoke tests for the product raw-read/session ratchet.

use super::{fixture_path, run_lint, workspace_root};
use std::path::Path;

#[path = "rules/product_raw_read.rs"]
mod product_raw_read;

#[test]
fn product_raw_read_positive_fixture_fires() {
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_product_raw_read_pos")
        .join("apps")
        .join("demo")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_product_raw_read_pos"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake app src dir");
    let pos_src = workspace.join(fixture_path("product_raw_read/pos.rs"));
    std::fs::copy(&pos_src, crate_src.join("main.rs")).expect("copy positive fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "product_raw_read positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[product_raw_read]"),
        "positive fixture must emit product_raw_read finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn product_raw_read_negative_fixture_is_clean() {
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_product_raw_read_neg")
        .join("apps")
        .join("demo")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_product_raw_read_neg"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake app src dir");
    let neg_src = workspace.join(fixture_path("product_raw_read/neg.rs"));
    std::fs::copy(&neg_src, crate_src.join("main.rs")).expect("copy negative fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 0,
        "product_raw_read negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[product_raw_read]"),
        "negative fixture must produce no product_raw_read finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn starter_templates_remain_product_raw_read_clean() {
    let workspace = workspace_root();
    let templates = workspace.join("crates/nmp-cli/templates");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&templates).expect("read nmp-cli templates") {
        let entry = entry.expect("read template entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".rs.tmpl") {
            continue;
        }
        assert!(
            product_raw_read::file_in_scope(&path),
            "template must be in product_raw_read scope: {}",
            path.display()
        );
        let body = std::fs::read_to_string(&path).expect("read template body");
        for (idx, line) in body.lines().enumerate() {
            let is_comment =
                line.trim_start().starts_with("//") || line.trim_start().starts_with("//!");
            let hits = product_raw_read::check(line, is_comment, false);
            assert!(
                hits.is_empty(),
                "{}:{} must not use raw read/session hooks: {:?}",
                path.display(),
                idx + 1,
                hits
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "expected at least one Rust starter template");
}

#[test]
fn native_runtime_nmp_app_does_not_expose_raw_interest_methods() {
    let workspace = workspace_root();
    let runtime_src = workspace.join("crates/nmp-native-runtime/src");
    let mut checked_impls = 0usize;

    for entry in std::fs::read_dir(&runtime_src).expect("read native-runtime src") {
        let entry = entry.expect("read native-runtime src entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        checked_impls += count_nmp_app_impls_without_raw_interest_public_methods(&path);
    }

    assert!(
        checked_impls > 0,
        "expected to scan native NmpApp impl blocks"
    );
}

fn count_nmp_app_impls_without_raw_interest_public_methods(path: &Path) -> usize {
    let body = std::fs::read_to_string(path).expect("read native-runtime source");
    let mut in_nmp_app_impl = false;
    let mut brace_depth = 0isize;
    let mut checked_impls = 0usize;

    for (idx, line) in body.lines().enumerate() {
        if !in_nmp_app_impl && line.trim_start().starts_with("impl NmpApp") {
            in_nmp_app_impl = true;
            brace_depth = 0;
            checked_impls += 1;
        }

        if in_nmp_app_impl {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("pub fn open_interest("),
                "{}:{} must not expose public NmpApp::open_interest",
                path.display(),
                idx + 1
            );
            assert!(
                !trimmed.starts_with("pub fn close_interest("),
                "{}:{} must not expose public NmpApp::close_interest",
                path.display(),
                idx + 1
            );

            brace_depth += line.matches('{').count() as isize;
            brace_depth -= line.matches('}').count() as isize;
            if brace_depth == 0 {
                in_nmp_app_impl = false;
            }
        }
    }

    checked_impls
}

#[test]
fn production_starter_rejects_hidden_register_defaults_preset() {
    let workspace = workspace_root();
    let banned = "register_defaults";
    let production_files = [
        "crates/nmp-cli/src/init.rs",
        "crates/nmp-cli/src/main.rs",
        "crates/nmp-cli/templates/README.md.tmpl",
        "crates/nmp-cli/templates/app_cargo.toml.tmpl",
        "crates/nmp-cli/templates/lib.rs.tmpl",
        "crates/nmp-cli/templates/shell.rs.tmpl",
        "crates/nmp-cli/templates/workspace_cargo.toml.tmpl",
        "docs/cli.md",
    ];

    for relative in production_files {
        let path = workspace.join(relative);
        let body = std::fs::read_to_string(&path).expect("read starter file");
        assert!(
            !body.contains(banned),
            "{} must not teach `{}` as the production starter path",
            relative,
            banned
        );
    }

    let scaffold_test = std::fs::read_to_string(workspace.join("crates/nmp-cli/tests/init.rs"))
        .expect("read init scaffold test");
    assert!(
        scaffold_test.contains("!lib.contains(\"nmp_defaults::register_defaults\")"),
        "init scaffold test must keep a negative guard against register_defaults"
    );

    let dx_gate =
        std::fs::read_to_string(workspace.join("crates/nmp-testing/tests/dx_scaffold_gate.rs"))
            .expect("read dx scaffold gate");
    assert!(
        dx_gate.contains("!lib_rs.contains(\"nmp_defaults::register_defaults\")"),
        "DX scaffold gate must keep a negative guard against register_defaults"
    );
}
