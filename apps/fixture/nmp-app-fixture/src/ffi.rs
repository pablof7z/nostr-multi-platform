use crate::{AppAction, AppUpdate};

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
pub struct FfiApp {
    kernel: nmp_core::KernelReducer,
    rev: u64,
}

impl FfiApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn app_name(&self) -> &'static str {
        "fixture"
    }

    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {
        self.rev = self.rev.saturating_add(1);
        match action {
            // ── KernelAction → KernelUpdate (T-NMP-145-FF) ────────────────
            // Routed through the public KernelReducer, which delegates to
            // nmp_core::actor::kernel_action::dispatch_kernel_action against
            // an encapsulated Kernel. Every variant — including OpenUri,
            // which registers an interest through the single-writer
            // registry — reduces here.
            AppAction::Kernel(action) => AppUpdate::Kernel(self.kernel.reduce(action)),

            // ── Module-projected actions (coverage boundary) ─────────────
            // Module crates expose no generic reducer reachable here.
            // Surface a typed rejection carrying the action namespace so the
            // boundary is observable rather than silent. See NMP-145.
            other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {
                uri: other.namespace().to_string(),
                reason: "module-projected action has no generated reducer; \
routing requires a module-reducer seam (see NMP-145)"
                    .to_string(),
            }),
        }
    }
}
