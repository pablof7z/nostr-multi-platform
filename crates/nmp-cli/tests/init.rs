//! End-to-end: `nmp init` into a tempdir must produce a thin composition-shell
//! scaffold (ADR-0069 + #2720) that `cargo check`s green, whose tests pass, and
//! whose `register` shell installs named NMP substrate/protocol/app pieces plus
//! one app-owned UniFFI facade and typed action doorway.

mod helpers;

use helpers::{nmp, TempDir};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const STARTER_INSTALLER_SEQUENCE: [&str; 13] = [
    "nmp_substrate::install",
    "nmp_nip50::register",
    "nmp_nip02::register",
    "nmp_replies::register",
    "nmp_nip25::register",
    "nmp_nip18::register",
    "nmp_nip84::register",
    "nmp_nip29::register",
    "nmp_wot::register",
    "nmp_nip51::register",
    "nmp_nip17::register",
    "nmp_nip22::register",
    "nmp_nip23::register",
];

#[test]
fn init_scaffold_is_a_compiling_composition_shell() {
    let tmp = TempDir::new("init");
    let root = tmp.path().join("demoapp");

    // 1. Scaffold.
    let out = nmp(
        tmp.path(),
        &["init", "demoapp", "--path", root.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "nmp init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(root.join("nmp.toml").exists());
    assert!(root.join("action-builders.json").exists());
    assert!(root
        .join("generated/ActionBuilders.generated.swift")
        .exists());
    assert!(root.join("generated/ActionBuilders.kt").exists());
    assert!(root.join("generated/actionBuilders.generated.ts").exists());
    assert!(root.join("crates/demoapp-core/src/lib.rs").exists());
    assert!(root
        .join("crates/demoapp-core/src/entry_action.rs")
        .exists());
    assert!(root.join("crates/demoapp-core/src/entry_view.rs").exists());
    assert!(root
        .join("crates/demoapp-core/schema/add_entry.fbs")
        .exists());
    assert!(root.join("crates/demoapp-app/src/lib.rs").exists());
    assert!(root.join("crates/demoapp-core/examples/shell.rs").exists());
    assert!(root.join("ci/check-uniffi-bindings.sh").exists());

    // 2. ADR-0069: production composition is explicit Rust, not a hidden
    //    preset. The scaffolded `register` shell installs the reusable
    //    substrate by name, the headless example drives it through
    //    `NmpAppBuilder`, and the native doorway is exactly one app-owned
    //    UniFFI facade crate, not raw C symbols or `nmp gen modules` output.
    let lib = std::fs::read_to_string(root.join("crates/demoapp-core/src/lib.rs"))
        .expect("read scaffolded lib.rs");
    let entry_action =
        std::fs::read_to_string(root.join("crates/demoapp-core/src/entry_action.rs"))
            .expect("read scaffolded entry_action.rs");
    let entry_view = std::fs::read_to_string(root.join("crates/demoapp-core/src/entry_view.rs"))
        .expect("read scaffolded entry_view.rs");
    let facade = std::fs::read_to_string(root.join("crates/demoapp-app/src/lib.rs"))
        .expect("read scaffolded facade lib.rs");
    let uniffi_check = std::fs::read_to_string(root.join("ci/check-uniffi-bindings.sh"))
        .expect("read scaffolded UniFFI binding check");
    let root_cargo_toml = std::fs::read_to_string(root.join("Cargo.toml"))
        .expect("read scaffolded workspace Cargo.toml");
    let cargo_toml = std::fs::read_to_string(root.join("crates/demoapp-core/Cargo.toml"))
        .expect("read scaffolded Cargo.toml");
    assert_named_installer_sequence(&lib);
    assert!(
        !lib.contains("nmp_defaults::register_defaults")
            && !lib.contains("nmp_defaults")
            && !lib.contains("nmp-defaults"),
        "scaffolded production `register` must not use nmp-defaults:\n{lib}"
    );
    assert!(
        !cargo_toml.contains("\nnmp-defaults =")
            && !cargo_toml.contains("package = \"nmp-defaults\"")
            && cargo_toml.contains("nmp-substrate")
            && cargo_toml.contains("nmp-nip50")
            && cargo_toml.contains("nmp-nip51")
            && cargo_toml.contains("nmp-nip17")
            && cargo_toml.contains("nmp-nip23")
            && cargo_toml.contains("nmp-content")
            && cargo_toml.contains("nmp-signer-iface")
            && cargo_toml.contains("flatbuffers"),
        "scaffolded Cargo.toml must depend on explicit owner crates, not nmp-defaults:\n{cargo_toml}"
    );
    assert!(
        root_cargo_toml.contains("\"crates/demoapp-core\"")
            && root_cargo_toml.contains("\"crates/demoapp-app\""),
        "workspace must include both the core and app-owned UniFFI facade crates:\n{root_cargo_toml}"
    );
    assert!(
        lib.contains("starter_projection_keys")
            && lib.contains("starter_builtin_projection_keys")
            && lib.contains("starter_home_feed_key")
            && lib.contains("starter_home_feed_spec")
            && lib.contains("nmp_native_runtime::feed::events()")
            && lib.contains("nmp_native_runtime::source::active_user().follows()")
            && lib.contains(".open_spec(starter_home_feed_key(), starter_home_feed_spec())")
            && lib.contains("HomeTimelineSession")
            && lib.contains("open_home_timeline_session")
            && lib.contains("close_home_timeline_session")
            && lib.contains("\"demoapp.timeline.home\"")
            && lib.contains("\"refs.profile\"")
            && lib.contains("\"refs.event\"")
            && lib.contains("\"refs.event.envelopes\"")
            && lib.contains("\"publish_outbox\""),
        "scaffolded starter must declare current v1 profile/content projections:\n{lib}"
    );
    assert!(
        !lib.contains("GeneratedActionBuilders.publishRaw")
            && !lib.contains("GeneratedActionBuilders.publishReply")
            && !lib.contains("GeneratedActionBuilders.publishProfile")
            && lib.contains("GeneratedActionBuilders.addEntry"),
        "scaffolded starter must point shells at the app-local generated builder, not generic publishRaw or built-in publish builders:\n{lib}"
    );
    assert!(
        entry_action.contains("impl ActionPayload for AddEntryAction")
            && entry_action.contains("fn decode_payload")
            && entry_action.contains("DeclaredActionNamespace::app_owned(ACTION_NAMESPACE)")
            && entry_action.contains("ActorCommand::Publish")
            && entry_action.contains("EVENT_KIND: u32 = 30445"),
        "scaffolded starter action must decode generated bytes into an app-owned ActionModule:\n{entry_action}"
    );
    assert!(
        lib.contains("pub mod entry_view")
            && entry_view.contains("pub fn dependencies")
            && entry_view.contains("kinds: vec![EVENT_KIND]")
            && entry_view.contains("ENTRY_VIEW_LIMIT")
            && entry_view.contains("pub fn on_event_inserted")
            && entry_view.contains("pub fn on_event_replaced")
            && entry_view.contains("pub fn on_event_removed")
            && entry_action.contains("published entry event must update the app-owned view"),
        "scaffolded starter view must react to the app-private event emitted by the action:\nlib.rs:\n{lib}\nentry_view.rs:\n{entry_view}\nentry_action.rs:\n{entry_action}"
    );
    assert!(
        facade.contains("uniffi::setup_scaffolding!()")
            && facade.contains("struct DemoappApp")
            && facade.contains("nmp_uniffi_support::dispatch_action_vec")
            && facade.contains("nmp_uniffi_support::set_update_sink")
            && facade.contains("nmp_uniffi_support::set_capability_callback")
            && !facade.contains("#[no_mangle]")
            && !facade.contains("extern \"C\""),
        "scaffolded app facade must be app-owned UniFFI over nmp-uniffi-support, not raw C glue:\n{facade}"
    );
    assert!(
        uniffi_check.contains("cargo build -p \"${FACADE_PKG}\"")
            && uniffi_check.contains("uniffi-bindgen")
            && uniffi_check.contains("generate --library")
            && uniffi_check.contains("--language swift")
            && uniffi_check.contains("--language kotlin")
            && uniffi_check.contains("FACADE_PKG=\"demoapp-app\"")
            && uniffi_check.contains("FACADE_CRATE=\"demoapp_app\""),
        "scaffolded app facade must include a Swift/Kotlin UniFFI binding check:\n{uniffi_check}"
    );
    assert!(
        !lib.contains("open_interest")
            && !lib.contains("ObservedProjection")
            && !lib.contains("register_defaults")
            && !lib.contains("compile_feed_params"),
        "scaffolded app API must not expose raw read internals:\n{lib}"
    );
    let legacy_embed_projection_key = ["claimed_event", "embeds"].join("_");
    assert!(
        !lib.contains("resolved_profiles") && !lib.contains(&legacy_embed_projection_key),
        "scaffolded starter must not teach legacy projection names:\n{lib}"
    );
    let shell = std::fs::read_to_string(root.join("crates/demoapp-core/examples/shell.rs"))
        .expect("read scaffolded shell.rs");
    assert!(
        shell.contains("NmpAppBuilder") && shell.contains("::register("),
        "scaffolded shell must build via NmpAppBuilder and call register:\n{shell}"
    );
    assert!(
        shell.contains("open_home_timeline_session")
            && shell.contains("close_home_timeline_session")
            && !shell.contains("compile_feed_params")
            && !shell.contains("open_interest"),
        "scaffolded shell must use the app-owned read helper, not raw read internals:\n{shell}"
    );
    assert!(
        shell.contains(
            ".declare_consumed_projections(demoapp_core::starter_builtin_projection_keys())"
        ),
        "scaffolded shell must declare kernel built-in starter projections:\n{shell}"
    );
    assert_no_retired_app_surface(&lib, &shell);
    assert!(
        !root.join("apps").exists(),
        "init must not scaffold a generated apps/ tree"
    );

    // 3. The scaffold compiles as-is (lib + example + tests). This links
    //    against local-path owner crates, so the whole composition root is
    //    type-checked end-to-end.
    let check = Command::new(env!("CARGO"))
        .args(["check", "--all-targets"])
        .current_dir(&root)
        .output()
        .expect("run cargo check");
    assert!(
        check.status.success(),
        "scaffold failed cargo check:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // 4. Skeleton tests pass for both the core and facade crates.
    let test = Command::new(env!("CARGO"))
        .args(["test", "-p", "demoapp-core"])
        .current_dir(&root)
        .output()
        .expect("run cargo test");
    assert!(
        test.status.success(),
        "scaffold tests failed:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );
    let facade_test = Command::new(env!("CARGO"))
        .args(["test", "-p", "demoapp-app"])
        .current_dir(&root)
        .output()
        .expect("run cargo test");
    assert!(
        facade_test.status.success(),
        "scaffold facade tests failed:\n{}",
        String::from_utf8_lossy(&facade_test.stderr)
    );

    // 5. The app-local action-builder registry is already generated and the
    //    normal NMP codegen drift gate sees it as current.
    let repo_manifest = repo_root().join("Cargo.toml");
    let registry_path = root.join("action-builders.json");
    let drift = Command::new(env!("CARGO"))
        .args([
            "run",
            "--manifest-path",
            repo_manifest.to_str().unwrap(),
            "-p",
            "nmp-codegen",
            "--",
            "gen",
            "action-builders",
            "--registry",
            registry_path.to_str().unwrap(),
            "--check",
        ])
        .output()
        .expect("run app-local action-builder drift gate");
    assert!(
        drift.status.success(),
        "scaffold action-builder drift gate failed:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&drift.stderr),
        String::from_utf8_lossy(&drift.stdout)
    );

    // 6. The scaffolded app-owned facade can generate real Swift/Kotlin UniFFI
    //    bindings from its cdylib, proving the native doorway is not just a
    //    compile-only stub.
    let bindings = Command::new("bash")
        .args(["ci/check-uniffi-bindings.sh"])
        .current_dir(&root)
        .output()
        .expect("run scaffold UniFFI binding check");
    assert!(
        bindings.status.success(),
        "scaffold UniFFI binding check failed:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&bindings.stderr),
        String::from_utf8_lossy(&bindings.stdout)
    );
    assert!(
        root.join("generated/uniffi/swift").exists()
            && root.join("generated/uniffi/kotlin").exists(),
        "scaffold UniFFI binding check must generate Swift/Kotlin output"
    );

    // 7. The documented headless shell path runs through start/open/close/stop.
    let run = run_shell_with_timeout(&root, "demoapp-core");
    assert!(
        run.status.success(),
        "scaffold shell failed:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("NmpAppBuilder"),
        "scaffold shell did not report the documented builder path:\n{}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn init_rejects_invalid_names() {
    let tmp = TempDir::new("reject");
    for bad in ["Demo", "1app", "my--app", "my_app", "app-"] {
        let out = nmp(
            tmp.path(),
            &[
                "init",
                bad,
                "--path",
                tmp.path().join("x").to_str().unwrap(),
            ],
        );
        assert!(!out.status.success(), "expected `{bad}` to be rejected");
    }
}

fn assert_named_installer_sequence(lib: &str) {
    let mut previous = 0;
    for installer in STARTER_INSTALLER_SEQUENCE {
        let index = lib[previous..]
            .find(installer)
            .map(|offset| previous + offset)
            .unwrap_or_else(|| {
                panic!(
                    "scaffolded `register` must install `{installer}` explicitly in the named starter sequence:\n{lib}"
                )
            });
        previous = index + installer.len();
    }
}

fn assert_no_retired_app_surface(lib: &str, shell: &str) {
    let legacy_embed_projection_key = ["claimed_event", "embeds"].join("_");
    let retired_home_feed_key = ["nmp", "feed", "home"].join(".");
    for forbidden in [
        "nmp-defaults",
        "nmp_defaults",
        "register_defaults",
        "open_interest",
        "ObservedProjection",
        "ReducedSource",
        "PublishRaw",
        "publishRaw",
        &retired_home_feed_key,
        "resolved_profiles",
        &legacy_embed_projection_key,
    ] {
        assert!(
            !lib.contains(forbidden) && !shell.contains(forbidden),
            "scaffold must not expose retired app-facing vocabulary `{forbidden}`:\n\
             lib.rs:\n{lib}\nshell.rs:\n{shell}",
        );
    }
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run_shell_with_timeout(root: &std::path::Path, pkg: &str) -> Output {
    let child = Command::new(env!("CARGO"))
        .args(["run", "--example", "shell", "-p", pkg])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cargo run shell");
    let child_id = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(output) => output.expect("wait for cargo run shell"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = Command::new("kill")
                .args(["-TERM", &child_id.to_string()])
                .status();
            let _ = rx.recv_timeout(Duration::from_secs(5));
            panic!("scaffold shell did not complete within 30s");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("scaffold shell waiter disconnected")
        }
    }
}
