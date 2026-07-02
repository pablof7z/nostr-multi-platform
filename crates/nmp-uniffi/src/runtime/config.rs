//! Storage path and projection config UniFFI methods — M14-C6.
//!
//! Owns the pre-start configuration UniFFI methods after the migrated C-ABI
//! symbols were deleted.
//!
//! ## Design notes
//!
//! * All four methods are init-only: call before `start()`. The runtime
//!   enforces this and returns `Err(NmpError::AlreadyStarted)` for the two
//!   fallible methods when called after start.
//! * `declare_consumed_projections` and `consume_all_builtin_projections` are
//!   fire-and-forget (infallible from the caller's perspective — the runtime
//!   silently ignores null/empty arrays per D6).
//! * `NmpError::RegistryUnavailable` maps from `NmpConfigStatus::Unavailable`
//!   and `IncrementalApplyError::RegistryUnavailable` (mutex poisoned).

use nmp_core::substrate::IncrementalApplyError;
use nmp_native_runtime::NmpConfigStatus;

use crate::{NmpApp, NmpError};

#[uniffi::export]
impl NmpApp {
    /// Set the persistent storage directory for the LMDB `EventStore` backend.
    ///
    /// Call **before** `start()`. Passing `None`, an empty string, or
    /// whitespace-only string clears any previously set path and lets the
    /// kernel fall back to its default.
    ///
    /// Returns `Err(NmpError::AlreadyStarted)` when called after `start()`
    /// (the existing path is left untouched; the composition ledger records
    /// `DroppedLateWiring`).
    pub fn set_storage_path(&self, path: Option<String>) -> Result<(), NmpError> {
        match self.inner.set_storage_path(path) {
            NmpConfigStatus::Ok => Ok(()),
            NmpConfigStatus::AlreadyStarted => Err(NmpError::AlreadyStarted),
            // NullApp cannot happen — self is always a valid Arc<NmpApp>.
            NmpConfigStatus::NullApp => Ok(()),
            NmpConfigStatus::Unavailable => Err(NmpError::RegistryUnavailable),
        }
    }

    /// ADR-0070 Rung 3 — declare that this host runtime owns the NMP
    /// cache-merge layer and is ready to receive frames with `Unchanged`
    /// projections omitted.
    ///
    /// Must be called **before** `start()`. After this call the kernel
    /// guarantees the next frame is a full baseline (all live Tier-2
    /// projections emitted as `Changed`). Until this is called the kernel
    /// emits full rows on every tick. Idempotent — subsequent pre-start
    /// calls return `Ok(())` without re-setting the latch.
    ///
    /// Returns `Err(NmpError::AlreadyStarted)` if called after `start()`, or
    /// `Err(NmpError::RegistryUnavailable)` if the snapshot-registry mutex is
    /// poisoned.
    pub fn declare_incremental_apply(&self) -> Result<(), NmpError> {
        match self.inner.declare_incremental_apply() {
            Ok(()) => Ok(()),
            Err(IncrementalApplyError::AlreadyStarted) => Err(NmpError::AlreadyStarted),
            Err(IncrementalApplyError::RegistryUnavailable) => Err(NmpError::RegistryUnavailable),
        }
    }

    /// ADR-0070 — declare the static set of Tier-2 built-in projection keys
    /// this host consumes (the output-side sibling of relay interest installs).
    ///
    /// `keys` is the union of every projection key any of the app's screens
    /// reads, known at build time. The kernel then serializes a built-in into
    /// each snapshot only if its key is in the declared set. An empty
    /// declaration leaves the kernel emitting every built-in (no narrowing);
    /// a non-empty declaration narrows to the declared members.
    ///
    /// Additive — multiple calls union. Intended as a host-init call before
    /// `start()`. D6: empty/blank keys are silently skipped.
    pub fn declare_consumed_projections(&self, keys: Vec<String>) {
        self.inner.declare_consumed_projections(keys);
    }

