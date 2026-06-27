//! UniFFI app-loop lane for Android (M14-0 / issue #2129).
//!
//! Replaces the following deleted JNI app-loop entry points:
//!   `nativeNew`, `nativeStart`, `nativeStop`, `nativeClose`, `nativeFree`,
//!   `nativeSetUpdateListener`, `nativeClearUpdateListener`,
//!   `nativeDispatchIntentBytes`, `nativeDispatchActionBytes`.
//!
//! The app-loop shape is now:
//!   Kotlin creates `AppHandle` via UniFFI → `start()` → dispatch/update
//!   loop (via `dispatch_action_bytes` / `dispatch_action_json` +
//!   `set_update_sink`) → `stop()` → `close()`.
//!
//! FlatBuffers bytes are preserved byte-for-byte across the boundary.
//!
//! ## Residual JNI lanes (staged for future migration)
//!
//! The following JNI symbols remain and are NOT app-loop duplicates —
//! they are distinct responsibilities on the existing Rust/JNI path until
//! their own staged issue lands:
//!
//! * **signer.rs**: `nativeSignInBunker`, `nativeCancelBunkerHandshake`,
//!   `nativeNostrConnectUri`
//! * **external_signer.rs**: `nativeSignInNip55`, `nativeDeliverSignerResponse`,
//!   `nativeSetSignerRequestListener`, `nativeClearSignerRequestListener`
//! * **capability.rs**: `nativeSetCapabilityHandler`
//! * **identity.rs**: `nativeIdentityRestore`
//! * **marmot.rs**: `nativeMarmotRegisterActive`, `nativeMarmotUnregister`
//! * **lib.rs**: `nativeSeedRelays`, `nativeSignInNsec`, `nativeSwitchAccount`,
//!   `nativeRemoveAccount`, `nativeCreateLocalAccount`, `nativeAddRelay`,
//!   `nativeRemoveRelay`, `nativeEncodeProfile`
//! * **platform.rs**: `nativeSetStoragePath`, `nativeLifecycleForeground`,
//!   `nativeLifecycleBackground`, `nativeIsAlive`
//! * **action.rs**: `nativeAckActionStage`, `nativeRetryPublish`,
//!   `nativeCancelPublish`, `nativeDispatchAction`
//! * **flat_feed.rs**: `nativeOpenHomeFeed`, `nativeOpenThread`,
//!   `nativeCloseThread`, `nativeOpenAuthor`, `nativeCloseAuthor`,
//!   `nativeCloseFeed`, `nativeLoadOlderFeed`
//! * **claims.rs**: `nativeClaimEvent`, `nativeReleaseEvent`,
//!   `nativeResolveRef`, `nativeReleaseRef`
//!
//! KernelBridge.kt passes `legacy_jni_session_id()` only to these lanes.
//!
//! ## D6 contract
//!
//! No method throws. Init failure yields an inert handle; `dispatch_*`
//! methods return `DispatchAck.error`, and `start`/`stop`/`close` are no-ops.
//!
//! ## D8 contract
//!
//! Update delivery is push-based via `UpdateSink.on_update`; no polling or
//! sleep-check loops exist here.

use std::ffi::CStr;
use std::sync::Arc;

use nmp_app_chirp::{
    dispatch_action_bytes_for, nmp_app_chirp_declare_consumed_projections, nmp_app_chirp_register,
    nmp_signer_broker_init, NmpRegisterStatus,
};
use nmp_ffi::{
    nmp_app_declare_incremental_apply, nmp_app_dispatch_action_bytes, nmp_app_free, nmp_app_new,
    nmp_app_start, nmp_app_stop, nmp_free_string, NmpConfigStatus,
};

use crate::external_signer;
use crate::session::{insert_session, remove_session, Session};

// ── Public binding types ──────────────────────────────────────────────────────

