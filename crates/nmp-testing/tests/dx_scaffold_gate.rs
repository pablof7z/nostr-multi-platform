//! DX Scaffold Gate — empirical developer-experience assertion suite.
//!
//! Turns `docs/aim.md` §1 ("a developer should be able to one-shot a working
//! Nostr application … without ever touching relay routing, cache invalidation,
//! replaceable-event semantics, or subscription lifecycle") and §2 invariant 4
//! ("No native business logic") into `cargo test`-runnable hard gates.
//!
//! Complements `scripts/dx-probe/dx-probe.sh` (which measures wall-clock timing
//! and runs the shell binary) with fast Rust-native assertions that CI can gate on.
//!
//! # Gates
//!
//! | Gate | Assertion                                                       |
//! |------|-----------------------------------------------------------------|
//! | G1   | `nmp init` scaffold compiles (`cargo check --all-targets`)      |
//! | G2   | Scaffold contains zero relay/cache/sub/replaceable-policy LOC   |
//! | G3   | Scaffold drives `NmpAppBuilder` in ≤ 3 developer commands       |
//! | G4   | Shell (examples/shell.rs) contains zero business-logic patterns |
//! | G5   | Adding a typed projection via the intended seam touches 1 file  |
//!
//! # Invocation
//!
//! ```sh
//! cargo test -p nmp-testing --test dx_scaffold_gate
//! ```
//!
//! # Doctrine references
//!
//! - `docs/aim.md` §1  — one-shot claim
//! - `docs/aim.md` §2 inv-4 — No native business logic
//! - `docs/aim.md` §4.14  — scaffolding CLI contract
//! - `docs/aim.md` §6 doctrine — all reads/writes through store/actions
//! - `AGENTS.md` — file-size rules (this file ≤ 300 LOC)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nmp_checkout() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/nmp-testing; repo root is two levels up.
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("nmp-dx-gate-{tag}-{}", std::process::id()));
        fs::create_dir_all(&p).expect("create tempdir");
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn count_policy_loc(content: &str) -> (usize, Vec<String>) {
    // Patterns representing framework-internal concerns that the developer
    // should NEVER touch (aim.md §1).  Any match in the generated scaffold
    // code (not doc-comments) is a DX smell.
    let patterns: &[&str] = &[
        "relay_pool",
        "add_relay(",
        "connect_relay",
        "select_relay",
        "cache_invalidat",
        "prune_cache",
        "subscribe(",
        "register_interest(",
        "replaceable",
    ];

    let mut count = 0;
    let mut hits = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // Skip doc comments and code comments.
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        for pattern in patterns {
            if trimmed.contains(pattern) {
                count += 1;
                hits.push(format!("  line {}: {}", lineno + 1, line));
                break; // count line once even if multiple patterns match
            }
        }
    }
    (count, hits)
}

// ---------------------------------------------------------------------------
// G1: Fresh scaffold compiles (cargo check --all-targets)
// ---------------------------------------------------------------------------

#[test]
fn g1_fresh_scaffold_compiles() {
    let tmp = TempDir::new("g1");
    let nmp = nmp_checkout();
    let app_root = tmp.path().join("dxdemo");
    let pkg = "dxdemo-core";

    // Run `nmp init`.
    let init = Command::new(cargo())
        .args([
            "run",
            "-p",
            "nmp-cli",
            "--manifest-path",
            nmp.join("Cargo.toml").to_str().unwrap(),
            "--",
            "init",
            "dxdemo",
            "--path",
            app_root.to_str().unwrap(),
            "--nmp-path",
            nmp.to_str().unwrap(),
        ])
        .output()
        .expect("run nmp-cli");

    assert!(
        init.status.success(),
        "G1 DX GAP: `nmp init` failed — scaffold was not produced.\n\
         stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&init.stderr),
        String::from_utf8_lossy(&init.stdout),
    );

    // Sanity: files exist.
    assert!(
        app_root.join("crates").join(pkg).join("src").join("lib.rs").exists(),
        "G1 DX GAP: lib.rs missing from scaffold"
    );
    assert!(
        app_root.join("crates").join(pkg).join("examples").join("shell.rs").exists(),
        "G1 DX GAP: shell.rs missing from scaffold"
    );

    // cargo check --all-targets with ZERO developer edits.
    let check = Command::new(cargo())
        .args(["check", "--all-targets"])
        .current_dir(&app_root)
        .output()
        .expect("cargo check");

    assert!(
        check.status.success(),
        "G1 DX GAP: fresh scaffold does not compile.\n\
         This means the developer cannot follow the init → check → run path.\n\
         stderr: {}",
        String::from_utf8_lossy(&check.stderr),
    );
}

// ---------------------------------------------------------------------------
// G2: Zero user-authored policy LOC in scaffold
// ---------------------------------------------------------------------------

