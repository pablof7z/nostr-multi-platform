//! Generator for the per-app `src/ffi.rs` (`FfiApp::dispatch`).
//!
//! NMP-145: this used to emit a placeholder `dispatch()` that ignored its
//! input and always returned `KernelUpdate::Diagnostics`. A stub in non-test
//! generated code is a doctrine violation and makes the FFI dispatch path
//! dead/fake. This module emits the *real* routing.
//!
//! ## Contract source of truth
//!
//! The generated `dispatch()` is behaviorally equivalent to the hand-written
//! reducer `nmp_core::actor::kernel_action::dispatch_kernel_action` for the
//! arms it can cover. The mapping is copied verbatim from that reducer:
//!
//! | `KernelAction`        | `KernelUpdate`                       |
//! |-----------------------|--------------------------------------|
//! | `Start`               | `Started { rev: 0 }`                 |
//! | `Stop`                | `Stopped { rev: 0 }`                 |
//! | `OpenView{ns,key}`    | `ViewOpened { ns, key }`             |
//! | `CloseView{ns,key}`   | `ViewClosed { ns, key }`             |
//! | `RunDiagnostics`      | `Diagnostics { summary: "" }`        |
//! | `OpenUri{uri}`        | *kernel-bound* — see boundary below  |
//!
//! ## Coverage boundary (why this is a maximal subset, not a stub)
//!
//! `OpenUri` and the module-projected variants (`AppAction::<Module>(_)`)
//! mutate the single-writer registry / run module reducers. The only reducer
//! that performs that work is `nmp_core::actor::dispatch_kernel_action`, which
//! is `pub(crate)` and needs a private `&mut Kernel` — neither is reachable
//! from a generated crate that depends on `nmp-core` as a normal library.
//!
//! Rather than emit another silent stub, those ops surface a **typed,
//! app-noun-free `KernelUpdate::UriRejected`** with a stable reason string
//! (D0/D6: never a panic across FFI, never a fake success). Wiring the real
//! `OpenUri` path requires nmp-core to expose a public pure reducer; that is
//! filed as the NMP-145 follow-up and referenced in the emitted reason.

use crate::AppManifest;

/// Emit `src/ffi.rs` for `manifest`.
pub(crate) fn ffi_rs(manifest: &AppManifest) -> String {
    // The module catch-all arm is only emitted when the manifest projects at
    // least one module. With zero modules `AppAction` has a single `Kernel(_)`
    // variant and the six explicit `KernelAction` arms below are exhaustive —
    // an `other =>` arm would be an `unreachable_patterns` warning (a hard
    // error under `deny(warnings)`), so we omit it. The kernel-bound `OpenUri`
    // arm is always present (it is a `KernelAction` variant, not a module).
    let module_arm = if manifest.ordered_modules().is_empty() {
        String::new()
    } else {
        "\n\n            // ── Module-projected actions (coverage boundary, NMP-145) ─────\n\
         \x20           // Module crates expose no generic reducer reachable here. Surface\n\
         \x20           // the same typed rejection, carrying the action namespace so the\n\
         \x20           // boundary is observable rather than silent.\n\
         \x20           other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {\n\
         \x20               uri: other.namespace().to_string(),\n\
         \x20               reason: \"module-projected action has no generated reducer; \\\n\
routing requires a public pure nmp-core reducer (see NMP-145 follow-up)\"\n\
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
/// observes. The pure (`Kernel`-free) `KernelAction` arms are routed verbatim
/// from `nmp_core::actor::dispatch_kernel_action`; kernel-bound (`OpenUri`)
/// and module-projected actions have no reducer reachable from this generated
/// crate and surface a typed `KernelUpdate::UriRejected` (D6: no panic across
/// FFI, no fake success). See NMP-145 for the OpenUri follow-up.
#[derive(Default)]
pub struct FfiApp {{
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
            // ── Pure KernelAction arms ────────────────────────────────────
            // Copied verbatim from nmp_core::actor::kernel_action::
            // dispatch_kernel_action. Behaviorally equivalent: no `&mut
            // Kernel` is required for any of these.
            AppAction::Kernel(nmp_core::KernelAction::Start) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::Started {{ rev: 0 }})
            }}
            AppAction::Kernel(nmp_core::KernelAction::Stop) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::Stopped {{ rev: 0 }})
            }}
            AppAction::Kernel(nmp_core::KernelAction::OpenView {{ namespace, key }}) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::ViewOpened {{ namespace, key }})
            }}
            AppAction::Kernel(nmp_core::KernelAction::CloseView {{ namespace, key }}) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::ViewClosed {{ namespace, key }})
            }}
            AppAction::Kernel(nmp_core::KernelAction::RunDiagnostics) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::Diagnostics {{
                    summary: String::new(),
                }})
            }}

            // ── Kernel-bound op (coverage boundary, NMP-145) ──────────────
            // `OpenUri` mutates the single-writer registry via the
            // `pub(crate)` nmp-core reducer + a private `&mut Kernel`,
            // neither reachable here. Surface a typed, app-noun-free
            // rejection (D6) rather than a panic or a fake success.
            AppAction::Kernel(nmp_core::KernelAction::OpenUri {{ uri }}) => {{
                AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {{
                    uri,
                    reason: "OpenUri is kernel-bound and has no reducer reachable \
from the generated FFI crate; needs a public pure nmp-core reducer \
(see NMP-145 follow-up)"
                        .to_string(),
                }})
            }}{module_arm}
        }}
    }}
}}
"#,
        name = manifest.name,
        module_arm = module_arm,
    )
}