/// Typed dispatch acknowledgement — the public Kotlin shape for dispatch
/// results (D6: no thrown exceptions; errors surface through this record).
///
/// Exactly one of `correlation_id` / `error` is `Some` for any real dispatch.
#[derive(uniffi::Record)]
pub struct DispatchAck {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
}

/// Kotlin callback interface for kernel update frames (D8: push, no polling).
///
/// Rust invokes `on_update` from the kernel's update-listener thread.
/// Implementations must marshal to the Android main thread themselves for UI.
///
/// A panicking implementation is contained: panics are caught inside the
/// Rust trampoline and logged/dropped; they do NOT unwind through the C
/// callback boundary (undefined behaviour).
#[uniffi::export(callback_interface)]
pub trait UpdateSink: Send + Sync {
    fn on_update(&self, frame: Vec<u8>);
}

// ── AppHandle ─────────────────────────────────────────────────────────────────

/// UniFFI app-loop handle for Android (M14-0 / issue #2129).
///
/// One instance per process. Lifecycle:
///   `AppHandle.new()` → `start()` → dispatch/update loop → `stop()` →
///   `close()`.
///
/// Thread-safety: all methods are safe to call from any thread.
///
/// ## `legacy_jni_session_id()`
///
/// Returns the internal session registry id used by residual JNI lanes
/// (signer, capability, marmot, identity). It is NOT permission to use
/// this id for app-loop operations from new Kotlin code.
#[derive(uniffi::Object)]
pub struct AppHandle {
    pub(crate) session: Arc<Session>,
    /// Session registry id for residual JNI lanes.
    pub(crate) handle: i64,
}