#[test]
fn g2_scaffold_has_zero_policy_loc() {
    let tmp = TempDir::new("g2");
    let nmp = nmp_checkout();
    let app_root = tmp.path().join("dxdemo2");
    let pkg = "dxdemo2-core";
    let crate_dir = app_root.join("crates").join(pkg);

    let init = Command::new(cargo())
        .args([
            "run",
            "-p",
            "nmp-cli",
            "--manifest-path",
            nmp.join("Cargo.toml").to_str().unwrap(),
            "--",
            "init",
            "dxdemo2",
            "--path",
            app_root.to_str().unwrap(),
            "--nmp-path",
            nmp.to_str().unwrap(),
        ])
        .output()
        .expect("run nmp-cli");

    assert!(init.status.success(), "nmp init failed");

    let lib_rs = fs::read_to_string(crate_dir.join("src").join("lib.rs"))
        .expect("read lib.rs");
    let shell_rs = fs::read_to_string(crate_dir.join("examples").join("shell.rs"))
        .expect("read shell.rs");

    // Only check shell.rs for policy code — lib.rs contains the skeleton
    // domain stubs (EntryRecord / EntryViewModule / EntryActionModule) which
    // are app-authored and intentionally free of framework-policy code.
    // shell.rs is the direct analogue of the native platform shell.
    let (shell_policy_loc, shell_hits) = count_policy_loc(&shell_rs);
    let (lib_policy_loc, lib_hits) = count_policy_loc(&lib_rs);
    let total_policy = shell_policy_loc + lib_policy_loc;

    assert_eq!(
        total_policy, 0,
        "G2 DX GAP: scaffold contains {total_policy} line(s) of framework-policy code \
         that the developer should NEVER touch (aim.md §1).\n\
         Shell hits:\n{}\nLib hits:\n{}",
        shell_hits.join("\n"),
        lib_hits.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// G3: ADR-0046 composition-shell contract (NmpAppBuilder + register_defaults)
// ---------------------------------------------------------------------------

/// G3 sub-test: the scaffolded shell drives `NmpAppBuilder` — the gateway
/// to "3 commands to running" (init → check → run).
#[test]
fn g3_shell_uses_nmp_app_builder() {
    let tmp = TempDir::new("g3");
    let nmp = nmp_checkout();
    let app_root = tmp.path().join("dxdemo3");
    let pkg = "dxdemo3-core";

    let init = Command::new(cargo())
        .args([
            "run",
            "-p",
            "nmp-cli",
            "--manifest-path",
            nmp.join("Cargo.toml").to_str().unwrap(),
            "--",
            "init",
            "dxdemo3",
            "--path",
            app_root.to_str().unwrap(),
            "--nmp-path",
            nmp.to_str().unwrap(),
        ])
        .output()
        .expect("run nmp-cli");

    assert!(init.status.success(), "nmp init failed");

    let shell_rs = fs::read_to_string(
        app_root.join("crates").join(pkg).join("examples").join("shell.rs"),
    )
    .expect("read shell.rs");

    // G3a: Shell drives NmpAppBuilder (aim.md §4.14 — builder is the gateway).
    assert!(
        shell_rs.contains("NmpAppBuilder"),
        "G3 DX GAP: shell.rs does not use NmpAppBuilder.\n\
         Developers cannot reach the 'cargo run --example shell' step without it.\n\
         shell.rs:\n{shell_rs}",
    );

    // G3b: Shell calls the app-core register() — the composition-root call
    // that wires register_defaults (ADR-0046).
    assert!(
        shell_rs.contains("::register("),
        "G3 DX GAP: shell.rs does not call ::register() — the composition-root\n\
         function that calls nmp_defaults::register_defaults.\n\
         shell.rs:\n{shell_rs}",
    );

    // G3c: .start() is called — confirms the lifecycle advances.
    assert!(
        shell_rs.contains(".start("),
        "G3 DX GAP: shell.rs does not call .start() on the builder.\n\
         The app will not boot without it.\n\
         shell.rs:\n{shell_rs}",
    );

    // G3d: Shell declares the kernel built-in starter projections.
    assert!(
        shell_rs.contains(
            ".declare_consumed_projections(dxdemo3_core::starter_builtin_projection_keys())"
        ),
        "G3 DX GAP: shell.rs does not declare kernel built-in starter projections.\n\
         shell.rs:\n{shell_rs}",
    );

    // G3e: lib.rs calls register_defaults (the canonical NMP composition).
    let lib_rs = fs::read_to_string(
        app_root.join("crates").join(pkg).join("src").join("lib.rs"),
    )
    .expect("read lib.rs");

    assert!(
        lib_rs.contains("nmp_defaults::register_defaults"),
        "G3 DX GAP: lib.rs register() does not call nmp_defaults::register_defaults.\n\
         The scaffold must wire the canonical NMP composition (ADR-0046).\n\
         lib.rs:\n{lib_rs}",
    );

    // G3f: lib.rs teaches current projections and typed write builders.
    for key in [
        "nmp.feed.home",
        "refs.profile",
        "refs.event",
        "refs.event.envelopes",
    ] {
        assert!(
            lib_rs.contains(key),
            "G3 DX GAP: starter projection key `{key}` missing.\nlib.rs:\n{lib_rs}",
        );
    }
    assert!(
        lib_rs.contains("GeneratedActionBuilders.publishRaw")
            && lib_rs.contains("GeneratedActionBuilders.publishReply"),
        "G3 DX GAP: starter must point shells at generated publish builders.\n\
         lib.rs:\n{lib_rs}",
    );
    assert!(
        lib_rs.contains("starter_projection_keys")
            && lib_rs.contains("starter_builtin_projection_keys")
            && lib_rs.contains("starter_home_feed_params"),
        "G3 DX GAP: starter must separate full projection contract from built-in declarations.\n\
         lib.rs:\n{lib_rs}",
    );
    assert!(
        !lib_rs.contains("resolved_profiles") && !lib_rs.contains("claimed_event_embeds"),
        "G3 DX GAP: starter code must not mention legacy projection data sources.\n\
         lib.rs:\n{lib_rs}",
    );
}

// ---------------------------------------------------------------------------
// G4: Thin-shell assertion — no business logic in native shell analogue
// ---------------------------------------------------------------------------

#[test]
fn g4_shell_has_no_business_logic() {
    let tmp = TempDir::new("g4");
    let nmp = nmp_checkout();
    let app_root = tmp.path().join("dxdemo4");
    let pkg = "dxdemo4-core";

    let init = Command::new(cargo())
        .args([
            "run",
            "-p",
            "nmp-cli",
            "--manifest-path",
            nmp.join("Cargo.toml").to_str().unwrap(),
            "--",
            "init",
            "dxdemo4",
            "--path",
            app_root.to_str().unwrap(),
            "--nmp-path",
            nmp.to_str().unwrap(),
        ])
        .output()
        .expect("run nmp-cli");

    assert!(init.status.success(), "nmp init failed");

    let shell_rs = fs::read_to_string(
        app_root.join("crates").join(pkg).join("examples").join("shell.rs"),
    )
    .expect("read shell.rs");

    // Business-logic patterns that belong in Rust core (aim.md §2 inv-4),
    // not in the native shell.  The shell must be: builder → register → start
    // → stop → free.  Nothing else.
    let business_logic_patterns: &[(&str, &str)] = &[
        ("relay_url",      "relay URL selection"),
        ("add_relay(",     "relay pool management"),
        ("subscribe(",     "manual subscription management"),
        ("cache_invalidat","cache invalidation"),
        ("replaceable",    "replaceable-event semantics"),
    ];

    let mut violations = Vec::new();
    for (lineno, line) in shell_rs.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") { continue; }
        for (pattern, label) in business_logic_patterns {
            if trimmed.contains(pattern) {
                violations.push(format!(
                    "  line {}: {label}: {}",
                    lineno + 1,
                    line
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "G4 DX GAP: shell.rs contains {} business-logic line(s) that aim.md §2 inv-4\n\
         says belong in Rust core, not in the native shell.\n\
         Violations:\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// G5: Add-a-feature cost — typed projection via intended seam
// ---------------------------------------------------------------------------

/// Asserts that the scaffold's AppHost seam (`register_typed_snapshot_projection`)
/// is reachable through the composition root — i.e., adding a new typed
/// projection requires changes to exactly 1 file (lib.rs / register() body).
///
/// We don't actually compile a new projection here (that's what
/// `scripts/dx-probe/dx-probe.sh` M4 measures).  We assert the structural
/// precondition: lib.rs already imports the substrate, so an app developer
/// just adds `app.register_typed_snapshot_projection(...)` in register() body.
#[test]
fn g5_add_feature_seam_is_one_file() {
    let tmp = TempDir::new("g5");
    let nmp = nmp_checkout();
    let app_root = tmp.path().join("dxdemo5");
    let pkg = "dxdemo5-core";

    let init = Command::new(cargo())
        .args([
            "run",
            "-p",
            "nmp-cli",
            "--manifest-path",
            nmp.join("Cargo.toml").to_str().unwrap(),
            "--",
            "init",
            "dxdemo5",
            "--path",
            app_root.to_str().unwrap(),
            "--nmp-path",
            nmp.to_str().unwrap(),
        ])
        .output()
        .expect("run nmp-cli");

    assert!(init.status.success(), "nmp init failed");

    let lib_rs = fs::read_to_string(
        app_root.join("crates").join(pkg).join("src").join("lib.rs"),
    )
    .expect("read lib.rs");

    // The `register` function must accept `&mut impl AppHost` — the substrate
    // trait that exposes `register_typed_snapshot_projection`.  If present,
    // an app developer just adds one call inside the function body = 1 file.
    assert!(
        lib_rs.contains("impl AppHost"),
        "G5 DX GAP: register() in lib.rs does not accept AppHost.\n\
         Adding a typed projection requires the AppHost seam to be present.\n\
         lib.rs:\n{lib_rs}",
    );

    // The substrate import must be present so the developer can immediately
    // use AppHost methods without additional imports.
    assert!(
        lib_rs.contains("nmp_core::substrate"),
        "G5 DX GAP: lib.rs does not import nmp_core::substrate.\n\
         An app developer adding a typed projection would need to add this import\n\
         (touching an extra line beyond the register() body).\n\
         lib.rs:\n{lib_rs}",
    );
}
