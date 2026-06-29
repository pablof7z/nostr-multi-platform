//! End-to-end: `nmp init` into a tempdir must produce a thin composition-shell
//! scaffold (ADR-0069) that `cargo check`s green, whose tests pass, and whose
//! `register` shell installs named NMP substrate/protocol/app pieces — NOT a
//! generated FFI crate or a hidden production preset.

mod helpers;

use helpers::{nmp, TempDir};
use std::process::Command;

const STARTER_INSTALLER_SEQUENCE: [&str; 5] = [
    "nmp_defaults::register_substrate",
    "nmp_defaults::register_nip50_protocol_defaults",
    "nmp_defaults::register_social_protocol_defaults",
    "nmp_defaults::register_dm_protocol_defaults",
    "nmp_defaults::register_longform_projection",
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
    assert_named_installer_sequence(&lib);
    assert!(
        !lib.contains("nmp_defaults::register_defaults"),
        "scaffolded production `register` must not call hidden register_defaults:\n{lib}"
    );
    assert!(
        lib.contains("starter_projection_keys")
            && lib.contains("starter_builtin_projection_keys")
            && lib.contains("starter_home_feed_params")
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
            && !lib.contains("register_defaults"),
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
    assert!(
        !root.join("apps").exists(),
        "ADR-0046: init must not scaffold a generated apps/ FFI tree"
    );

    // 3. The scaffold compiles as-is (lib + example + tests). This links
    //    against the local-path `nmp-defaults` / `nmp-ffi` / `nmp-core`
    //    crates, so the whole composition root is type-checked end-to-end.
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