#[uniffi::export]
impl AppHandle {
    /// Construct and initialise the Chirp app kernel.
    ///
    /// D6: on any init failure, returns an inert handle whose `dispatch_*`
    /// methods return `DispatchAck.error` and whose `start`/`stop`/`close`
    /// are no-ops.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        match init_app_handle() {
            Ok(h) => h,
            Err(reason) => {
                eprintln!("AppHandle::new() init failure: {reason}");
                Arc::new(AppHandle {
                    session: Session::inert_session(),
                    handle: 0,
                })
            }
        }
    }

    /// Start the kernel event loop.
    ///
    /// `visible_limit`: max items in the visible window (0 = kernel default).
    /// `emit_hz`: update emission rate in Hz (0 = kernel default, max 12).
    pub fn start(&self, visible_limit: u32, emit_hz: u32) {
        self.session.with_app(|app| nmp_app_start(app, visible_limit, emit_hz));
    }

    /// Stop the kernel event loop.
    pub fn stop(&self) {
        self.session.with_app(|app| nmp_app_stop(app));
    }

    /// Dispatch a pre-encoded FlatBuffers `DispatchEnvelope` byte buffer.
    ///
    /// `bytes` must be a valid NMPD `DispatchEnvelope` produced by a Chirp
    /// action builder (correlation_id + namespace + schema_version + typed
    /// payload). Returns `DispatchAck.correlation_id` on acceptance or
    /// `DispatchAck.error` on rejection; never throws (D6).
    ///
    /// This is the primary byte-accurate dispatch method. Kotlin callers that
    /// already produce encoded action envelopes should use this over the
    /// legacy JSON adapters below.
    pub fn dispatch_action_bytes(&self, bytes: Vec<u8>) -> DispatchAck {
        let result_ptr = self.session.with_app(|app| {
            nmp_app_dispatch_action_bytes(app, bytes.as_ptr(), bytes.len())
        });
        match result_ptr {
            None => DispatchAck {
                correlation_id: None,
                error: Some("inert or closed handle — dispatch rejected".to_string()),
            },
            Some(ptr) if ptr.is_null() => DispatchAck {
                correlation_id: None,
                error: Some("dispatch returned null (kernel internal error)".to_string()),
            },
            Some(ptr) => {
                // SAFETY: nmp_app_dispatch_action_bytes returns a heap-allocated
                // NUL-terminated C string.  nmp_free_string is the canonical free.
                let json = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
                nmp_free_string(ptr);
                parse_dispatch_ack(&json)
            }
        }
    }

    /// Dispatch from a `(namespace, body_json)` pair.
    ///
    /// JSON adapter for namespaces that pre-date the FlatBuffers write boundary:
    /// kept as a RESIDUAL for the Marmot hybrid builder path (#2169) and the
    /// terminal-UI (TUI) consumer. The intent/action-spec path is GONE (M14-1 /
    /// #2145); all Chirp social write verbs use `dispatch_action_bytes` with
    /// generated builders. Routes through the same typed byte doorway
    /// (`nmp_app_dispatch_action_bytes`) as `dispatch_action_bytes`.
    /// Never throws (D6).
    pub fn dispatch_action_json(&self, namespace: String, body_json: String) -> DispatchAck {
        let result = self
            .session
            .with_app(|app| dispatch_action_bytes_for(app, &namespace, &body_json));
        dispatch_result_to_ack(result)
    }

    /// Register a UniFFI update sink for kernel frames (D8: push, no polling).
    ///
    /// Replaces any previously registered sink atomically. The old sink will
    /// not receive any frames after this call returns (but may still be
    /// running an in-flight `on_update` until the next `clear_update_sink`
    /// quiescence).
    ///
    /// Panics inside the Kotlin callback are contained (caught in `catch_unwind`
    /// by the trampoline, logged/dropped, never abort the process).
    ///
    /// Kotlin must marshal frames to the main thread itself; `on_update` is
    /// called from a Rust background thread.
    pub fn set_update_sink(&self, sink: Box<dyn UpdateSink>) {
        // Convert Box<dyn UpdateSink> → Arc<dyn UpdateSink> for the snapshot
        // pattern (lock → clone → release → invoke, preventing deadlock if the
        // callback calls back into AppHandle dispatch methods).
        let sink_arc: Arc<dyn UpdateSink> = Arc::from(sink);
        let update_fn: Arc<dyn Fn(Vec<u8>) + Send + Sync> = Arc::new(move |bytes: Vec<u8>| {
            let s = sink_arc.clone();
            // catch_unwind: a panicking Kotlin callback must not unwind through
            // the C callback boundary (undefined behaviour in nmp-ffi).
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                s.on_update(bytes);
            }));
        });
        self.session.set_generic_sink(update_fn);
    }

    /// Deregister the update sink.
    ///
    /// Quiescent: after this returns, the old sink is guaranteed to have
    /// received its last `on_update` call (any in-flight invocation completes
    /// before this returns). The kernel update callback is re-registered so
    /// `set_update_sink` remains callable afterwards.
    ///
    /// Uses a lock-free quiescence sequence (extracts app pointer under a
    /// brief lock, releases the lock, then waits on the kernel Condvar) to
    /// avoid deadlock when Kotlin's on_update callback calls back into
    /// dispatch methods on another thread.
    pub fn clear_update_sink(&self) {
        // Step 1: Quiesce WITHOUT holding Session.state (avoids deadlock when
        // the in-flight on_update re-enters AppHandle via dispatch).
        self.session.quiesce_update_callback_lockfree();
        // Step 2: Clear the generic sink slot (safe: no in-flight on_update).
        self.session.clear_generic_sink();
        // Step 3: Re-register the C callback so future set_update_sink calls
        // deliver updates correctly.
        self.session.reregister_update_callback_lockfree();
    }

    /// Shut down the kernel session and release all native resources.
    ///
    /// Quiescent: waits for any in-flight `on_update` to complete before
    /// clearing sinks and removing the session from the registry. After this
    /// returns, no callbacks will fire. Idempotent.
    ///
    /// KernelBridge.kt must call this instead of the deleted `nativeFree`.
    /// Named `shutdown` (not `close`) to avoid colliding with the UniFFI-generated
    /// `AutoCloseable.close()` override on the Kotlin side.
    pub fn shutdown(&self) {
        // close_updates quiesces update + capability callbacks and clears sinks.
        self.session.close_updates();
        // Remove from registry so residual JNI lanes cannot obtain new Arc
        // clones after close.
        remove_session(self.handle);
        // free_native is idempotent: checks freed/updates_closed and returns
        // early if close_updates already ran.
        self.session.free_native();
    }

    /// Session registry id for residual JNI lanes.
    ///
    /// Returns the `jlong` handle that signer, capability, marmot, and
    /// identity JNI functions use to look up the kernel session. NOT for
    /// app-loop operations.
    pub fn legacy_jni_session_id(&self) -> i64 {
        self.handle
    }
}

