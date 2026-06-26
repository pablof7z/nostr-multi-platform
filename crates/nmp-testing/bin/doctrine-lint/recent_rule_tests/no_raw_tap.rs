use super::{fixture_path, run_lint, workspace_root};

fn staged_src(name: &str, root: &str) -> std::path::PathBuf {
    let workspace = workspace_root();
    let base = workspace.join("target").join(name);
    let _ = std::fs::remove_dir_all(&base);
    let src = base.join(root).join("src");
    std::fs::create_dir_all(&src).expect("create staged source dir");
    src
}

#[test]
fn positive_fixture_fires() {
    let workspace = workspace_root();
    let src = staged_src("doctrine_lint_no_raw_tap_pos", "crates/nmp-fake-crate");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos.rs"));
    std::fs::copy(&pos_src, src.join("pos.rs")).expect("copy pos fixture");

    let (code, stdout, stderr) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(
        code, 1,
        "no_raw_tap positive must exit 1; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap positive must emit >=1 finding; stdout:\n{stdout}"
    );
    for token in [
        "RawEventObserver",
        "KernelEventObserver",
        "register_live_event_tap",
        "NmpEventObserverCallback",
        "nmp_app_register_event_observer",
    ] {
        assert!(
            stdout.contains(token),
            "no_raw_tap finding must name {token}; stdout:\n{stdout}"
        );
    }
}

#[test]
fn bare_allow_does_not_silence() {
    let src = staged_src("doctrine_lint_no_raw_tap_bare", "crates/fake");
    std::fs::write(
        src.join("bare.rs"),
        "fn w() { app.register_raw_event_observer(f, o); } // doctrine-allow: no_raw_tap\n",
    )
    .expect("write fixture");
    let (code, stdout, _) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(
        code, 1,
        "bare allow must NOT silence no_raw_tap; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "finding must still fire; stdout:\n{stdout}"
    );
}

#[test]
fn negative_fixture_clean() {
    let workspace = workspace_root();
    let src = staged_src("doctrine_lint_no_raw_tap_neg", "crates/nmp-fake-crate");
    let neg_src = workspace.join(fixture_path("no_raw_tap/neg.rs"));
    std::fs::copy(&neg_src, src.join("neg.rs")).expect("copy neg fixture");

    let (code, stdout, stderr) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(
        code, 0,
        "no_raw_tap negative must exit 0; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("error[no_raw_tap]"),
        "no_raw_tap negative must produce zero findings; stdout:\n{stdout}"
    );
}

#[test]
fn class_fixture_fires_without_named_token() {
    let workspace = workspace_root();
    let src = staged_src("doctrine_lint_no_raw_tap_class", "crates/nmp-fake-crate");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos_class.rs"));
    std::fs::copy(&pos_src, src.join("pos_class.rs")).expect("copy class fixture");

    let (code, stdout, stderr) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(
        code, 1,
        "no_raw_tap CLASS positive must exit 1; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap CLASS positive must emit a finding; stdout:\n{stdout}"
    );
}

#[test]
fn covers_apps_scope() {
    let workspace = workspace_root();
    let src = staged_src("doctrine_lint_no_raw_tap_apps", "apps/fake-app");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos.rs"));
    std::fs::copy(&pos_src, src.join("pos.rs")).expect("copy pos fixture");

    let (code, stdout, stderr) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(
        code, 1,
        "no_raw_tap must scan apps/ and exit 1; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap must flag a banned token under apps/; stdout:\n{stdout}"
    );
}
