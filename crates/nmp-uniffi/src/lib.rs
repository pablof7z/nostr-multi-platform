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

use nmp_native_runtime::{new_app, NmpApp as RuntimeApp};
pub use nmp_uniffi_support::{clamp_emit_hz, clamp_visible};

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
pub use sessions::{FeedLoadStatus, FeedLoadStopReason, FeedSessionHandle};

// ── Typed dispatch outcome ────────────────────────────────────────────────────

/// Typed outcome of a `dispatch_action` call.
///
/// Exactly one of `correlation_id` (accepted) or `error` (rejected/failed)
/// will be `Some`. `code` is `Some` only for coded rejections — it carries
/// the stable machine-readable token (issue #1734) alongside the
/// human-readable `error`. This field is load-bearing: this crate's
/// `dispatch_wrapper_passes_through_code_field` test guards the invariant.
#[derive(uniffi::Record, Debug, Clone)]
pub struct DispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    /// Machine-readable code for coded rejections; `None` for plain errors.
    pub code: Option<String>,
}

impl From<nmp_uniffi_support::DispatchOutcome> for DispatchOutcome {
    fn from(out: nmp_uniffi_support::DispatchOutcome) -> Self {
        DispatchOutcome {
            correlation_id: out.correlation_id,
            error: out.error,
            code: out.code,
        }
    }
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
    /// Inner runtime handle. App-specific composition roots must use the
    /// constrained pre-start facade hook below instead of reaching through
    /// the UniFFI object and owning native runtime internals.
    inner: RuntimeApp,
    search_handles: Mutex<BTreeMap<String, nmp_native_runtime::Nip50SearchHandle>>,
}

impl NmpApp {
    /// Run an app-owned composition installer against the runtime before
    /// `start`.
    ///
    /// This is intentionally Rust-only. It exists for app facade crates that
    /// must install proprietary composition while native shells are holding the
    /// generated UniFFI object. Do not use this for lifecycle, dispatch, refs,
    /// storage, or any operation that already has a typed UniFFI method.
    #[allow(invalid_reference_casting)]
    pub fn configure_pre_start_for_app_facade<R>(
        &self,
        configure: impl FnOnce(&mut RuntimeApp) -> R,
    ) -> R {
        // SAFETY: app facade crates call this during one-shot composition before
        // `start`, while no runtime actor exists and no shell callback can
        // observe `inner`. The raw cast is centralized here so app crates do not
        // own `NmpApp` internals or duplicate Arc-pointer mutation logic.
        let inner = unsafe {
            let ptr = std::ptr::addr_of!(self.inner) as *mut RuntimeApp;
            &mut *ptr
        };
        configure(inner)
    }
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
        nmp_uniffi_support::start_runtime(&self.inner, visible_limit, emit_hz);
    }

    /// Reconfigure rendering limits without restarting. Same clamp rules as
    /// `start`.
    pub fn configure(&self, visible_limit: u32, emit_hz: u32) {
        nmp_uniffi_support::configure_runtime(&self.inner, visible_limit, emit_hz);
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
        nmp_uniffi_support::set_update_sink(&self.inner, sink, |sink, frame| {
            sink.on_update(frame);
        });
    }

    /// Dispatch an NMPD FlatBuffers action envelope and return the outcome.
    ///
    /// * `DispatchOutcome.correlation_id` — present on acceptance (the
    ///   HOST-SUPPLIED envelope id, not a kernel-minted one, per ADR-0071 §4).
    /// * `DispatchOutcome.error` — present on rejection or post-mint failure.
    /// * `DispatchOutcome.code` — present for coded rejections (issue #1734).
    ///
    /// D6 fail-closed: never throws; every error surfaces as a populated
    /// `DispatchOutcome.error`. D8: non-blocking channel send.
    pub fn dispatch_action(&self, envelope: Vec<u8>) -> DispatchOutcome {
        nmp_uniffi_support::dispatch_action_vec(&self.inner, envelope).into()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
