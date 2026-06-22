//! wasm32 composition-root entry point for the Chirp web client.
//!
//! Exposes `NmpWasmRuntime` — a `#[wasm_bindgen]` struct that wraps
//! [`nmp_wasm::WasmRuntime`] and calls [`setup_chirp_web_feeds`] at
//! construction so the `nmp.feed.home` typed projection is registered and
//! produced in every snapshot frame that leaves the wasm module.
//!
//! # JS API surface
//!
//! The class name (`NmpWasmRuntime`) and the method names (`handle_json`,
//! `set_snapshot_callback`, `recent_routing_decisions`) are stable. The
//! generated JS module file is `nmp_app_chirp_web.js` (derived from the crate
//! name); `wasmBridge.ts` sets its `defaultModulePath` constant accordingly.
//!
//! ADR-0064 §5 / #1743: there is NO `dispatch_app_action_async` Promise
//! entrypoint. Writes route through the typed `WorkerRequest::DispatchBytes`
//! doorway (via `handle_json`); signing is the `BeginSign` capability
//! round-trip driven by pure message re-entry — no `Arc<dyn Signer>` is awaited
//! inside a publish flow.
//!
//! # Identity-change hook
//!
//! `handle_json` detects a `SetIdentity` request and calls
//! [`ChirpWebFeedSetup::notify_account_changed`] after `WasmRuntime::handle`
//! returns. The call is unconditional: on a failed identity install the active
//! account slot is unchanged, so `notify_account_changed` short-circuits with
//! no engine reset. On success the follow set is re-seeded and the perspective
//! reset clears the prior account's roots from the engine.
//!
//! # Doctrine
//!
//! * **D0** — no protocol nouns in this module's public API; the surface is
//!   `#[wasm_bindgen]` methods that mirror the prior entry point.
//! * **D6** — every failure mode at the protocol layer resolves as a
//!   `WorkerEvent::Error` or `WorkerEvent::CapabilityFailure` in the returned
//!   JSON array, never as a Promise rejection. Promise rejection is reserved
//!   for two catastrophic binding failures that cannot be expressed as a
//!   `WorkerEvent` without the runtime being in an undefined state:
//!   (a) `WasmRuntimeError::KernelContract` — a `KernelReducer` invariant was
//!   violated (e.g. `Start` returned something other than `Started`); the
//!   runtime is permanently broken and no further events can be trusted;
//!   (b) `WorkerEvent` JSON serialisation failure — an in-memory value that is
//!   supposed to be serialisable is not, indicating a compile-time regression
//!   in the serde impl. In both cases the JS host's catch boundary (in
//!   `wasmBridge.ts`) converts the rejection to a synthetic `error` event so
//!   the JS caller still sees data, not an unhandled Promise failure.
//! * **D8** — `handle_json` is synchronous; writes (including signing) ride the
//!   message-driven worker protocol, never an in-flow `Promise` over a
//!   persistent signer.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use nmp_wasm::{WasmRuntime, WasmRuntimeError, WorkerEvent, WorkerRequest};

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

    /// Process one `WorkerRequest` JSON envelope and return a JSON-serialised
    /// array of `WorkerEvent`s.
    ///
    /// `UpdateBytes` events are drained through the registered snapshot
    /// callback (if any) before this method returns, so the JS host sees
    /// binary frames on the callback channel and control events on the JSON
    /// return value — same contract as the prior `nmp-wasm` entry point.
    ///
    /// A `SetIdentity` request additionally triggers
    /// [`ChirpWebFeedSetup::notify_account_changed`] so the follow set is
    /// re-seeded from the newly-active account and the engine resets on a
    /// real perspective change.
    ///
    /// # D6 — errors as data
    ///
    /// This method is **infallible at the protocol layer**: parse failures and
    /// host config errors resolve to a `WorkerEvent::Error` in the returned
    /// JSON array. The method signature `Result<JsValue, JsValue>` is kept
    /// because `wasm-bindgen` requires it for Promise-resolution; Promise
    /// rejection (the `Err` path) is reserved for the two catastrophic cases
    /// documented in the module-level D6 note — `KernelContract` violations
    /// and response serialisation failures. Both are non-recoverable: the
    /// runtime is in an undefined state and no further requests should be sent.
    pub fn handle_json(&mut self, request: &str) -> Result<JsValue, JsValue> {
        // D6: parse failures are protocol-layer errors — resolve as data.
        let req: WorkerRequest = match serde_json::from_str(request) {
            Ok(r) => r,
            Err(err) => {
                let events = vec![WorkerEvent::Error {
                    code: "parse_error".to_string(),
                    message: err.to_string(),
                    correlation_id: None,
                }];
                return serialize_events_to_js(&events);
            }
        };
        let is_set_identity = matches!(req, WorkerRequest::SetIdentity(_));
        let events = match self.runtime.handle(req) {
            Ok(evts) => evts,
            Err(WasmRuntimeError::InvalidConfig(msg)) => {
                // D6: host configuration errors are protocol-layer data, not
                // catastrophic failures — the runtime is still consistent.
                return serialize_events_to_js(&[WorkerEvent::Error {
                    code: "invalid_config".to_string(),
                    message: msg,
                    correlation_id: None,
                }]);
            }
            Err(WasmRuntimeError::KernelContract(msg)) => {
                // Catastrophic: a KernelReducer invariant was violated. The
                // runtime is in an undefined state; Promise rejection tells the
                // JS host not to send further requests. The wasmBridge.ts catch
                // boundary converts this to a synthetic error event.
                return Err(JsValue::from_str(&format!("kernel_contract: {msg}")));
            }
        };
        if is_set_identity {
            // Unconditional: a failed identity install leaves the slot unchanged,
            // so notify_account_changed is a no-op. Successful install
            // re-seeds the follow set and resets the feed perspective.
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
        serialize_events_to_js(&control_events)
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
}

impl Default for NmpWasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Serialise `events` to a JSON string `JsValue`.
///
/// Returns `Ok(JsValue)` on success. Returns `Err(JsValue)` only when
/// serialisation itself fails — a catastrophic serde regression, not a
/// recoverable protocol error. Callers document this as the one remaining
/// Promise-reject path (see the module-level D6 note).
fn serialize_events_to_js(events: &[WorkerEvent]) -> Result<JsValue, JsValue> {
    serde_json::to_string(events)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| JsValue::from_str(&format!("serialize_error: {e}")))
}