// ── Init helper ───────────────────────────────────────────────────────────────

fn init_app_handle() -> Result<Arc<AppHandle>, String> {
    let app = nmp_app_new();
    if app.is_null() {
        return Err("nmp_app_new returned null".to_string());
    }

    let broker_rc = nmp_signer_broker_init(app);
    if broker_rc != NmpConfigStatus::Ok as u32 {
        eprintln!("nmp_signer_broker_init failed: rc={broker_rc}");
        nmp_app_free(app);
        return Err(format!("nmp_signer_broker_init failed rc={broker_rc}"));
    }

    // ADR-0053: declare Chirp's projection-consumption intent (must precede start).
    nmp_app_chirp_declare_consumed_projections(app);

    // ADR-0055 R3-S4: declare incremental-apply contract (must precede start).
    let rc = nmp_app_declare_incremental_apply(app);
    if rc != 0 {
        eprintln!("nmp_app_declare_incremental_apply failed: rc={rc}");
        nmp_app_free(app);
        return Err(format!("nmp_app_declare_incremental_apply failed rc={rc}"));
    }

    // Register Chirp (null viewer — user signs in later).
    // FIX 3 (D6): runtime check instead of debug_assert_eq. A failed
    // registration returns an inert handle rather than panicking in debug
    // builds or proceeding with a non-inert handle in release builds.
    let mut chirp = std::ptr::null_mut();
    let reg_status = nmp_app_chirp_register(app, std::ptr::null(), &mut chirp);
    if reg_status != NmpRegisterStatus::Ok as u32 {
        eprintln!("nmp_app_chirp_register failed: rc={reg_status}");
        nmp_app_free(app);
        return Err(format!("nmp_app_chirp_register failed rc={reg_status}"));
    }

    // Session::new registers the kernel update callback (via nmp_app_set_update_callback).
    let session = Arc::new(Session::new(app, chirp));

    let handle = insert_session(Arc::clone(&session));
    if handle == 0 {
        // insert_session freed app via Arc::clone(session).free_native().
        return Err("failed to register session in registry (mutex poisoned)".to_string());
    }

    // ADR-0048 Stage 2: install external-signer capability trampoline.
    external_signer::install(app, handle);

    Ok(Arc::new(AppHandle { session, handle }))
}

// ── Dispatch helpers ──────────────────────────────────────────────────────────

pub(crate) fn parse_dispatch_ack(json: &str) -> DispatchAck {
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(json) {
        DispatchAck {
            correlation_id: obj
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            error: obj
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    } else {
        DispatchAck {
            correlation_id: None,
            error: Some(format!("malformed dispatch result (not JSON): {json}")),
        }
    }
}

fn dispatch_result_to_ack(result: Option<Result<String, String>>) -> DispatchAck {
    match result {
        None => DispatchAck {
            correlation_id: None,
            error: Some("inert or closed handle — dispatch rejected".to_string()),
        },
        Some(Ok(cid)) => DispatchAck {
            correlation_id: Some(cid),
            error: None,
        },
        Some(Err(e)) => DispatchAck {
            correlation_id: None,
            error: Some(e),
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "uniffi_app_loop/tests.rs"]
mod tests;
