//! Generator for the per-app `src/ffi.rs` (`FfiApp::dispatch`).
//!
//! NMP-145 + T-NMP-145-FF: this used to emit a placeholder `dispatch()` that
//! ignored its input and always returned `KernelUpdate::Diagnostics`. A stub
//! in non-test generated code is a doctrine violation and makes the FFI
//! dispatch path dead/fake. The first NMP-145 pass replaced the stub with
//! a per-arm `match` copied verbatim from the hand-written reducer, but it
//! could not cover `OpenUri` (kernel-bound — needs a private `&mut Kernel`)
//! and so surfaced a typed `UriRejected` for that arm.
//!
//! T-NMP-145-FF closes that boundary: `nmp-core` exposes a public
//! [`nmp_core::KernelReducer`] that owns an encapsulated `Kernel` and routes
//! every [`KernelAction`] (including `OpenUri`) through the same
//! `dispatch_kernel_action` reducer the actor uses. The generated `FfiApp`
//! owns a `KernelReducer` and routes `AppAction::Kernel(_)` through it in one
//! line — no per-arm copy-paste, no rejection arm.
//!
//! ## Coverage boundary (modules)
//!
//! Module-projected variants (`AppAction::<Module>(_)`) run module reducers
//! that have no generic surface reachable from this generated crate. They
//! continue to surface a typed, app-noun-free [`KernelUpdate::UriRejected`]
//! with a stable reason string (D0/D6: never a panic across FFI, never a fake
//! success). Wiring real module routing is a separate seam.

use crate::AppManifest;

/// Emit `src/ffi.rs` for `manifest`.
pub(crate) fn ffi_rs(manifest: &AppManifest) -> String {
    // The module catch-all arm is only emitted when the manifest projects at
    // least one module. With zero modules `AppAction` has a single
    // `Kernel(_)` variant and the single kernel arm below is exhaustive —
    // an `other =>` arm would be an `unreachable_patterns` warning (a hard
    // error under `deny(warnings)`), so we omit it.
    let module_arm = if manifest.ordered_modules().is_empty() {
        String::new()
    } else {
        "\n\n            // ── Module-projected actions (coverage boundary) ─────────────\n\
         \x20           // Module crates expose no generic reducer reachable here.\n\
         \x20           // Surface a typed rejection carrying the action namespace so the\n\
         \x20           // boundary is observable rather than silent. See NMP-145.\n\
         \x20           other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {\n\
         \x20               uri: other.namespace().to_string(),\n\
         \x20               reason: \"module-projected action has no generated reducer; \\\n\
routing requires a module-reducer seam (see NMP-145)\"\n\
         \x20                   .to_string(),\n\
         \x20           }),"
            .to_string()
    };
    // NOTE: kept as one `format!` with `{{`/`}}` escaping so the emitted file
    // stays byte-deterministic (no map iteration, no timestamps).
    format!(
        r#"use crate::{{AppAction, AppUpdate}};

/// Per-app FFI entry-point.
///
/// `dispatch` reduces one [`AppAction`] into the [`AppUpdate`] the host app
/// observes. The kernel arm is routed through the public
/// [`nmp_core::KernelReducer`], which delegates to the same
/// `dispatch_kernel_action` reducer the actor loop uses — so `OpenUri` (and
/// every other [`nmp_core::KernelAction`] variant) reduces end-to-end through
/// the same encapsulated kernel. Module-projected actions have no reducer
/// reachable from this generated crate and surface a typed
/// [`nmp_core::KernelUpdate::UriRejected`] (D6: no panic across FFI, no fake
/// success). See NMP-145 / T-NMP-145-FF.
#[derive(Default)]
pub struct FfiApp {{
    kernel: nmp_core::KernelReducer,
    rev: u64,
}}

impl FfiApp {{
    pub fn new() -> Self {{
        Self::default()
    }}

    pub fn app_name(&self) -> &'static str {{
        "{name}"
    }}

    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {{
        self.rev = self.rev.saturating_add(1);
        match action {{
            // ── KernelAction → KernelUpdate (T-NMP-145-FF) ────────────────
            // Routed through the public KernelReducer, which delegates to
            // nmp_core::actor::kernel_action::dispatch_kernel_action against
            // an encapsulated Kernel. Every variant — including OpenUri,
            // which registers an interest through the single-writer
            // registry — reduces here.
            AppAction::Kernel(action) => AppUpdate::Kernel(self.kernel.reduce(action)),{module_arm}
        }}
    }}
}}
"#,
        name = manifest.name,
        module_arm = module_arm,
    )
}
