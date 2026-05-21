//! Contract tests for the generated `FfiApp` (`src/ffi.rs`).
//!
//! The generated `FfiApp` routes one `AppAction` through NMP's live
//! extensibility seams:
//!
//! * `AppAction::Kernel(_)` reduces through the public
//!   `nmp_core::KernelReducer` — behaviorally equivalent to the hand-written
//!   `dispatch_kernel_action` the actor loop uses.
//! * App-module actions reduce through the generic
//!   `nmp_core::nmp_app_dispatch_action` seam — the host-extensibility path.
//! * Protocol-module actions (no generic dispatch surface) surface a typed,
//!   app-noun-free `KernelUpdate::UriRejected` — never a panic (D6), never a
//!   fake success.
//!
//! These tests pin that routing as a STRING contract (they read the emitted
//! `ffi.rs`, they do not compile it). The compile-and-run proof lives in
//! `cargo check -p nmp-app-fixture` + `cargo test -p fixture-todo-core`.

use std::fs;
use std::path::PathBuf;

/// Generate `ffi.rs` for the canonical fixture manifest (zero protocol
/// modules, one app module: `fixture-todo-core`).
fn generate_fixture(name: &str) -> String {
    generate_ffi(name, "[]", r#"["fixture-todo-core"]"#)
}

/// Generate `ffi.rs` for a manifest with the given `protocol` / `app` module
/// arrays (verbatim TOML array literals).
fn generate_ffi(name: &str, protocol: &str, app: &str) -> String {
    let mut root = std::env::temp_dir();
    root.push(format!("nmp-codegen-ffi-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let manifest = root.join("nmp.toml");
    fs::write(
        &manifest,
        format!(
            r#"
        [app]
        name = "fixture"
        display_name = "Fixture"

        [modules]
        kernel = "nmp-core"
        protocol = {protocol}
        app = {app}
        "#
        ),
    )
    .unwrap();
    let out = root.join("out");
    nmp_codegen::generate_modules(&manifest, &out).unwrap();
    let ffi = fs::read_to_string(out.join("src/ffi.rs")).unwrap();
    fs::remove_dir_all(&root).unwrap();
    ffi
}

/// The original-sin body: a single unconditional `Diagnostics` that ignores
/// the action discriminant. If this string ever reappears the generator has
/// regressed to a stub.
#[test]
fn dispatch_is_not_the_old_diagnostics_stub() {
    let ffi = generate_fixture("not-stub");
    assert!(
        !ffi.contains(r#"format!("dispatched {} at rev {}", action.namespace(), self.rev)"#),
        "generated dispatch regressed to the NMP-145 fake-Diagnostics stub:\n{ffi}"
    );
    assert!(
        !ffi.contains("unimplemented!")
            && !ffi.contains("todo!")
            && !ffi.contains("TODO")
            && !ffi.contains("FIXME")
            && !ffi.contains("panic!"),
        "generated dispatch must contain no stub/panic markers (D6):\n{ffi}"
    );
}

/// Real routing: `AppAction::Kernel(_)` is routed through the public
/// `nmp_core::KernelReducer`, which delegates to the same hand-written
/// `dispatch_kernel_action` reducer used by the actor loop. The generated
/// `match` must branch on the action discriminant and must hand every
/// `KernelAction` (without per-arm copy-paste) to `self.kernel.reduce(...)`.
#[test]
fn dispatch_routes_kernel_arm_through_public_reducer() {
    let ffi = generate_fixture("pure-arms");

    assert!(
        ffi.contains("match action"),
        "generated dispatch must branch on the action discriminant:\n{ffi}"
    );
    assert!(
        ffi.contains("AppAction::Kernel(action)") && ffi.contains("self.kernel.reduce(action)"),
        "Kernel arm must route through the public reducer:\n{ffi}"
    );
}

/// The kernel arm is unaffected by the host-extensibility migration:
/// `FfiApp` still owns a `nmp_core::KernelReducer` and routes every
/// `AppAction::Kernel(_)` through `self.kernel.reduce(_)` — `OpenUri` and
/// every other `KernelAction` reduce end-to-end through it, no rejection arm.
#[test]
fn dispatch_routes_kernel_actions_through_nmp_core_reducer() {
    let ffi = generate_fixture("reducer-routing");
    assert!(
        ffi.contains("nmp_core::KernelReducer"),
        "FfiApp must own a nmp_core::KernelReducer:\n{ffi}"
    );
    assert!(
        ffi.contains("self.kernel.reduce("),
        "Kernel actions must route through self.kernel.reduce(...):\n{ffi}"
    );
    // OpenUri specifically is no longer a kernel-bound rejection.
    assert!(
        !ffi.contains("OpenUri is kernel-bound"),
        "OpenUri must no longer be rejected as kernel-bound:\n{ffi}"
    );
}

/// THE MIGRATION CONTRACT: an app-module action no longer surfaces the legacy
/// `UriRejected` stub — it is routed through the generic
/// `nmp_app_dispatch_action` seam. The generated `FfiApp` owns a `*mut NmpApp`
/// (allocated via `nmp_app_new`), emits a per-app-module dispatch arm, and
/// frees the app in `Drop`.
#[test]
fn app_module_actions_route_through_dispatch_action_seam() {
    let ffi = generate_fixture("app-seam");
    // The host owns an `NmpApp` allocated via `nmp_app_new`.
    assert!(
        ffi.contains("app: *mut NmpApp") && ffi.contains("nmp_app_new()"),
        "FfiApp must own a *mut NmpApp allocated via nmp_app_new:\n{ffi}"
    );
    // The app module's namespace registration runs in `new()`.
    assert!(
        ffi.contains("fixture_todo_core::register(unsafe { &mut *app })"),
        "FfiApp::new must register the app module's namespace:\n{ffi}"
    );
    // The app-module action arm dispatches through the generic seam.
    assert!(
        ffi.contains("AppAction::FixtureTodoCore(action)")
            && ffi.contains("self.dispatch_app_action(")
            && ffi.contains("fixture_todo_core::ACTION_NAMESPACE"),
        "app-module actions must route through nmp_app_dispatch_action:\n{ffi}"
    );
    // The actual C-ABI seam symbol is called.
    assert!(
        ffi.contains("nmp_app_dispatch_action(self.app"),
        "the dispatch helper must call the nmp_app_dispatch_action C symbol:\n{ffi}"
    );
    // The app module's accepted-update constructor is used on success.
    assert!(
        ffi.contains("fixture_todo_core::accepted"),
        "an accepted dispatch must use the module's accepted() constructor:\n{ffi}"
    );
    // The owned NmpApp is released in Drop.
    assert!(
        ffi.contains("impl Drop for FfiApp") && ffi.contains("nmp_app_free(self.app)"),
        "FfiApp must free its NmpApp in Drop:\n{ffi}"
    );
}

/// An app-only manifest (the canonical fixture) emits explicit per-app-module
/// dispatch arms, so the kernel + app arms are exhaustive. A protocol-module
/// `other =>` catch-all there would be an `unreachable_patterns` warning (hard
/// error under `deny(warnings)`) — the generator must omit it.
#[test]
fn app_only_manifest_omits_protocol_catch_all() {
    let ffi = generate_fixture("app-only");
    assert!(
        !ffi.contains("other =>"),
        "an app-only crate must not emit an unreachable protocol catch-all:\n{ffi}"
    );
}

/// A zero-module manifest yields an `AppAction` with only `Kernel(_)`, so the
/// single `AppAction::Kernel(action)` arm is exhaustive. No catch-all, no stub.
#[test]
fn zero_module_manifest_omits_unreachable_catch_all() {
    let ffi = generate_ffi("zero", "[]", "[]");
    assert!(
        !ffi.contains("other =>"),
        "zero-module crate must not emit an unreachable catch-all:\n{ffi}"
    );
    // The kernel arm still routes through the public reducer.
    assert!(
        ffi.contains("self.kernel.reduce("),
        "zero-module crate must still route the kernel arm through the public reducer:\n{ffi}"
    );
    assert!(
        !ffi.contains("unimplemented!") && !ffi.contains("panic!"),
        "no stub/panic markers even with zero modules:\n{ffi}"
    );
}

/// A protocol-bearing manifest DOES emit the `other =>` catch-all: protocol
/// modules have no generic dispatch surface reachable from the generated
/// crate, so their actions surface a typed `KernelUpdate::UriRejected` (D6).
#[test]
fn protocol_module_manifest_emits_catch_all() {
    let ffi = generate_ffi("with-protocol", r#"["nmp-nip01"]"#, "[]");
    assert!(
        ffi.contains("other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected"),
        "a protocol-bearing crate must route protocol-projected actions to the rejection:\n{ffi}"
    );
}

/// Determinism: the same manifest in yields byte-identical source out.
#[test]
fn generated_ffi_is_deterministic() {
    let a = generate_fixture("det-a");
    let b = generate_fixture("det-b");
    assert_eq!(a, b);
}

#[allow(dead_code)]
fn temp() -> PathBuf {
    std::env::temp_dir()
}
