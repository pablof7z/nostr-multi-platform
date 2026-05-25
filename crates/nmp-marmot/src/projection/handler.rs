//! `MarmotMlsOpHandler` — the [`nmp_core::substrate::HostOpHandler`] impl that
//! routes [`MarmotAction`](super::action::MarmotAction) JSON envelopes
//! through the live [`MarmotProjection`](super::state::MarmotProjection)
//! and the existing [`super::ops::dispatch`] handlers.
//!
//! # The bridge between the kernel's generic seam and Marmot's typed ops
//!
//! `nmp-core` defines [`HostOpHandler::handle`](nmp_core::substrate::HostOpHandler::handle)
//! as `(&str, &str) -> serde_json::Value` — exactly the JSON-in / JSON-out
//! shape the legacy bespoke `nmp_marmot_dispatch` envelope spoke (deleted
//! in ADR-0025 PR 3, 2026-05-23), with `correlation_id` added so the
//! actor's `DispatchHostOp` arm can record the terminal verdict in the
//! kernel's `action_stages` mirror.
//!
//! This handler:
//!
//! 1. parses `action_json` back into the typed [`MarmotAction`] enum (it
//!    was just serialized by [`super::action::MarmotActionModule::execute`];
//!    `serde_json::from_str` cannot fail here in practice, but a D6 soft-
//!    fail envelope is returned for the should-be-impossible case);
//! 2. re-serializes the typed enum to the legacy `{"op": "...", ...}` JSON
//!    shape the existing [`super::ops::dispatch`] handlers consume; and
//! 3. invokes [`super::ops::dispatch`] under the projection's `Mutex<Inner>`
//!    via [`MarmotProjection::with_inner`].
//!
//! The result `serde_json::Value` is returned verbatim — the actor's arm
//! interprets `{"ok":true,...}` vs `{"ok":false,...}` and routes to the
//! appropriate `record_action_*` kernel API.
//!
//! # Why re-serialize when we already have JSON?
//!
//! The action body is parsed twice (once by the registry's adapter into
//! `MarmotAction`, once here back into the legacy envelope shape) to keep
//! the parsing layer EXACTLY symmetric: the typed enum is the validation
//! gate; the legacy envelope is what `ops::dispatch` consumes. Removing the
//! round-trip would require either (a) bypassing the typed validation gate
//! (losing the D6 shape rejection) or (b) rewriting `ops::dispatch` to take
//! the typed enum (touching every op handler — out of scope for PR 1).
//!
//! # Threading
//!
//! `HostOpHandler::handle` runs INLINE on the actor thread (the
//! `DispatchHostOp` dispatch arm). The handler acquires the projection's
//! `Mutex<Inner>` via `with_inner`. After ADR-0025 PR 3 (2026-05-23,
//! deleted the legacy bespoke `nmp_marmot_dispatch` C-ABI symbol), the
//! actor thread is the sole HOST writer (D4) — the only other caller of
//! that mutex is the in-process Rust-native [`crate::ffi::MarmotHandle::dispatch`]
//! accessor (REPL / TUI / integration tests), which runs from the
//! caller's own thread. In production (Chirp iOS) only the actor thread
//! writes; the `MarmotHandle::dispatch` path is not on the production
//! hot path.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::substrate::HostOpHandler;

use crate::projection::action::MarmotAction;
use crate::projection::ops;
use crate::projection::state::MarmotProjection;

/// `HostOpHandler` impl that delegates to a shared [`MarmotProjection`].
///
/// Holds an `Arc<MarmotProjection>` — the same `Arc` the FFI register path
/// installs into the [`crate::ffi::MarmotHandle`]. The register-with-keys
/// wiring clones it once into the handler and once into the handle, so
/// the projection outlives BOTH the host substrate-generic dispatch path
/// (kernel actor → this handler) and the in-process Rust-native accessor
/// path ([`crate::ffi::MarmotHandle::dispatch`]) — handler dropped → `Arc`
/// count -1; handle dropped → `Arc` count -1; only the final drop frees
/// `MarmotProjection`.
pub struct MarmotMlsOpHandler {
    projection: Arc<MarmotProjection>,
}

