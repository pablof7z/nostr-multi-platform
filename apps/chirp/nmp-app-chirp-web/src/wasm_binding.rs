//! wasm32 composition-root entry point for the Chirp web client.
//!
//! Exposes `NmpWasmRuntime` — a `#[wasm_bindgen]` struct that wraps
//! [`nmp_wasm::WasmRuntime`] and calls [`setup_chirp_web_feeds`] at
//! construction so the `nmp.feed.home` typed projection is registered and
//! produced in every snapshot frame that leaves the wasm module.
//!
//! # JS API surface (unchanged from `nmp-wasm`)
//!
//! The class name (`NmpWasmRuntime`) and all four method names
//! (`handle_json`, `set_snapshot_callback`, `recent_routing_decisions`,
//! `dispatch_app_action_async`) are identical to the binding that previously
//! lived in `crates/nmp-wasm/src/lib.rs`. The generated JS module file is now
//! `nmp_app_chirp_web.js` (derived from the crate name); `wasmBridge.ts`
//! updates only its `defaultModulePath` constant.
//!
//! # Identity-change hook
//!
//! `handle_json` detects a `SetSigner` request and calls
//! [`ChirpWebFeedSetup::notify_account_changed`] after `WasmRuntime::handle`
//! returns. The call is unconditional: on a failed signer install the active
//! account slot is unchanged, so `notify_account_changed`'s `last_seen` guard
//! short-circuits with no engine reset. On success the follow set is re-seeded
//! and any identity switch clears the prior account's roots from the engine.
//!
//! # Doctrine
//!
//! * **D0** — no protocol nouns in this module's public API; the surface is
//!   `#[wasm_bindgen]` methods that mirror the prior entry point.
//! * **D6** — every failure mode surfaces as a JS-side error string or a
//!   `CapabilityFailure` event; no silent drops.
//! * **D8** — `handle_json` is synchronous; the only async path is
//!   `dispatch_app_action_async` which returns a `Promise`.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use nmp_wasm::{AppActionDispatch, WorkerEvent, WorkerRequest, WasmRuntime};

use crate::composition::{setup_chirp_web_feeds, ChirpWebFeedSetup};

/// wasm32 composition root for the Chirp web client.
///
/// Constructs a [`WasmRuntime`], calls [`setup_chirp_web_feeds`] to register
/// the `nmp.feed.home` typed projection, and exposes the same JS API the prior
/// `nmp-wasm` entry point provided. The JS class name is intentionally
/// preserved so `wasmBridge.ts` needs only a single import-path update.
#[wasm_bindgen]
pub struct NmpWasmRuntime {
    runtime: WasmRuntime,
    setup: ChirpWebFeedSetup,
}

#[wasm_bindgen]
impl NmpWasmRuntime {
    /// Construct the composition root.
    ///
    /// Installs the console panic hook (idempotent), creates a fresh
    /// `WasmRuntime`, and wires the `nmp.feed.home` OP-feed projection by
    /// calling [`setup_chirp_web_feeds`]. The feed observer and post-tick
    /// drain are live from this point; they fire on the first relay frame
    /// after `Start`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        let runtime = WasmRuntime::new();
        let setup = setup_chirp_web_feeds(&runtime);
        Self { runtime, setup }
    }

    /// Process one `WorkerRequest` JSON envelope and return a JSON array of
    /// `WorkerEvent`s.
    ///
    /// `UpdateBytes` events are drained through the registered snapshot
    /// callback (if any) before this method returns, so the JS host sees
    /// binary frames on the callback channel and control events on the JSON
    /// return value — same contract as the prior `nmp-wasm` entry point.
    ///
    /// A `SetSigner` request additionally triggers
    /// [`ChirpWebFeedSetup::notify_account_changed`] so the follow set is
    /// re-seeded from the newly-active account and the engine resets on an
    /// actual identity switch.
    pub fn handle_json(&mut self, request: &str) -> Result<JsValue, JsValue> {
        let req: WorkerRequest = serde_json::from_str(request)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let is_set_signer = matches!(req, WorkerRequest::SetSigner(_));
        let events = self
            .runtime
            .handle(req)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if is_set_signer {
            // Unconditional: failed signer install leaves the slot unchanged,
            // so notify_account_changed's last_seen guard is a no-op. Successful
            // install re-seeds the follow set and detects any pubkey change.
            self.setup.notify_account_changed();
        }
        // Route UpdateBytes through the callback channel; collect control events.
        let callback_handle = self.runtime.snapshot_callback_handle().clone();
        let mut control_events: Vec<WorkerEvent> = Vec::with_capacity(events.len());
        for event in events {
            match event {
                WorkerEvent::UpdateBytes { bytes } => {
                    push_bytes_if_callback(&callback_handle, &bytes);
                }
                other => control_events.push(other),
            }
        }
        serde_json::to_string(&control_events)
            .map(|s| JsValue::from_str(&s))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Install (or clear) the JS callback the runtime invokes whenever a
    /// relay-driven kernel mutation produces a fresh snapshot.
    ///
    /// Delegates to [`WasmRuntime::set_snapshot_callback`] unchanged.
    #[wasm_bindgen]
    pub fn set_snapshot_callback(&mut self, callback: Option<js_sys::Function>) {
        self.runtime.set_snapshot_callback(callback);
    }

    /// JSON snapshot of the kernel's recent routing decisions.
    ///
    /// Delegates to [`WasmRuntime::recent_routing_decisions`] unchanged.
    #[wasm_bindgen]
    pub fn recent_routing_decisions(&self) -> String {
        self.runtime.recent_routing_decisions()
    }

    /// Async dispatch entry point for app-level write actions that need a
    /// signer.
    ///
    /// Accepts a JSON-serialised [`AppActionDispatch`] and returns a
    /// `js_sys::Promise` resolving to the JSON-serialised [`WorkerEvent`].
    /// Delegates to [`WasmRuntime::start_publish_app_action`] unchanged.
    #[wasm_bindgen]
    pub fn dispatch_app_action_async(&mut self, request_json: &str) -> js_sys::Promise {
        let parsed: Result<AppActionDispatch, _> = serde_json::from_str(request_json);
        let dispatch = match parsed {
            Ok(d) => d,
            Err(err) => {
                let message =
                    format!("dispatch_app_action_async: invalid request_json: {err}");
                return js_sys::Promise::reject(&JsValue::from_str(&message));
            }
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let now_secs = (js_sys::Date::now() / 1000.0) as u64;
        let future = self.runtime.start_publish_app_action(
            dispatch.action,
            dispatch.correlation_id,
            now_secs,
        );
        future_to_promise(async move {
            let event = future.await;
            serde_json::to_string(&event)
                .map(|s| JsValue::from_str(&s))
                .map_err(|e| JsValue::from_str(&e.to_string()))
        })
    }
}

impl Default for NmpWasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Push `bytes` through the JS snapshot callback, if any.
///
/// Mirrors `nmp_wasm::snapshot::push_bytes_if_callback` (which is
/// `pub(crate)`) without re-exporting it. Kept private to this module.
fn push_bytes_if_callback(
    callback: &Rc<RefCell<Option<js_sys::Function>>>,
    bytes: &[u8],
) {
    let callback_ref = callback.borrow();
    let Some(callback_fn) = callback_ref.as_ref() else {
        return;
    };
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let _ = callback_fn.call1(&JsValue::NULL, &array.into());
}
