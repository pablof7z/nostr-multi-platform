//! UniFFI binding surface for the NMP native app lifecycle and byte doorway.
//!
//! # Scope (#2389 — base runtime object)
//!
//! This crate exposes the minimal native API:
//! * `NmpApp` — Arc-wrapped runtime object (construct/start/configure/stop/
//!   shutdown/reset + dispatch + update-sink).
//! * `UpdateSink` — callback interface (Rust→shell push; `on_update(frame)` is
//!   the NMPU FlatBuffers frame).
//! * `DispatchOutcome` — typed result of `dispatch_action`.
//!
//! # Pre-start composition seams (NOT in this slice)
//!
//! Storage path, `declare_consumed_projections`, capability/signer registration
//! are intentionally absent. They layer in via:
//! * C2 — signer registration
//! * C5 — capability registration
//! * C6 — storage path + projection config
//!
//! # Drift gate
//!
//! The generated Swift/Kotlin bindings in `generated/` are checked in and
//! pinned against a CI drift script (`ci/check-uniffi-bindings-drift.sh`).
//! Regenerate with:
//!   bash ci/check-uniffi-bindings-drift.sh --regen

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_native_runtime::{
    dispatch_action_bytes_typed, new_app, NmpApp as RuntimeApp, UpdateListener, DEFAULT_EMIT_HZ,
    DEFAULT_VISIBLE_LIMIT,
};

uniffi::setup_scaffolding!();

// ── Stateless helpers (C1 — NIP-19, NIP-21, content, intent) ─────────────────
// Each sub-surface lives in its own file; this keeps C2–C7 file-disjoint.
pub mod stateless;
pub use stateless::NmpError;

// ── Identity / signer / relay (C2 — account, relay, broker, external) ────────
pub mod identity;

// ── Reference resolution (C3 — resolve_ref, profile, event embed) ─────────────
pub mod refs;
pub use refs::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefShape, ResolveMetadata};

// ── Capability, action-lane, publish-control (C4) ────────────────────────────
pub mod capability;
pub use capability::{ActionResultObserver, CapabilitySink};

// ── Feed viewport, URI routing, search sessions (C5) ─────────────────────────
pub mod sessions;
pub use sessions::FeedSessionHandle;

// ── Lifecycle signals, storage config, projection config, diagnostics (C6) ───
pub mod runtime;

// ── ADR-0058 mirror pull-page surface (C7) ───────────────────────────────────
pub mod mirror;
pub use mirror::MirrorPullResult;

// ── Typed dispatch outcome ────────────────────────────────────────────────────

/// Typed outcome of a `dispatch_action` call.
///
/// Exactly one of `correlation_id` (accepted) or `error` (rejected/failed)
/// will be `Some`. `code` is `Some` only for coded rejections — it carries
/// the stable machine-readable token (issue #1734) alongside the
/// human-readable `error`. This field is load-bearing:
/// `nmp-ffi`'s `coded_rejection_tests.rs:122` guards the same invariant on
/// the C-ABI side.
#[derive(uniffi::Record, Debug, Clone)]
pub struct DispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    /// Machine-readable code for coded rejections; `None` for plain errors.
    pub code: Option<String>,
}

// ── Callback interface ────────────────────────────────────────────────────────

/// Rust→shell push interface: receives NMPU FlatBuffers update frames.
///
/// Implementations MUST NOT call back into any `NmpApp` method from within
/// `on_update` — reentrancy is forbidden (the quiescence gate would deadlock).
#[uniffi::export(callback_interface)]
pub trait UpdateSink: Send + Sync {
    /// Called on every NMPU frame emitted by the runtime.
    ///
    /// `frame` is a copy of the original `&[u8]` — the copy is made BEFORE the
    /// foreign call so no Rust state is held across the Swift/Kotlin call.
    fn on_update(&self, frame: Vec<u8>);
}

/// Rust→shell lifecycle observer.
///
/// Receives the same phase codes as the C ABI:
/// * `0` — foreground
/// * `1` — background
///
/// Implementations MUST NOT call `set_lifecycle_callback` from inside this
/// method; the setter drains in-flight callbacks before returning.
#[uniffi::export(callback_interface)]
pub trait LifecycleSink: Send + Sync {
    fn on_lifecycle_phase(&self, phase: u32);
}

// ── App object ────────────────────────────────────────────────────────────────

/// Arc-wrapped NMP native runtime.
///
/// Wraps `nmp_native_runtime::NmpApp` behind a UniFFI `Object` interface so
/// Swift/Kotlin hold `Arc<NmpApp>` semantics. Lifecycle:
/// 1. `NmpApp()` — construct (no IO, no actor started).
/// 2. `setUpdateSink(sink)` — register the NMPU frame observer.
/// 3. `start(...)` — spawn the actor and begin the event loop.
/// 4. `stop()` / `configure(...)` — pause/reconfigure.
/// 5. `shutdown()` — explicit teardown; `Arc` drop is the fallback.
#[derive(uniffi::Object)]
pub struct NmpApp {
    inner: RuntimeApp,
    search_handles: Mutex<BTreeMap<String, nmp_native_runtime::Nip50SearchHandle>>,
}

