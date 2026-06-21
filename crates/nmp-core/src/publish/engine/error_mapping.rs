//! D6 FFI mapping for `PublishEngineError`.
//!
//! Per D6 ("errors never cross FFI as exceptions") every operational failure
//! must reach the platform as a state field on the `PublishStatusSnapshot` —
//! never as a thrown exception or `Result<T, E>` across the boundary.
//!
//! The engine returns `Result<(), PublishEngineError>` from `start_publish`
//! / `cancel_publish` / `resume_from_store` so the in-process caller (the
//! kernel actor) can decide what to do next. Before crossing FFI, the
//! actor / FFI adapter calls [`engine_error_to_failure`] and pushes the
//! resulting `RecentFailure` onto the view via
//! [`PublishStatusState::push_failure`]. That makes the error observable
//! to the platform exactly the way `RelayAck::Failed` chains do, with no
//! special-case error code path.
//!
//! This module is a pure function plus a thin wrapper on `PublishEngine`
//! — no I/O, no allocations beyond the produced `RecentFailure`.

use super::super::action::PublishHandle;
use super::super::view::RecentFailure;
use super::PublishEngineError;

/// Pseudo relay-url used for engine-level failures that don't belong to a
/// real relay (no targets resolved, duplicate handle, store backend error).
/// Kept stable so platform UIs can filter/group on it if needed.
pub const ENGINE_FAILURE_RELAY_URL: &str = "(engine)";

/// Map a `PublishEngineError` into a `RecentFailure` row the snapshot can
/// carry across FFI. Pure function; unit-testable in isolation.
///
/// `event_id` may be empty when the error happens before an event is
/// associated with a handle (e.g. duplicate handle on a fresh start) — the
/// snapshot consumer is expected to tolerate that.
#[must_use]
pub fn engine_error_to_failure(
    err: &PublishEngineError,
    handle: &PublishHandle,
    event_id: &str,
    now_ms: u64,
) -> RecentFailure {
    let reason = match err {
        PublishEngineError::DuplicateHandle(h) => {
            format!("duplicate publish handle: {h}")
        }
        PublishEngineError::NoTargets => "no relays resolved for publish target".to_string(),
        PublishEngineError::Store(store_err) => {
            format!("publish store backend failure: {store_err:?}")
        }
        PublishEngineError::UnsupportedAction(detail) => {
            format!("publish engine received an unsupported action: {detail}")
        }
    };
    RecentFailure {
        handle: handle.clone(),
        event_id: event_id.to_string(),
        relay_url: ENGINE_FAILURE_RELAY_URL.to_string(),
        reason,
        at_ms: now_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::traits::PublishStoreError;
    use super::*;

    #[test]
    fn duplicate_handle_maps_to_descriptive_recent_failure() {
        let err = PublishEngineError::DuplicateHandle("p-dup".to_string());
        let failure = engine_error_to_failure(&err, &"p-dup".to_string(), "ev-1", 1_234);
        assert_eq!(failure.handle, "p-dup");
        assert_eq!(failure.event_id, "ev-1");
        assert_eq!(failure.relay_url, ENGINE_FAILURE_RELAY_URL);
        assert_eq!(failure.at_ms, 1_234);
        assert!(
            failure.reason.contains("duplicate"),
            "reason must call out duplicate: {}",
            failure.reason
        );
        assert!(
            failure.reason.contains("p-dup"),
            "reason must include the handle: {}",
            failure.reason
        );
    }

    #[test]
    fn no_targets_maps_to_recent_failure_with_stable_reason() {
        let err = PublishEngineError::NoTargets;
        let failure = engine_error_to_failure(&err, &"p-empty".to_string(), "ev-empty", 9_000);
        // Match the existing `emit_no_targets` reason string verbatim so
        // platform UIs that snapshot/test on that text stay stable whether
        // the failure was emitted by `start_publish_inner`'s direct path
        // or by an FFI adapter calling this helper.
        assert_eq!(failure.reason, "no relays resolved for publish target");
        assert_eq!(failure.relay_url, ENGINE_FAILURE_RELAY_URL);
    }

    #[test]
    fn store_backend_error_maps_to_recent_failure() {
        let err = PublishEngineError::Store(PublishStoreError::Backend("lmdb died".to_string()));
        let failure = engine_error_to_failure(&err, &"p-store".to_string(), "ev-store", 42);
        assert_eq!(failure.handle, "p-store");
        assert!(
            failure.reason.contains("publish store"),
            "reason must mention publish store: {}",
            failure.reason
        );
        assert!(
            failure.reason.contains("lmdb died"),
            "reason must carry the backend message: {}",
            failure.reason
        );
    }

    #[test]
    fn store_not_found_maps_to_recent_failure() {
        let err = PublishEngineError::Store(PublishStoreError::NotFound);
        let failure = engine_error_to_failure(&err, &"p-nf".to_string(), "", 0);
        assert!(
            failure.reason.contains("NotFound"),
            "reason must include the store variant: {}",
            failure.reason
        );
    }

    #[test]
    fn unsupported_action_maps_to_recent_failure() {
        // A `PublishProfile` reaching the engine is a wiring bug; D6 requires
        // it surface as snapshot-visible state, never a panic.
        let err = PublishEngineError::UnsupportedAction("PublishProfile");
        let failure = engine_error_to_failure(&err, &"p-bad".to_string(), "ev-bad", 7);
        assert_eq!(failure.relay_url, ENGINE_FAILURE_RELAY_URL);
        assert!(
            failure.reason.contains("unsupported action"),
            "reason must call out the unsupported action: {}",
            failure.reason
        );
        assert!(
            failure.reason.contains("PublishProfile"),
            "reason must carry the detail: {}",
            failure.reason
        );
    }
}
