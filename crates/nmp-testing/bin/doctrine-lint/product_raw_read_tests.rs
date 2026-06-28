//! Smoke tests for the product raw-read/session ratchet.

use super::{fixture_path, run_lint, workspace_root};

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
