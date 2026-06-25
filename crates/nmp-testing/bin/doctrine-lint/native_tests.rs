use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("workspace root must exist two levels above CARGO_MANIFEST_DIR")
}

fn run_lint(args: &[&str]) -> (i32, String, String) {
    let root = workspace_root();
    let output = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "run",
            "--quiet",
            "-p",
            "nmp-testing",
            "--bin",
            "doctrine-lint",
            "--",
        ])
        .args(args)
        .output()
        .expect("cargo run must spawn");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn fake_native_root(label: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = workspace_root().join("target").join(label);
    let _ = std::fs::remove_dir_all(&root);
    for (rel, body) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("create parent");
        std::fs::write(path, body).expect("write fixture");
    }
    root
}

#[test]
fn native_looped_sleep_fires() {
    let root = fake_native_root(
        "doctrine_native_looped_sleep",
        &[(
            "ios/App/Poller.swift",
            "func poll() async {\n    while true {\n        try? await Task.sleep(for: .seconds(1))\n    }\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains("Task.sleep"));
}

#[test]
fn native_one_shot_sleep_is_not_polling() {
    let root = fake_native_root(
        "doctrine_native_one_shot_sleep",
        &[(
            "ios/App/Toast.swift",
            "func clearLater() async {\n    try? await Task.sleep(for: .seconds(2))\n    clearToast()\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
}

#[test]
fn native_publish_policy_envelope_fires() {
    let root = fake_native_root(
        "doctrine_native_publish_policy",
        &[(
            "apps/chirp/android/app/src/main/java/example/Composer.kt",
            "fun publish(bridge: Bridge) {\n    bridge.dispatchAction(\"nmp.publish\", \"\"\"{\"PublishRaw\":{\"kind\":1,\"tags\":[],\"content\":\"hi\",\"target\":\"Auto\"}}\"\"\")\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains("PublishRaw"));
}

#[test]
fn native_scheduled_timer_fires() {
    let root = fake_native_root(
        "doctrine_native_timer",
        &[(
            "ios/App/TimerPoller.swift",
            "func start() {\n    Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in refresh() }\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains(".scheduledTimer"));
}

#[test]
fn native_lifecycle_leak_marker_fires() {
    let root = fake_native_root(
        "doctrine_native_leak_marker",
        &[(
            "ios/App/Lifecycle.swift",
            "func close() {\n    // small bounded leak until the next screen mount\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains("small bounded leak"));
}

#[test]
fn live_workspace_native_doctrine_is_clean_with_allowlist() {
    let (code, stdout, stderr) = run_lint(&["--workspace-native"]);
    assert_eq!(
        code, 0,
        "workspace native doctrine must be clean; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}
