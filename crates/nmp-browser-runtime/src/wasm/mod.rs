//! `NmpWasmRuntime` — wasm-bindgen entry point for `nmp-browser-runtime`
//! (#2038 item A).
//!
//! This module satisfies the TypeScript contract in
//! `web/packages/runtime-web/src/wasmBridge.ts`:
//!
//! ```ts
//! type NmpWasmRuntime = {
//!   handle_json(request: string): unknown;
//!   handle_dispatch_bytes?(bytes: Uint8Array): unknown;
//!   recent_routing_decisions?(): string;
//!   set_snapshot_callback?(callback: SnapshotCallback | null): void;
//! };
//! type NmpWasmModule = {
//!   NmpWasmRuntime?: new () => NmpWasmRuntime;
//!   nmp_encode_npub?: EncodeNpubFn;
//! };
//! ```
//!
//! # Snapshot delivery
//!
//! Snapshots (FlatBuffers merged update-frame bytes) are pushed asynchronously
//! via the callback registered through [`NmpWasmRuntime::set_snapshot_callback`].
//! They are NOT returned inline from `handle_json`; the bridge code in
//! `wasmBridge.ts` sets the callback in its constructor before sending any
//! requests.
//!
//! # Async pump loop
//!
//! On `wasm32` the runtime arms a one-shot `gloo_timers` callback set to 0ms
//! after inbound relay events arrive (via the `set_wake` hook). The callback
//! calls `pump_once()` which drains the inbox, runs the maintenance tick, and
//! fans outbound to relay drivers — then pushes the snapshot.
//!
//! On native (CI / tests) there is no JS runtime so the pump is driven
//! manually by tests or by `NmpRuntimeCore::pump_once`.
//!
//! # Always-compiled non-wasm code
//!
//! `NmpRuntimeCore` (in `super::core`) and all protocol types (in
//! `super::protocol`) are always-compiled. This lets `cargo test -p
//! nmp-browser-runtime` exercise the full routing logic without a wasm target.
//! Only the `#[wasm_bindgen]` wrappers and JS/gloo imports are gated on
//! `cfg(target_arch = "wasm32")`.

pub(crate) mod core;
pub(crate) mod dispatch;
pub(crate) mod dispatch_support;
pub(crate) mod group_discovery;
pub(crate) mod group_events;
pub(crate) mod identity;
pub(crate) mod notifications;
pub(crate) mod protocol;
pub(crate) mod ref_routing;
pub(crate) mod search;
pub(crate) mod store_failure;
pub(crate) mod web_locks;