impl MarmotMlsOpHandler {
    /// Build the handler around the shared projection. Called from the
    /// FFI register path immediately after [`MarmotProjection::new`].
    #[must_use]
    pub fn new(projection: Arc<MarmotProjection>) -> Self {
        Self { projection }
    }
}

impl HostOpHandler for MarmotMlsOpHandler {
    fn handle(&self, action_json: &str, _correlation_id: &str) -> serde_json::Value {
        // (1) Parse the action JSON into the typed enum. The registry's
        // adapter already did this once before `execute` ran (which is
        // what produced the JSON we're parsing now), so `from_str` cannot
        // fail in any reachable production path — but D6 keeps the soft-
        // fail branch instead of `unwrap`.
        let typed: MarmotAction = match serde_json::from_str(action_json) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::json!({
                    "ok": false,
                    "error": format!("MarmotMlsOpHandler: action_json did not parse to MarmotAction: {e}"),
                });
            }
        };

        // (2) Re-serialize the typed enum to the legacy `{"op": "...", ...}`
        // envelope shape `ops::dispatch` consumes. Same D6 contract.
        let legacy_envelope = match serde_json::to_value(&typed) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::json!({
                    "ok": false,
                    "error": format!("MarmotMlsOpHandler: failed to re-serialize MarmotAction: {e}"),
                });
            }
        };

        // (3) Invoke the existing dispatch entry point under the
        // projection's mutex. `with_inner` returns `None` if the mutex is
        // poisoned — D6 surface as a soft-fail envelope (matches the
        // soft-fail envelope the legacy bespoke `nmp_marmot_dispatch`
        // symbol returned pre-PR-3, and the envelope
        // `MarmotHandle::dispatch` returns today).
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.projection
            .with_inner(|h| ops::dispatch(h, &legacy_envelope, now_secs))
            .unwrap_or_else(|| serde_json::json!({
                "ok": false,
                "error": "MarmotMlsOpHandler: projection mutex poisoned",
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handler is `Send + Sync` — required for storage in
    /// `nmp_core::substrate::HostOpHandlerSlot` (which holds
    /// `Arc<dyn HostOpHandler: Send + Sync>`).
    #[test]
    fn handler_satisfies_send_and_sync_trait_bounds() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MarmotMlsOpHandler>();
    }

    /// A handler whose `action_json` does not parse to `MarmotAction`
    /// returns a soft-fail envelope (NOT a panic). The reachable production
    /// path can't trigger this (the registry adapter parsed the same JSON
    /// before producing it), but the D6 soft-fail branch must work for the
    /// should-be-impossible case.
    ///
    /// We do NOT exercise the happy path here — that requires a real
    /// `MarmotProjection` with an MDK SQLite DB, which is the territory
    /// of the integration tests in `src/tests.rs`. This unit test only
    /// proves the boundary-defensive layer.
    ///
    /// Construction mirrors the pattern in `src/tests.rs` /
    /// `src/ffi/tests.rs`: an in-memory `MdkSqliteStorage` (gated on
    /// `mdk-sqlite-storage`'s `test-utils` feature, which is in this
    /// crate's `dev-dependencies`) wrapped via `MarmotService::from_storage`.
    #[test]
    fn malformed_action_json_returns_soft_fail_envelope() {
        use crate::service::MarmotService;
        use mdk_core::MdkConfig;
        use mdk_sqlite_storage::MdkSqliteStorage;
        use nostr::Keys;

        let storage =
            MdkSqliteStorage::new_in_memory().expect("in-memory MDK storage should construct");
        let service = MarmotService::from_storage(storage, Keys::generate(), MdkConfig::default());
        let projection = Arc::new(MarmotProjection::new(service));
        let handler = MarmotMlsOpHandler::new(projection);

        let result = handler.handle("not valid json{}", "corr-id");
        assert_eq!(
            result.get("ok").and_then(|v| v.as_bool()),
            Some(false),
            "malformed action_json must return a soft-fail envelope, got: {result}"
        );
        let err = result
            .get("error")
            .and_then(|v| v.as_str())
            .expect("soft-fail envelope must carry an error string");
        assert!(
            err.contains("did not parse to MarmotAction"),
            "error should name the parse failure, got: {err}"
        );
    }
}
