//! wasm32 composition-root entry point for the Chirp web client.
//!
//! Exposes `NmpWasmRuntime` — a `#[wasm_bindgen]` struct that wraps
//! [`nmp_wasm::WasmRuntime`] and installs [`setup_chirp_web_feeds`] as a
//! pre-start composition hook so it observes the final event store selected by
//! ADR-0054 boot-time injection.
//!
//! # JS API surface
//!
//! The class name (`NmpWasmRuntime`) and the method names (`handle_json`,
//! `handle_dispatch_bytes`, `set_snapshot_callback`, `set_event_callback`,
//! `recent_routing_decisions`) are stable. The generated JS module file is
//! `nmp_app_chirp_web.js` (derived from the crate name); `wasmBridge.ts` sets
//! its `defaultModulePath` constant accordingly.
//!
//! ADR-0064 §5 / #1743: there is NO `dispatch_app_action_async` Promise
//! entrypoint. Writes route through the typed `handle_dispatch_bytes` doorway;
//! signing is the `BeginSign` capability
//! round-trip driven by pure message re-entry — no `Arc<dyn Signer>` is awaited
//! inside a publish flow.
//!
//! # Identity-change hook
//!
//! `handle_json` detects a `SetIdentity` request and calls
//! [`ChirpWebFeedSetup::notify_account_changed`] after `WasmRuntime::handle`
//! returns once the pre-start setup has run. On a failed identity install the
//! active account slot is unchanged, so `notify_account_changed` short-circuits
//! with no engine reset. On success the follow set is re-seeded and the
//! perspective reset clears the prior account's roots from the engine.
//!
//! # Doctrine
//!
//! * **D0** — no protocol nouns in this module's public API; the surface is
//!   `#[wasm_bindgen]` methods that mirror the prior entry point.
//! * **D6** — every failure mode at the protocol layer resolves as a
//!   `WorkerEvent::Error` or `WorkerEvent::CapabilityFailure` in the returned
//!   JSON array, never as a Promise rejection. Promise rejection is reserved
//!   for response JSON serialisation failure — an in-memory value that is
//!   supposed to be serialisable is not, indicating a compile-time regression
//!   in the serde impl. The JS host's catch boundary (in `wasmBridge.ts`)
//!   converts that rejection to a synthetic `error` event so the JS caller
//!   still sees data, not an unhandled Promise failure.
//! * **D8** — `handle_json` and `handle_dispatch_bytes` are synchronous; writes
//!   and signing ride the message-driven worker protocol, never an in-flow
//!   `Promise` over a persistent signer.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use nmp_wasm::{WasmRuntime, WasmRuntimeError, WorkerEvent, WorkerRequest};

use crate::composition::{setup_chirp_web_feeds, ChirpWebFeedSetup};

/// wasm32 composition root for the Chirp web client.
///
/// Constructs a [`WasmRuntime`], defers [`setup_chirp_web_feeds`] until the
/// runtime's pre-start hook, and exposes the same JS API the prior `nmp-wasm`
/// entry point provided. The JS class name is intentionally preserved so
/// `wasmBridge.ts` needs only a single import-path update.
#[wasm_bindgen]
pub struct NmpWasmRuntime {
    runtime: WasmRuntime,
    setup: Rc<RefCell<Option<ChirpWebFeedSetup>>>,
}

#[wasm_bindgen]
impl NmpWasmRuntime {
    /// Construct the composition root.
    ///
    /// Installs the console panic hook (idempotent), creates a fresh
    /// `WasmRuntime`, and registers a pre-start hook that wires the
    /// `nmp.feed.home` OP-feed projection after ADR-0054 store injection has
    /// selected the final backend. The feed observer and post-tick drain are
    /// live before relay drivers start.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        let mut runtime = WasmRuntime::new();
        let setup = Rc::new(RefCell::new(None));
        let setup_slot = Rc::clone(&setup);
        runtime
            .install_before_start_hook(move |runtime| {
                *setup_slot.borrow_mut() = Some(setup_chirp_web_feeds(runtime));
            })
            .expect("fresh runtime accepts pre-start composition hook");
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
    /// [`ChirpWebFeedSetup::notify_account_changed`] after the pre-start setup
    /// has run, so the follow set is re-seeded from the newly-active account
    /// and the engine resets on a real perspective change.
    ///
    /// # D6 — errors as data
    ///
    /// This method is **infallible at the protocol layer**: parse failures and
    /// host config errors resolve to a `WorkerEvent::Error` in the returned
    /// JSON array. The method signature `Result<JsValue, JsValue>` is kept
    /// because `wasm-bindgen` requires it for Promise-resolution; Promise
    /// rejection (the `Err` path) is reserved for response serialisation
    /// failure, documented in the module-level D6 note.
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
        };
        if is_set_identity {
            if let Some(setup) = self.setup.borrow().as_ref() {
                // A failed identity install leaves the slot unchanged, so this
                // is a no-op. Successful install re-seeds the follow set and
                // resets the feed perspective.
                setup.notify_account_changed();
            }
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

    /// Process a binary `dispatch_bytes` request (#1008 / ADR-0064).
    ///
    /// Receives the raw `Uint8Array` of a finished `DispatchEnvelope` FlatBuffers
    /// root directly — avoids the `JSON.stringify(Uint8Array) → {}` corruption
    /// that occurs on the `handle_json` path (a `Uint8Array` serialises as an
    /// empty object in JSON, not a number array). The JS host calls this method
    /// for `dispatch_bytes` requests instead of routing through `handle_json`.
    ///
    /// Returns the same JSON-serialised `WorkerEvent[]` as `handle_json` (control
    /// events only; `UpdateBytes` frames are drained through the snapshot callback).
    ///
    /// # D6 — fail closed
    ///
    /// A malformed envelope is returned as a data-shaped `[{ type: "error",
    /// code: "dispatch_envelope_rejected", ... }]` — never as a Promise rejection.
    #[wasm_bindgen]
    pub fn handle_dispatch_bytes(&mut self, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let events = self.runtime.dispatch_bytes(bytes);
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

    /// Install (or clear) the JS callback the runtime invokes for async
    /// control events produced outside a synchronous `handle_*` call.
    #[wasm_bindgen]
    pub fn set_event_callback(&mut self, callback: Option<js_sys::Function>) {
        self.runtime.set_event_callback(callback);
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
