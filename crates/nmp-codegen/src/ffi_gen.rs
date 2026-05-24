//! Generator for the per-app `src/ffi.rs` (`FfiApp`).
//!
//! ## What the generated `FfiApp` does
//!
//! `FfiApp` is the per-app FFI shell. It owns an [`nmp_core::NmpApp`] and
//! routes one `AppAction` into the `AppUpdate` the host observes:
//!
//! * `AppAction::Kernel(_)` — routed through the public
//!   [`nmp_core::KernelReducer`], which delegates to the same
//!   `dispatch_kernel_action` reducer the actor loop uses (so `OpenUri` and
//!   every other `KernelAction` reduces end-to-end). Unchanged by the
//!   host-extensibility migration.
//! * `AppAction::<AppModule>(_)` — routed through the **live action seam**:
//!   `nmp_core::nmp_app_dispatch_action` against the namespace the app module
//!   registered. This replaces the former `UriRejected` stub — app-module
//!   actions now reduce end-to-end through `dispatch_action`.
//! * `AppAction::<ProtocolModule>(_)` — protocol modules expose no generic
//!   dispatch surface reachable from this generated crate; they continue to
//!   surface a typed, app-noun-free [`nmp_core::KernelUpdate::UriRejected`]
//!   (D6: no panic across FFI, no fake success). Wiring real protocol-module
//!   routing is a separate seam.
//!
//! ## App-module convention (the host-extensibility contract)
//!
//! Every **app** module crate listed under `[modules].app` MUST export four
//! symbols the generator wires mechanically — no per-crate special-casing:
//!
//! * `pub fn register(app: &mut nmp_core::NmpApp) -> Store;` — wires the
//!   module's action namespace + snapshot projection into `app` during host
//!   init. The returned `Store` handle is retained by `FfiApp`.
//! * `pub const ACTION_NAMESPACE: &str;` — the namespace `dispatch_action`
//!   keys on.
//! * `pub fn accepted() -> Update;` — the `Update` value returned when a
//!   dispatched action is accepted.
//! * `pub type Store;` — the store-handle type `register` returns; the
//!   generated `FfiApp` types its per-module field as `<crate>::Store`.
//!
//! `fixture-todo-core` is the live consumer of this contract.

use crate::{rust_crate_name, variant_name, AppManifest};

