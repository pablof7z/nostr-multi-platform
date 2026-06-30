//! Smoke tests for the nip29_kind_blind ratchet (#2509 / #2513).
//!
//! Stages the per-rule fixtures under a fake `crates/nmp-nip29/src/` tree (the
//! rule's scope) and runs the prebuilt doctrine-lint binary against them:
//!   - the positive fixture (a reintroduced `react_in_group` namespace + a
//!     `REACTION_KIND` constant) must fail with a nip29_kind_blind finding;
//!   - the negative fixture (only allowlisted verbs + a reason-bearing escape
//!     hatch) must be clean.

use super::{fixture_path, run_lint, workspace_root};

fn stage(dir_tag: &str, fixture: &str) -> String {
    let workspace = workspace_root();
    let root = workspace.join("target").join(dir_tag);
    let tmp = root.join("crates").join("nmp-nip29").join("src");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&tmp).expect("create temp nmp-nip29 src dir");
    let src = workspace.join(fixture_path(fixture));
    let file_name = std::path::Path::new(fixture)
        .file_name()
        .expect("fixture has a file name");
    std::fs::copy(&src, tmp.join(file_name)).expect("copy fixture");
    tmp.to_string_lossy().into_owned()
}

#[test]
fn nip29_kind_blind_positive_fixture_fires() {
    let tmp = stage(
        "doctrine_lint_nip29_kind_blind_pos",
        "nip29_kind_blind/pos.rs",
    );
    let (code, stdout, stderr) = run_lint(&["--path", &tmp]);
    assert_eq!(
        code, 1,
        "nip29_kind_blind positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[nip29_kind_blind]"),
        "positive fixture must emit a nip29_kind_blind finding; stdout:\n{}",
        stdout
    );
    // Both the reintroduced per-kind namespace AND the banned authoring constant
    // must be named.
    for token in ["react_in_group", "REACTION_KIND"] {
        assert!(
            stdout.contains(token),
            "nip29_kind_blind positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn nip29_kind_blind_negative_fixture_clean() {
    let tmp = stage(
        "doctrine_lint_nip29_kind_blind_neg",
        "nip29_kind_blind/neg.rs",
    );
    let (code, stdout, stderr) = run_lint(&["--path", &tmp]);
    assert_eq!(
        code, 0,
        "nip29_kind_blind negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[nip29_kind_blind]"),
        "negative fixture must produce zero nip29_kind_blind findings; stdout:\n{}",
        stdout
    );
}
