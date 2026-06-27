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
    let output = Command::new(env!("CARGO_BIN_EXE_doctrine-lint"))
        .current_dir(&root)
        .args(args)
        .output()
        .expect("doctrine-lint binary must spawn");
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
fn native_transport_dispatch_api_fires() {
    let root = fake_native_root(
        "doctrine_native_transport_dispatch",
        &[(
            "apps/chirp/android/app/src/main/java/example/Wallet.kt",
            "fun connect(bridge: Bridge, json: String) {\n    bridge.dispatchActionBytes(\"nmp.wallet.connect\", json)\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains("dispatchActionBytes"));
}

#[test]
fn native_swift_raw_dispatch_api_fires() {
    let root = fake_native_root(
        "doctrine_native_swift_raw_dispatch",
        &[(
            "apps/chirp/ios/Chirp/Bridge/LeakyBridge.swift",
            "func join(json: String) {\n    dispatchRawActionBytes(namespace: \"nmp.nip29.join\", bodyJson: json)\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 1, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(stdout.contains("error[D18]") && stdout.contains("dispatchRawActionBytes"));
}

#[test]
fn native_typed_action_wrapper_is_allowed() {
    let root = fake_native_root(
        "doctrine_native_typed_action_wrapper",
        &[(
            "apps/chirp/android/app/src/main/java/example/Wallet.kt",
            "fun connect(model: Model, uri: String) {\n    model.dispatchWalletConnect(uri)\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
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
fn native_pure_bytes_dispatch_not_flagged() {
    // A plain `dispatchBytes(builderBytes)` call has no namespace literal —
    // it is the sanctioned hand-written wrapper name and must NOT fire D18.
    let root = fake_native_root(
        "doctrine_native_pure_bytes_dispatch",
        &[(
            "apps/chirp/android/app/src/main/java/example/Relay.kt",
            "fun publishRelayList(bridge: KernelBridge, bytes: ByteArray) {\n    return bridge.dispatchBytes(builderBytes)\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(
        !stdout.contains("error[D18]"),
        "pure-bytes dispatch must not fire D18; stdout:\n{}",
        stdout
    );
}

#[test]
fn native_uniffi_doorway_call_not_flagged() {
    // `appHandle?.dispatchActionBytes(bytes)` is the UniFFI doorway call — no
    // namespace literal on the line.  It must NOT be flagged by D18.
    let root = fake_native_root(
        "doctrine_native_uniffi_doorway",
        &[(
            "apps/chirp/android/app/src/main/java/example/KernelBridge.kt",
            "internal fun dispatchBytes(bytes: ByteArray): DispatchResult {\n    val ack = appHandle?.dispatchActionBytes(bytes)\n        ?: return DispatchResult.Failure(\"null\")\n    return DispatchResult.fromAck(ack)\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        run_lint(&["--workspace-native", "--workspace-native-root", &root_str]);
    assert_eq!(code, 0, "stdout:\n{}\nstderr:\n{}", stdout, stderr);
    assert!(
        !stdout.contains("error[D18]"),
        "UniFFI doorway call (no namespace literal) must not fire D18; stdout:\n{}",
        stdout
    );
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
