//! End-to-end: `nmp init` into a tempdir must produce a thin composition-shell
//! scaffold (ADR-0069) that `cargo check`s green, whose tests pass, and whose
//! `register` shell installs named NMP substrate/protocol/app pieces — NOT a
//! generated FFI crate or a hidden production preset.

mod helpers;

use helpers::{nmp, TempDir};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const STARTER_INSTALLER_SEQUENCE: [&str; 19] = [
    "nmp_substrate::install",
    "nmp_nip50::register_search_scopes",
    "nmp_nip50::register_input_scopes",
    "nmp_nip02::register_follow_actions",
    "nmp_replies::register_actions",
    "nmp_nip25::Nip25Descriptor",
    "nmp_nip18::Nip18Descriptor",
    "nmp_nip84::Nip84Descriptor",
    "nmp_nip29::register_input_scopes",
    "nmp_wot::register_runtime",
    "nmp_nip51::register_mute_runtime",
    "nmp_nip51::register_bookmark_runtime",
    "nmp_nip51::register_bookmark_set_runtime",
    "nmp_nip51::register_web_bookmark_runtime",
    "nmp_nip51::register_search_relay_runtime_with_fallbacks",
    "nmp_nip17::register_actions",
    "nmp_nip17::register_runtime",
    "nmp_nip22::register_runtime",
    "nmp_nip23::register_longform_projection",
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
    assert!(root.join("crates/demoapp-core/src/lib.rs").exists());
    assert!(root.join("crates/demoapp-core/examples/shell.rs").exists());

    // 2. ADR-0069: production composition is explicit Rust, not a hidden
    //    preset. The scaffolded `register` shell installs the reusable
    //    substrate by name, the headless example drives it through
    //    `NmpAppBuilder`, and there is NO generated `apps/` FFI tree and NO
    //    `nmp gen modules` step.
    let lib = std::fs::read_to_string(root.join("crates/demoapp-core/src/lib.rs"))
        .expect("read scaffolded lib.rs");
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
            && cargo_toml.contains("nmp-content"),
        "scaffolded Cargo.toml must depend on explicit owner crates, not nmp-defaults:\n{cargo_toml}"
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
            && lib.contains("GeneratedActionBuilders.publishReply")
            && lib.contains("GeneratedActionBuilders.publishProfile"),
        "scaffolded starter must point shells at typed generated publish builders, not generic publishRaw:\n{lib}"
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
        "ADR-0046: init must not scaffold a generated apps/ FFI tree"
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

    // 4. Skeleton tests pass.
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

    // 5. The documented headless shell path runs through start/open/close/stop.
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