    /// ADR-0070 / Workstream-E4 — declare intent to consume EVERY Tier-2
    /// built-in projection (`DeclaredProjections::All`).
    ///
    /// Use this instead of leaving consumption intent undeclared when the
    /// app genuinely reads the full built-in set. Idempotent; call before
    /// `start()`. D6: safe as a no-op when the kernel slot is unavailable.
    pub fn consume_all_builtin_projections(&self) {
        self.inner.consume_all_builtin_projections();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::{NmpApp, NmpError};

    // ── set_storage_path ──────────────────────────────────────────────────

    /// Parity with C-ABI test `storage_path_can_be_set_after_prestart_command`:
    /// `set_storage_path` before `start()` returns `Ok(())`.
    #[test]
    fn parity_set_storage_path_before_start_ok() {
        let app = NmpApp::new();
        let path = std::env::temp_dir().join(format!("nmp-uniffi-storage-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        let result = app.set_storage_path(Some(path.to_string_lossy().to_string()));
        assert!(result.is_ok(), "set_storage_path before start must be Ok");
        let _ = std::fs::remove_dir_all(path);
    }

    /// Parity with C-ABI test `storage_path_after_start_is_rejected_and_recorded`:
    /// `set_storage_path` AFTER `start()` returns `Err(AlreadyStarted)`.
    #[test]
    fn parity_set_storage_path_after_start_already_started() {
        let app = NmpApp::new();
        app.start(256, 4);
        let result = app.set_storage_path(Some("/tmp/nmp-uniffi-late".to_string()));
        assert!(
            matches!(result, Err(NmpError::AlreadyStarted)),
            "set_storage_path after start must return AlreadyStarted, got {result:?}",
        );
        app.shutdown();
    }

    /// `set_storage_path(None)` before start is a clear-path no-op — Ok.
    #[test]
    fn parity_set_storage_path_none_ok() {
        let app = NmpApp::new();
        assert!(app.set_storage_path(None).is_ok());
    }

    // ── declare_incremental_apply ─────────────────────────────────────────

    /// Parity with C-ABI return-code `0 = ok`: `declare_incremental_apply`
    /// before `start()` returns `Ok(())`.
    #[test]
    fn parity_declare_incremental_apply_before_start_ok() {
        let app = NmpApp::new();
        let result = app.declare_incremental_apply();
        assert!(
            result.is_ok(),
            "declare_incremental_apply before start must be Ok"
        );
    }

    /// Parity with C-ABI return-code `1 = AlreadyStarted`:
    /// `declare_incremental_apply` AFTER `start()` returns
    /// `Err(AlreadyStarted)`.
    #[test]
    fn parity_declare_incremental_apply_after_start_already_started() {
        let app = NmpApp::new();
        app.start(256, 4);
        let result = app.declare_incremental_apply();
        assert!(
            matches!(result, Err(NmpError::AlreadyStarted)),
            "declare_incremental_apply after start must return AlreadyStarted, got {result:?}",
        );
        app.shutdown();
    }

    /// Idempotent: calling `declare_incremental_apply` twice before start
    /// returns `Ok(())` both times.
    #[test]
    fn parity_declare_incremental_apply_idempotent() {
        let app = NmpApp::new();
        assert!(app.declare_incremental_apply().is_ok());
        assert!(
            app.declare_incremental_apply().is_ok(),
            "second pre-start call must be idempotent Ok"
        );
    }

    // ── declare_consumed_projections ──────────────────────────────────────

    /// Fire-and-forget: calling with a non-empty key list before start
    /// must not panic.
    #[test]
    fn parity_declare_consumed_projections_no_panic() {
        let app = NmpApp::new();
        app.declare_consumed_projections(vec![
            "accounts".to_string(),
            "relay_diagnostics".to_string(),
        ]);
    }

    /// Calling with an empty list is a no-op (D6).
    #[test]
    fn parity_declare_consumed_projections_empty_no_panic() {
        let app = NmpApp::new();
        app.declare_consumed_projections(vec![]);
    }

    /// Additive: multiple calls union — neither panics nor resets the set.
    #[test]
    fn parity_declare_consumed_projections_additive_no_panic() {
        let app = NmpApp::new();
        app.declare_consumed_projections(vec!["accounts".to_string()]);
        app.declare_consumed_projections(vec!["relay_diagnostics".to_string()]);
    }

    // ── consume_all_builtin_projections ───────────────────────────────────

    /// Fire-and-forget: calling before start must not panic.
    #[test]
    fn parity_consume_all_builtin_projections_no_panic() {
        let app = NmpApp::new();
        app.consume_all_builtin_projections();
    }

    /// Idempotent: calling multiple times must not panic.
    #[test]
    fn parity_consume_all_builtin_projections_idempotent() {
        let app = NmpApp::new();
        app.consume_all_builtin_projections();
        app.consume_all_builtin_projections();
    }
}
