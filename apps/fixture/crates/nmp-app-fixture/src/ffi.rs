use std::ffi::{CStr, CString};

use nmp_ffi::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new, NmpApp,
};

use crate::{AppAction, AppUpdate};

/// Per-app FFI entry-point — wired onto NMP's live extensibility seams.
///
/// `FfiApp` owns an [`NmpApp`] and reduces one [`AppAction`] into the
/// [`AppUpdate`] the host observes:
///
/// * [`AppAction::Kernel`] — routed through the public
///   [`nmp_core::KernelReducer`], which delegates to the same
///   `dispatch_kernel_action` reducer the actor loop uses — so `OpenUri` (and
///   every other [`nmp_core::KernelAction`] variant) reduces end-to-end.
/// * App-module actions — routed through the generic
///   [`nmp_app_dispatch_action`] seam against the namespace each app module
///   registered in [`FfiApp::new`]. The host-registered module validates, the
///   host-registered executor applies the action, and the host-registered
///   snapshot projection carries the result.
/// * Protocol-module actions — have no generic dispatch surface reachable from
///   this generated crate; they surface a typed
///   [`nmp_core::KernelUpdate::UriRejected`] (D6: no panic, no fake success).
pub struct FfiApp {
    /// The owned NMP app handle. Allocated by [`nmp_app_new`] in
    /// [`FfiApp::new`]; released by [`nmp_app_free`] in [`Drop`].
    app: *mut NmpApp,
    /// The public `KernelReducer` for the [`AppAction::Kernel`] arm.
    kernel: nmp_core::KernelReducer,
    rev: u64,
    /// Local store handle returned by `fixture_todo_core::register`, shared
    /// with that module's registered action executor + snapshot projector.
    #[allow(dead_code)]
    fixture_todo_core_store: fixture_todo_core::Store,
}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the
// `app: *mut NmpApp` field. The generated host shell is created, dispatched
// against, and dropped from one isolation context (the same caller convention
// the consuming host crate documents). The pointer is only ever read
// (passed to `nmp_app_dispatch_action`) and freed once in `Drop`.
unsafe impl Send for FfiApp {}
unsafe impl Sync for FfiApp {}

impl FfiApp {
    /// Construct the host: allocate an [`NmpApp`] and wire every app module's
    /// action + snapshot seams into it.
    ///
    /// Registration happens here, during host init — before `nmp_app_start`
    /// and before any `dispatch` call — because each module's `register` seam
    /// needs `&mut NmpApp`.
    pub fn new() -> Self {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; `app` is valid for the
        // lifetime of this `FfiApp` (freed in `Drop`). No aliasing `&NmpApp`
        // is live during this exclusive borrow — `FfiApp` is not yet built.
        let fixture_todo_core_store = fixture_todo_core::register(unsafe { &mut *app });
        Self {
            app,
            kernel: nmp_core::KernelReducer::new(),
            rev: 0,
            fixture_todo_core_store,
        }
    }

    pub fn app_name(&self) -> &'static str {
        "fixture"
    }

    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {
        self.rev = self.rev.saturating_add(1);
        match action {
            // ── KernelAction → KernelUpdate ───────────────────────────────
            // Routed through the public `KernelReducer`, which delegates to
            // `nmp_core::actor::kernel_action::dispatch_kernel_action` against
            // an encapsulated `Kernel`. Every variant — including `OpenUri` —
            // reduces here.
            AppAction::Kernel(action) => AppUpdate::Kernel(self.kernel.reduce(action)),

            // ── fixture-todo-core → live action seam ──────────────────────────────
            // Routed through the generic `nmp_app_dispatch_action` against
            // the namespace `fixture_todo_core::register` wired into the kernel's
            // action registry. The host module validates, the host executor
            // applies the action, and the host snapshot projection carries
            // the result — the host-extensibility thesis, end-to-end.
            AppAction::FixtureTodoCore(action) => {
                match serde_json::to_string(&action) {
                    Ok(action_json) => self.dispatch_app_action(
                        fixture_todo_core::ACTION_NAMESPACE,
                        &action_json,
                        fixture_todo_core::accepted as fn() -> fixture_todo_core::Update,
                        AppUpdate::FixtureTodoCore,
                    ),
                    Err(error) => AppUpdate::Kernel(
                        nmp_core::KernelUpdate::UriRejected {
                            uri: fixture_todo_core::ACTION_NAMESPACE.to_string(),
                            reason: format!("action encode failed: {error}"),
                        },
                    ),
                }
            }
        }
    }

    /// Drive one app-module action through the generic
    /// [`nmp_app_dispatch_action`] seam and map the JSON result onto an
    /// [`AppUpdate`].
    ///
    /// `{"correlation_id":…}` (accept) → `accepted_variant(accepted())`;
    /// `{"error":…}` (a host-validator rejection) → a typed
    /// [`nmp_core::KernelUpdate::UriRejected`] carrying the namespace
    /// (D6: failures are data, never a panic / fake success).
    fn dispatch_app_action<U>(
        &self,
        namespace: &str,
        action_json: &str,
        accepted: fn() -> U,
        accepted_variant: fn(U) -> AppUpdate,
    ) -> AppUpdate {
        let result = self.dispatch_action_json(namespace, action_json);
        let parsed: serde_json::Value = match serde_json::from_str(&result) {
            Ok(value) => value,
            Err(error) => {
                return AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {
                    uri: namespace.to_string(),
                    reason: format!("dispatch result decode failed: {error}"),
                });
            }
        };
        if parsed.get("correlation_id").is_some() {
            accepted_variant(accepted())
        } else {
            let reason = parsed
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("action rejected")
                .to_string();
            AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {
                uri: namespace.to_string(),
                reason,
            })
        }
    }

    /// Call the C-ABI [`nmp_app_dispatch_action`] and return its JSON result
    /// as an owned `String`, freeing the returned C string. The generated host
    /// calls the seam through the same `extern "C"` symbol a host consumer would.
    fn dispatch_action_json(&self, namespace: &str, action_json: &str) -> String {
        // An interior NUL cannot cross to C — collapse it to an error JSON so
        // the caller still gets well-formed data (D6).
        let (ns, body) = match (CString::new(namespace), CString::new(action_json)) {
            (Ok(ns), Ok(body)) => (ns, body),
            _ => return r#"{"error":"action contains NUL byte"}"#.to_string(),
        };
        let ptr = nmp_app_dispatch_action(self.app, ns.as_ptr(), body.as_ptr());
        if ptr.is_null() {
            // `nmp_app_dispatch_action` never returns null for a non-null app
            // (D6); treat a null as data rather than a panic.
            return r#"{"error":"dispatch_action returned null"}"#.to_string();
        }
        // SAFETY: `ptr` is a valid NUL-terminated C string from
        // `nmp_app_dispatch_action`; copied immediately, then freed below.
        let out = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        nmp_app_free_string(ptr);
        out
    }
}

impl Default for FfiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FfiApp {
    fn drop(&mut self) {
        // Release the `NmpApp` allocated in `new()`. `nmp_app_free`'s `Drop`
        // sends `Shutdown` and joins the actor thread, so the actor cannot
        // outlive `FfiApp`.
        nmp_app_free(self.app);
    }
}