#[uniffi::export]
impl NmpApp {
    /// Construct a new `NmpApp`. No IO is performed; the actor is NOT started
    /// yet. Call `start` after all pre-start configuration.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(NmpApp {
            inner: new_app(),
            search_handles: Mutex::new(BTreeMap::new()),
        })
    }

    /// Start the runtime actor with the given rendering limits.
    ///
    /// Clamp rules (parity with C-ABI `nmp_app_start`):
    /// * `visible_limit == 0` → use default (100). Otherwise clamp(1..=500).
    /// * `emit_hz == 0` → use default (6 Hz). Otherwise clamp(1..=12).
    pub fn start(&self, visible_limit: u32, emit_hz: u32) {
        self.inner
            .start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
    }

    /// Reconfigure rendering limits without restarting. Same clamp rules as
    /// `start`.
    pub fn configure(&self, visible_limit: u32, emit_hz: u32) {
        self.inner
            .configure_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
    }

    /// Signal the kernel to pause event processing (no data loss).
    pub fn stop(&self) {
        self.inner.stop_runtime();
    }

    /// Signal the kernel to reset (clears transient state).
    pub fn reset(&self) {
        self.inner.reset_runtime();
    }

    /// Explicit idempotent teardown: clears the update sink, sends Shutdown,
    /// and joins the actor + listener threads. Safe to call multiple times.
    ///
    /// Named `shutdown` (NOT `close`) to avoid Kotlin `AutoCloseable`
    /// friction discovered in #2149. `Arc` drop is the fallback.
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }

    /// Register (or clear) the NMPU frame observer.
    ///
    /// After this returns the previous sink is guaranteed to be neither
    /// registered nor mid-invocation (quiescence contract, identical to the
    /// C-ABI `nmp_app_set_update_callback` guarantee). Pass `None` to clear.
    ///
    /// The `frame` bytes passed to `on_update` are a freshly-copied `Vec<u8>`;
    /// no Rust state is held across the foreign call.
    ///
    /// # UniFFI 0.29 type note
    /// UniFFI 0.29 passes callback interface implementations as `Box<dyn Trait>`
    /// (unique ownership). The sink is moved into the update-listener closure
    /// which is then owned by the runtime's `Arc<UpdateListenerGate>`.
    pub fn set_update_sink(&self, sink: Option<Box<dyn UpdateSink>>) {
        let listener: Option<UpdateListener> = sink.map(|s| {
            // Wrap in Arc so the closure is `Sync` (required by UpdateListener).
            let s: Arc<dyn UpdateSink> = Arc::from(s);
            Arc::new(move |bytes: &[u8]| {
                // Copy BEFORE foreign call — no Rust lock held here.
                let frame = bytes.to_vec();
                // Panic containment: a Swift/Kotlin abort must not unwind
                // into the Rust update-listener thread (D6).
                let s = Arc::clone(&s);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    s.on_update(frame)
                }));
            }) as UpdateListener
        });
        self.inner.set_update_listener(listener);
    }

    /// Dispatch an NMPD FlatBuffers action envelope and return the outcome.
    ///
    /// * `DispatchOutcome.correlation_id` — present on acceptance (the
    ///   HOST-SUPPLIED envelope id, not a kernel-minted one, per ADR-0064 §4).
    /// * `DispatchOutcome.error` — present on rejection or post-mint failure.
    /// * `DispatchOutcome.code` — present for coded rejections (issue #1734).
    ///
    /// D6 fail-closed: never throws; every error surfaces as a populated
    /// `DispatchOutcome.error`. D8: non-blocking channel send.
    pub fn dispatch_action(&self, envelope: Vec<u8>) -> DispatchOutcome {
        let out = dispatch_action_bytes_typed(&self.inner, &envelope);
        DispatchOutcome {
            correlation_id: out.correlation_id,
            error: out.error,
            code: out.code,
        }
    }
}

// ── Clamp helpers (parity with nmp-ffi/src/app_lifecycle_ffi.rs) ─────────────

/// Clamp `visible_limit` identically to `nmp_app_start` / `nmp_app_configure`.
/// `0` → `DEFAULT_VISIBLE_LIMIT` (100); otherwise clamp(1..=500).
fn clamp_visible(visible_limit: u32) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

/// Clamp `emit_hz` identically to `nmp_app_start` / `nmp_app_configure`.
/// `0` → `DEFAULT_EMIT_HZ` (6); otherwise clamp(1..=12).
fn clamp_emit_hz(emit_hz: u32) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