// ── wasm32 entry point ────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use js_sys::Function;
    use wasm_bindgen::prelude::*;

    use super::core::NmpRuntimeCore;

    // ── Shared-state wrapper ─────────────────────────────────────────────────

    /// Internal shared state, wrapped in `Rc<RefCell<>>` so closures (wake timer,
    /// snapshot sink) can share ownership with `NmpWasmRuntime` without crossing
    /// thread boundaries. Single-threaded wasm32 guarantees no contention.
    struct Inner {
        core: NmpRuntimeCore,
        durable_tab_lock: Option<super::web_locks::DurableTabLock>,
        /// JS function to call with `(bytes: Uint8Array)` on snapshot push.
        snapshot_cb: Option<Function>,
        /// Wake closure built by `set_snapshot_callback` but not yet installed
        /// because the handle did not exist at that point (#2139 BLOCKER 1).
        /// Installed onto the handle the next time a request (notably Start)
        /// populates `core.handle`.
        pending_wake: Option<Rc<dyn Fn()>>,
    }

    impl Inner {
        fn new() -> Self {
            Self {
                core: NmpRuntimeCore::new(),
                durable_tab_lock: None,
                snapshot_cb: None,
                pending_wake: None,
            }
        }

        /// If a wake closure was stored before `Start`, install it now that the
        /// handle exists (#2139 BLOCKER 1 — prevents relay events and signer
        /// completions from being silently dropped on the default NO-OP wake).
        fn try_install_pending_wake(&mut self) {
            if let Some(h) = self.core.handle.as_mut() {
                if let Some(wake) = self.pending_wake.take() {
                    h.set_wake(wake);
                }
            }
        }

        /// Push snapshot bytes via the installed JS callback, if any, when the
        /// kernel reports a real change since the last emitted frame.
        fn push_snapshot_via_js(&mut self) {
            if self.snapshot_cb.is_none() {
                return;
            }
            if let Some(h) = self.core.handle.as_mut() {
                if let Some(bytes) = h.next_frame_if_dirty(true) {
                    if let Some(cb) = &self.snapshot_cb {
                        let arr = js_sys::Uint8Array::from(bytes.as_slice());
                        // D6 — drop the error; a broken callback is not fatal.
                        let _ = cb.call1(&JsValue::NULL, &arr);
                    }
                }
            }
        }

        /// Drive one pump turn, buffer any sign terminal events for the next
        /// `handle_json` call, and push the snapshot (#2139 BLOCKER 2 — was
        /// `let _ = events` which silently discarded sign terminals).
        fn pump_and_push_snapshot(&mut self) {
            let events = self.core.pump_once();
            // Buffer sign terminal events so they are delivered on the next
            // handle_json call rather than being silently dropped.
            self.core.buffer_host_events(events);
            self.push_snapshot_via_js();
        }
    }

    fn schedule_pump(inner: Rc<RefCell<Inner>>, scheduled: Rc<Cell<bool>>) {
        if scheduled.replace(true) {
            return;
        }

        gloo_timers::callback::Timeout::new(0, move || {
            scheduled.set(false);
            let pumped = match inner.try_borrow_mut() {
                Ok(mut guard) => {
                    guard.pump_and_push_snapshot();
                    true
                }
                Err(_) => false,
            };
            if !pumped {
                schedule_pump(inner, scheduled);
            }
        })
        .forget();
    }

    // ── NmpWasmRuntime ─────────────────────────────────────────────────────────

    /// `NmpWasmRuntime` — the browser JS host constructs one instance per Worker,
    /// then drives it with `handle_json` / `handle_dispatch_bytes`.
    ///
    /// Exported to JS as `new NmpWasmRuntime()`.
    #[wasm_bindgen]
    pub struct NmpWasmRuntime {
        inner: Rc<RefCell<Inner>>,
    }

    #[wasm_bindgen]
    impl NmpWasmRuntime {
        /// Construct an unstarted runtime.
        ///
        /// Call `handle_json` with a `WorkerRequest::Hello` then
        /// `WorkerRequest::Start` before any other requests.
        #[wasm_bindgen(constructor)]
        pub fn new() -> NmpWasmRuntime {
            // Install the panic hook once (idempotent).
            crate::install_panic_hook();

            NmpWasmRuntime {
                inner: Rc::new(RefCell::new(Inner::new())),
            }
        }

        /// Handle a JSON-serialised `WorkerRequest` and return a JSON array
        /// of `WorkerEvent`s.
        ///
        /// After the request runs, attempts to install any pending wake closure
        /// (deferred from a `set_snapshot_callback` call that preceded `Start`),
        /// then pushes the updated snapshot (#2139 BLOCKER 1).
        ///
        /// The return value is `unknown` from the JS side; the bridge casts it
        /// via `parseWorkerEvents`. Large binary payloads should go through
        /// `handle_dispatch_bytes` instead.
        pub fn handle_json(&mut self, request: &str) -> JsValue {
            let result = {
                let mut inner = self.inner.borrow_mut();
                let json = inner.core.handle_json_request(request);
                // Install pending wake if Start just populated core.handle.
                inner.try_install_pending_wake();
                inner.push_snapshot_via_js();
                json
            };
            JsValue::from_str(&result)
        }

        /// Handle raw `DispatchEnvelope` bytes (ADR-0064 binary write doorway).
        ///
        /// Avoids JSON round-tripping the binary payload, which would corrupt it.
        /// Returns a JSON array of `WorkerEvent`s (same as `handle_json`).
        pub fn handle_dispatch_bytes(&mut self, bytes: &[u8]) -> JsValue {
            let result = {
                let mut inner = self.inner.borrow_mut();
                let json = inner.core.handle_dispatch_bytes_raw(bytes);
                inner.try_install_pending_wake();
                inner.push_snapshot_via_js();
                json
            };
            JsValue::from_str(&result)
        }

        /// Return a JSON snapshot of recent routing decisions (pull-only,
        /// diagnostic). Does NOT trigger a snapshot push.
        pub fn recent_routing_decisions(&self) -> String {
            self.inner.borrow().core.recent_routing_decisions()
        }

        /// Install (or clear) the snapshot callback.
        ///
        /// The callback is called with a `Uint8Array` of merged FlatBuffers
        /// update-frame bytes whenever the kernel state changes. Pass `null`
        /// to uninstall.
        ///
        /// This wires the async pump wake: when inbound relay events arrive the
        /// relay pool fires a 0ms timer that calls `pump_and_push_snapshot()` on
        /// the shared inner state, which drains inbox + pushes snapshot without
        /// holding the borrow at JS boundary.
        ///
        /// # Wake ordering fix (#2139 BLOCKER 1)
        ///
        /// `wasmBridge.ts` calls `set_snapshot_callback` in its constructor,
        /// BEFORE the host sends `Start`. The handle does not exist yet, so the
        /// wake closure cannot be installed immediately. Instead it is stored in
        /// `pending_wake` and installed onto the handle the next time
        /// `handle_json` (or `handle_dispatch_bytes`) is called after `Start`
        /// creates the handle.
        pub fn set_snapshot_callback(&mut self, cb: Option<Function>) {
            let mut inner = self.inner.borrow_mut();
            inner.snapshot_cb = cb;

            // Build the wake closure. Captures a clone of the Rc so the timer
            // callback can borrow the Inner without holding the current borrow.
            let inner_rc = Rc::clone(&self.inner);
            let wake_scheduled = Rc::new(Cell::new(false));
            let wake: Rc<dyn Fn()> = Rc::new(move || {
                schedule_pump(Rc::clone(&inner_rc), Rc::clone(&wake_scheduled));
            });

            // If the handle already exists, install immediately.
            // Otherwise store for deferred installation after Start (#2139 BLOCKER 1).
            if let Some(h) = inner.core.handle.as_mut() {
                h.set_wake(Rc::clone(&wake));
                inner.pending_wake = None;
            } else {
                inner.pending_wake = Some(wake);
            }
        }

        /// Async pre-`Start` hook: open the durable OPFS-SQLite store and stash it
        /// on the core so the next (synchronous) `Start` injects it instead of an
        /// in-memory store (#1007 PR-7).
        ///
        /// The host MUST `await` this before sending `WorkerRequest::Start`. This
        /// is the async-open-before-`Start` seam: `OpfsSqliteEventStore::open`
        /// acquires the OPFS SyncAccessHandle pool asynchronously — work the
        /// synchronous `handle_start` cannot do, so it is hoisted here and the
        /// ready `Arc<dyn EventStore>` parked on the core for `handle_start` to
        /// `take()` and `inject_store(..)`.
        ///
        /// `app_id` + `database_name` compose the per-app OPFS namespace (see
        /// [`super::core::opfs_database_name`]).
        ///
        /// # Degraded-mode diagnostics (#1007 PR-8)
        ///
        /// On a successful open the ready `Arc<dyn EventStore>` is parked for
        /// `handle_start` to inject. On **open failure** the error is classified
        /// into a **stable reason string** ([`super::store_failure`]) and parked
        /// on the core; `handle_start` threads it through
        /// `BrowserAppBuilder::with_store_open_failure` so the in-memory fallback
        /// session reports the **same** Tier-3 `store_open_failure` diagnostic
        /// the native LMDB degraded-open path emits. Never a silent
        /// pretend-durable: durability is OFF and the host sees exactly why
        /// (Safari < 17.4, private browsing, quota, handle loss, second-tab
        /// pool-lock).
        ///
        /// Gated on `feature = "opfs-sqlite-backend"`: a wasm build without the
        /// durable backend simply has no such hook and starts in-memory.
        #[cfg(feature = "opfs-sqlite-backend")]
        pub async fn prepare_store(&self, app_id: String, database_name: String) {
            let db_name = super::core::opfs_database_name(&app_id, &database_name);
            let lock = match super::web_locks::acquire_durable_tab_lock(&db_name).await {
                Ok(lock) => lock,
                Err(err) => {
                    tracing::warn!(
                        "OPFS-SQLite durable tab lock unavailable for {db_name:?}: {err}; \
                         falling back to in-memory store (durability OFF)"
                    );
                    if let Ok(mut inner) = self.inner.try_borrow_mut() {
                        inner.core.set_store_open_failure(
                            super::store_failure::SECOND_TAB_POOL_LOCK.to_string(),
                        );
                    }
                    return;
                }
            };
            match nmp_store::OpfsSqliteEventStore::open(&db_name).await {
                Ok(store) => {
                    let store: std::sync::Arc<dyn nmp_store::EventStore> =
                        std::sync::Arc::new(store);
                    // Park on the core for handle_start to take(). try_borrow_mut:
                    // the wake timer is the only other borrower and never overlaps
                    // this await-driven call on the single-threaded wasm target.
                    if let Ok(mut inner) = self.inner.try_borrow_mut() {
                        inner.core.set_injected_store(store);
                        inner.durable_tab_lock = Some(lock);
                    }
                }
                Err(err) => {
                    lock.release();
                    // Classify into a stable reason and park it for handle_start to
                    // thread onto the kernel's Tier-3 `store_open_failure`. The
                    // fallback to in-memory is honest: durability is OFF and the
                    // reason is surfaced through the snapshot, not just logged.
                    let reason = super::store_failure::classify_open_failure(&err);
                    tracing::warn!(
                        "OPFS-SQLite open failed for {db_name:?}: {err} \
                         (classified: {reason}); falling back to in-memory store \
                         (durability OFF)"
                    );
                    if let Ok(mut inner) = self.inner.try_borrow_mut() {
                        inner.core.set_store_open_failure(reason.to_string());
                    }
                }
            }
        }
    }

    impl Default for NmpWasmRuntime {
        fn default() -> Self {
            Self::new()
        }
    }

    // ── Free function: nmp_encode_npub ────────────────────────────────────────

    /// Encode a 32-byte secp256k1 public key (64 hex chars) as a JSON object
    /// `{"npub":"npub1…","npubShort":"npub1abc…xyz"}`.
    ///
    /// Returns the JSON string on success, or an empty string if `hex` is not
    /// valid 64-char hex (D6: total on JS boundary — never throws).
    ///
    /// The bridge (`wasmBridge.ts` line 74) calls `JSON.parse(json)` expecting
    /// exactly the `{npub, npubShort}` shape (#2139 BLOCKER 3 — was returning
    /// a bare `npub1…` string which caused `JSON.parse` to throw on the object
    /// destructure).
    ///
    /// Exported to JS as `nmp_encode_npub(hex: string): string`.
    #[wasm_bindgen]
    pub fn nmp_encode_npub(hex: &str) -> String {
        match nmp_nostr_id::encode_npub(hex) {
            Ok(npub) => {
                let npub_short = nmp_core::display::short_npub(hex);
                // serde_json::to_string cannot fail on a simple object literal;
                // unwrap_or_default is a belt-and-suspenders guard only.
                serde_json::json!({ "npub": npub, "npubShort": npub_short }).to_string()
            }
            Err(_) => String::new(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_impl::{nmp_encode_npub, NmpWasmRuntime};

// ── Non-wasm stubs (native CI / doc builds) ───────────────────────────────────

/// Encode a 32-byte secp256k1 public key hex as a JSON object
/// `{"npub":"npub1…","npubShort":"npub1abc…xyz"}`.
///
/// On native this is a plain Rust function (no `#[wasm_bindgen]`); the wasm
/// target exports it via `nmp_encode_npub` above. Returns an empty string on
/// invalid input (D6: total — no panic, no error value on the JS boundary).
///
/// (#2139 BLOCKER 3 — was returning a bare `npub1…` string).
#[cfg(not(target_arch = "wasm32"))]
pub fn nmp_encode_npub(hex: &str) -> String {
    match nmp_nostr_id::encode_npub(hex) {
        Ok(npub) => {
            let npub_short = nmp_core::display::short_npub(hex);
            serde_json::json!({ "npub": npub, "npubShort": npub_short }).to_string()
        }
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_npub_returns_json_object() {
        let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let json = nmp_encode_npub(hex);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
        assert!(
            parsed["npub"].as_str().unwrap_or("").starts_with("npub1"),
            "npub field must start with npub1"
        );
        assert!(
            !parsed["npubShort"].as_str().unwrap_or("").is_empty(),
            "npubShort field must be non-empty"
        );
    }

    #[test]
    fn encode_npub_invalid_returns_empty() {
        let result = nmp_encode_npub("not-valid");
        assert!(result.is_empty(), "result={result:?}");
    }
}