/// Push `bytes` through the JS snapshot callback, if any.
///
/// Mirrors `nmp_wasm::snapshot::push_bytes_if_callback` (which is
/// `pub(crate)`) without re-exporting it. Kept private to this module.
fn push_bytes_if_callback(callback: &Rc<RefCell<Option<js_sys::Function>>>, bytes: &[u8]) {
    let callback_ref = callback.borrow();
    let Some(callback_fn) = callback_ref.as_ref() else {
        return;
    };
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let _ = callback_fn.call1(&JsValue::NULL, &array.into());
}

/// Encode a 64-char hex pubkey as `{ "npub": "npub1…", "npubShort": "npub1abcd…wxyz" }`.
///
/// Both forms are produced by the canonical Rust NIP-19 encoder
/// ([`nmp_core::nip19::encode_npub`]) and Rust-side truncation — never by the
/// browser (aim.md §6.9: shells must not bech32-encode or reformat npubs
/// locally). Returns `null` on an invalid hex pubkey (D6: no panic, no silent
/// bad output). The web shell calls this to populate `ProfileWire.npub` /
/// `npubShort` from the raw hex the kernel projection emits (ADR-0032).
#[wasm_bindgen]
#[must_use]
pub fn nmp_encode_npub(hex: &str) -> Option<String> {
    let npub = nmp_core::nip19::encode_npub(hex).ok()?;
    // `npub1` + 58 data chars + checksum. Truncate the data section, keeping the
    // `npub1` prefix and the trailing chars, with an ellipsis — a stable,
    // Rust-owned short form (mirrors the tui/desktop `npub_short`).
    let short = if npub.len() > 20 {
        format!("{}…{}", &npub[..10], &npub[npub.len() - 6..])
    } else {
        npub.clone()
    };
    // Hand-built JSON: both values are bech32 (ASCII `[a-z0-9]` + `…`), so no
    // escaping is needed beyond the fixed structure.
    Some(format!("{{\"npub\":\"{npub}\",\"npubShort\":\"{short}\"}}"))
}