/// Emit `src/ffi.rs` for `manifest`.
pub(crate) fn ffi_rs(manifest: &AppManifest) -> String {
    let app_modules = &manifest.modules.app;
    let protocol_modules = &manifest.modules.protocol;

    // ── Per-app-module registration calls (inside `FfiApp::new`) ──────────
    // One `let <crate>_store = <crate>::register(unsafe { &mut *app });` per
    // app module. The store handle is retained so the host can inspect the
    // module's local state directly.
    let register_calls: String = app_modules
        .iter()
        .map(|module| {
            let crate_name = rust_crate_name(module);
            format!(
                "\n        // SAFETY: `nmp_app_new` never returns null; `app` is valid for the\n\
                 \x20       // lifetime of this `FfiApp` (freed in `Drop`). No aliasing `&NmpApp`\n\
                 \x20       // is live during this exclusive borrow — `FfiApp` is not yet built.\n\
                 \x20       let {crate_name}_store = {crate_name}::register(unsafe {{ &mut *app }});"
            )
        })
        .collect();

    // ── Per-app-module struct fields holding the returned store handles ───
    let store_fields: String = app_modules
        .iter()
        .map(|module| {
            let crate_name = rust_crate_name(module);
            format!(
                "\n    /// Local store handle returned by `{crate_name}::register`, shared\n\
                 \x20   /// with that module's registered action executor + snapshot projector.\n\
                 \x20   #[allow(dead_code)]\n\
                 \x20   {crate_name}_store: {crate_name}::Store,"
            )
        })
        .collect();

    // ── Per-app-module struct-init lines (inside `FfiApp::new`) ───────────
    let store_inits: String = app_modules
        .iter()
        .map(|module| {
            let crate_name = rust_crate_name(module);
            format!("\n            {crate_name}_store,")
        })
        .collect();

    // ── Per-app-module dispatch arms ─────────────────────────────────────
    // Each app-module action is serialized to JSON and driven through the
    // generic `nmp_app_dispatch_action` seam. Accept → `AppUpdate::<Variant>`
    // via the module's `accepted()`; reject → typed `KernelUpdate::UriRejected`
    // carrying the namespace (D6: failures are data).
    let app_arms: String = app_modules
        .iter()
        .map(|module| {
            let crate_name = rust_crate_name(module);
            let variant = variant_name(module);
            format!(
                "\n\n            // ── {module} → live action seam ──────────────────────────────\n\
                 \x20           // Routed through the generic `nmp_app_dispatch_action` against\n\
                 \x20           // the namespace `{crate_name}::register` wired into the kernel's\n\
                 \x20           // action registry. The host module validates, the host executor\n\
                 \x20           // applies the action, and the host snapshot projection carries\n\
                 \x20           // the result — the host-extensibility thesis, end-to-end.\n\
                 \x20           AppAction::{variant}(action) => {{\n\
                 \x20               match serde_json::to_string(&action) {{\n\
                 \x20                   Ok(action_json) => self.dispatch_app_action(\n\
                 \x20                       {crate_name}::ACTION_NAMESPACE,\n\
                 \x20                       &action_json,\n\
                 \x20                       {crate_name}::accepted as fn() -> {crate_name}::Update,\n\
                 \x20                       AppUpdate::{variant},\n\
                 \x20                   ),\n\
                 \x20                   Err(error) => AppUpdate::Kernel(\n\
                 \x20                       nmp_core::KernelUpdate::UriRejected {{\n\
                 \x20                           uri: {crate_name}::ACTION_NAMESPACE.to_string(),\n\
                 \x20                           reason: format!(\"action encode failed: {{error}}\"),\n\
                 \x20                       }},\n\
                 \x20                   ),\n\
                 \x20               }}\n\
                 \x20           }}"
            )
        })
        .collect();

    // ── Protocol-module catch-all (only when protocol modules are present) ─
    // Protocol modules expose no generic dispatch surface reachable here. With
    // app modules already covered by explicit arms above, this `other =>` arm
    // is emitted ONLY when at least one protocol module exists — otherwise it
    // would be an `unreachable_patterns` warning (a hard error under
    // `deny(warnings)`), since the kernel + app arms are then exhaustive.
    let protocol_arm = if protocol_modules.is_empty() {
        String::new()
    } else {
        "\n\n            // ── Protocol-projected actions (coverage boundary) ───────────\n\
         \x20           // Protocol modules expose no generic dispatch surface reachable\n\
         \x20           // here. Surface a typed rejection carrying the action namespace\n\
         \x20           // so the boundary is observable rather than silent (D6).\n\
         \x20           other => AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {\n\
         \x20               uri: other.namespace().to_string(),\n\
         \x20               reason: \"protocol-projected action has no generic dispatch \\\n\
seam reachable from the generated crate\"\n\
         \x20                   .to_string(),\n\
         \x20           }),"
            .to_string()
    };

    // NOTE: kept as one `format!` with `{{`/`}}` escaping so the emitted file
    // stays byte-deterministic (no map iteration, no timestamps). The outer
    // raw string uses the `r##"…"##` delimiter because the emitted body itself
    // contains `r#"…"#` raw-string literals (the error-JSON fallbacks).
    format!(
        r##"use std::ffi::{{CStr, CString}};

use nmp_ffi::{{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new, NmpApp,
}};

use crate::{{AppAction, AppUpdate}};

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
pub struct FfiApp {{
    /// The owned NMP app handle. Allocated by [`nmp_app_new`] in
    /// [`FfiApp::new`]; released by [`nmp_app_free`] in [`Drop`].
    app: *mut NmpApp,
    /// The public `KernelReducer` for the [`AppAction::Kernel`] arm.
    kernel: nmp_core::KernelReducer,
    rev: u64,{store_fields}
}}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the
// `app: *mut NmpApp` field. The generated host shell is created, dispatched
// against, and dropped from one isolation context (the same caller convention
// the consuming host crate documents). The pointer is only ever read
// (passed to `nmp_app_dispatch_action`) and freed once in `Drop`.
unsafe impl Send for FfiApp {{}}
unsafe impl Sync for FfiApp {{}}

impl FfiApp {{
    /// Construct the host: allocate an [`NmpApp`] and wire every app module's
    /// action + snapshot seams into it.
    ///
    /// Registration happens here, during host init — before `nmp_app_start`
    /// and before any `dispatch` call — because each module's `register` seam
    /// needs `&mut NmpApp`.
    pub fn new() -> Self {{
        let app = nmp_app_new();{register_calls}
        Self {{
            app,
            kernel: nmp_core::KernelReducer::new(),
            rev: 0,{store_inits}
        }}
    }}

    pub fn app_name(&self) -> &'static str {{
        "{name}"
    }}

    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {{
        self.rev = self.rev.saturating_add(1);
        match action {{
            // ── KernelAction → KernelUpdate ───────────────────────────────
            // Routed through the public `KernelReducer`, which delegates to
            // `nmp_core::actor::kernel_action::dispatch_kernel_action` against
            // an encapsulated `Kernel`. Every variant — including `OpenUri` —
            // reduces here.
            AppAction::Kernel(action) => AppUpdate::Kernel(self.kernel.reduce(action)),{app_arms}{protocol_arm}
        }}
    }}

    /// Drive one app-module action through the generic
    /// [`nmp_app_dispatch_action`] seam and map the JSON result onto an
    /// [`AppUpdate`].
    ///
    /// `{{"correlation_id":…}}` (accept) → `accepted_variant(accepted())`;
    /// `{{"error":…}}` (a host-validator rejection) → a typed
    /// [`nmp_core::KernelUpdate::UriRejected`] carrying the namespace
    /// (D6: failures are data, never a panic / fake success).
    fn dispatch_app_action<U>(
        &self,
        namespace: &str,
        action_json: &str,
        accepted: fn() -> U,
        accepted_variant: fn(U) -> AppUpdate,
    ) -> AppUpdate {{
        let result = self.dispatch_action_json(namespace, action_json);
        let parsed: serde_json::Value = match serde_json::from_str(&result) {{
            Ok(value) => value,
            Err(error) => {{
                return AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {{
                    uri: namespace.to_string(),
                    reason: format!("dispatch result decode failed: {{error}}"),
                }});
            }}
        }};
        if parsed.get("correlation_id").is_some() {{
            accepted_variant(accepted())
        }} else {{
            let reason = parsed
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("action rejected")
                .to_string();
            AppUpdate::Kernel(nmp_core::KernelUpdate::UriRejected {{
                uri: namespace.to_string(),
                reason,
            }})
        }}
    }}

    /// Call the C-ABI [`nmp_app_dispatch_action`] and return its JSON result
    /// as an owned `String`, freeing the returned C string. The generated host
    /// calls the seam through the same `extern "C"` symbol a host consumer would.
    fn dispatch_action_json(&self, namespace: &str, action_json: &str) -> String {{
        // An interior NUL cannot cross to C — collapse it to an error JSON so
        // the caller still gets well-formed data (D6).
        let (ns, body) = match (CString::new(namespace), CString::new(action_json)) {{
            (Ok(ns), Ok(body)) => (ns, body),
            _ => return r#"{{"error":"action contains NUL byte"}}"#.to_string(),
        }};
        let ptr = nmp_app_dispatch_action(self.app, ns.as_ptr(), body.as_ptr());
        if ptr.is_null() {{
            // `nmp_app_dispatch_action` never returns null for a non-null app
            // (D6); treat a null as data rather than a panic.
            return r#"{{"error":"dispatch_action returned null"}}"#.to_string();
        }}
        // SAFETY: `ptr` is a valid NUL-terminated C string from
        // `nmp_app_dispatch_action`; copied immediately, then freed below.
        let out = unsafe {{ CStr::from_ptr(ptr) }}
            .to_string_lossy()
            .into_owned();
        nmp_app_free_string(ptr);
        out
    }}
}}

impl Default for FfiApp {{
    fn default() -> Self {{
        Self::new()
    }}
}}

impl Drop for FfiApp {{
    fn drop(&mut self) {{
        // Release the `NmpApp` allocated in `new()`. `nmp_app_free`'s `Drop`
        // sends `Shutdown` and joins the actor thread, so the actor cannot
        // outlive `FfiApp`.
        nmp_app_free(self.app);
    }}
}}
"##,
        name = manifest.name,
        store_fields = store_fields,
        register_calls = register_calls,
        store_inits = store_inits,
        app_arms = app_arms,
        protocol_arm = protocol_arm,
    )
}
