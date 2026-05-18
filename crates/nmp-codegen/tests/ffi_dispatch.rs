//! Anti-stub contract for the generated `FfiApp::dispatch`.
//!
//! NMP-145: the codegen used to emit a placeholder `dispatch()` that ignored
//! its input and always returned `KernelUpdate::Diagnostics`. That is a
//! doctrine violation (a stub in non-test generated code) and makes the FFI
//! dispatch path dead/fake. These tests pin the *real* routing so any
//! regression back to a single fake-Diagnostics body fails loudly.
//!
//! The generated `dispatch()` MUST be behaviorally equivalent to the
//! hand-written `nmp_core::actor::dispatch_kernel_action` for the pure
//! (`Kernel`-free) `KernelAction` arms. The kernel-bound `OpenUri` arm and the
//! module-projected variants have no reducer reachable from the generated
//! crate (nmp-core's reducer is `pub(crate)` and needs a private `&mut
//! Kernel`); they are surfaced as a typed, app-noun-free `KernelUpdate`
//! rejection — never a panic (D6) and never a fake success.

use std::fs;
use std::path::PathBuf;

fn generate_fixture(name: &str) -> String {
    let mut root = std::env::temp_dir();
    root.push(format!("nmp-codegen-ffi-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let manifest = root.join("nmp.toml");
    fs::write(
        &manifest,
        r#"
        [app]
        name = "fixture"
        display_name = "Fixture"

        [modules]
        kernel = "nmp-core"
        protocol = []
        app = ["fixture-todo-core"]
        "#,
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

/// Real routing: each pure `KernelAction` arm must map to the exact
/// `KernelUpdate` the hand-written reducer emits. Asserting the constructor
/// shapes textually is the anti-stub discriminator — a body that returns one
/// update regardless of input cannot contain all of these.
#[test]
fn dispatch_routes_each_pure_kernel_arm() {
    let ffi = generate_fixture("pure-arms");

    // Mirrors nmp_core::actor::kernel_action::dispatch_kernel_action.
    let expected = [
        "AppAction::Kernel(nmp_core::KernelAction::Start)",
        "AppUpdate::Kernel(nmp_core::KernelUpdate::Started { rev: 0 })",
        "AppAction::Kernel(nmp_core::KernelAction::Stop)",
        "AppUpdate::Kernel(nmp_core::KernelUpdate::Stopped { rev: 0 })",
        "nmp_core::KernelAction::OpenView { namespace, key }",
        "nmp_core::KernelUpdate::ViewOpened { namespace, key }",
        "nmp_core::KernelAction::CloseView { namespace, key }",
        "nmp_core::KernelUpdate::ViewClosed { namespace, key }",
        "nmp_core::KernelAction::RunDiagnostics",
        "nmp_core::KernelUpdate::Diagnostics",
    ];
    for needle in expected {
        assert!(
            ffi.contains(needle),
            "generated dispatch missing real routing fragment `{needle}`:\n{ffi}"
        );
    }
    assert!(
        ffi.contains("match action"),
        "generated dispatch must branch on the action discriminant:\n{ffi}"
    );
}

/// Kernel-bound + module-projected ops have no reducer reachable from the
/// generated crate. They must surface as a typed, app-noun-free rejection
/// (D6: no panic across FFI, no fake success), referencing the follow-up.
#[test]
fn uncovered_ops_surface_a_typed_rejection_not_a_panic() {
    let ffi = generate_fixture("uncovered");
    assert!(
        ffi.contains("KernelUpdate::UriRejected"),
        "uncovered ops must surface KernelUpdate::UriRejected (D6 typed error):\n{ffi}"
    );
    assert!(
        ffi.contains("NMP-145"),
        "the rejection reason must reference the NMP-145 follow-up boundary:\n{ffi}"
    );
    // OpenUri is the documented kernel-bound op that cannot be generated.
    assert!(
        ffi.contains("nmp_core::KernelAction::OpenUri { uri }"),
        "OpenUri must be matched explicitly and routed to the rejection:\n{ffi}"
    );
}

fn generate_zero_module() -> String {
    let mut root = std::env::temp_dir();
    root.push(format!("nmp-codegen-ffi-zero-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let manifest = root.join("nmp.toml");
    fs::write(
        &manifest,
        r#"
        [app]
        name = "bare"
        display_name = "Bare"

        [modules]
        kernel = "nmp-core"
        protocol = []
        app = []
        "#,
    )
    .unwrap();
    let out = root.join("out");
    nmp_codegen::generate_modules(&manifest, &out).unwrap();
    let ffi = fs::read_to_string(out.join("src/ffi.rs")).unwrap();
    fs::remove_dir_all(&root).unwrap();
    ffi
}

/// A zero-module manifest yields an `AppAction` with only `Kernel(_)`, so the
/// six explicit `KernelAction` arms are exhaustive. Emitting a module
/// catch-all there would be an `unreachable_patterns` warning (hard error
/// under `deny(warnings)`) — the generator must omit it.
#[test]
fn zero_module_manifest_omits_unreachable_catch_all() {
    let ffi = generate_zero_module();
    assert!(
        !ffi.contains("other =>"),
        "zero-module crate must not emit an unreachable module catch-all:\n{ffi}"
    );
    // The kernel-bound OpenUri rejection is still present (it is a
    // KernelAction variant, always reachable).
    assert!(
        ffi.contains("nmp_core::KernelAction::OpenUri { uri }")
            && ffi.contains("KernelUpdate::UriRejected"),
        "OpenUri must still route to the typed rejection without modules:\n{ffi}"
    );
    assert!(
        !ffi.contains("unimplemented!") && !ffi.contains("panic!"),
        "no stub/panic markers even with zero modules:\n{ffi}"
    );
}

/// With modules present the catch-all IS emitted (covered by the fixture
/// generator used elsewhere).
#[test]
fn module_manifest_emits_catch_all() {
    let ffi = generate_fixture("with-modules");
    assert!(
        ffi.contains("other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected"),
        "module-bearing crate must route projected actions to the rejection:\n{ffi}"
    );
}

/// Determinism still holds for the richer body.
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
