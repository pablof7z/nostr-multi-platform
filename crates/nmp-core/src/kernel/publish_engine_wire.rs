//! Wire-frame ↔ engine mapping helpers used by `kernel::publish_engine`.
//!
//! Two narrow concerns live here so the main `publish_engine` file stays
//! within the AGENTS.md soft cap:
//!
//! - [`split_ok_message`] — parse a NIP-20 `OK-false` reason like
//!   `"blocked: spam"` into `(code, message)`. The engine's classifier
//!   (`crate::publish::state::classify_ack`) keys retry policy off `code`;
//!   keeping the parser here means the engine itself never sees the wire
//!   string (D7 — dispatchers / kernel are the only path that touch raw
//!   wire shapes; the engine takes pre-classified `RelayAck` values).
//! - [`describe_engine_error`] — map a `PublishEngineError` to the kernel
//!   pair `(toast_string, queue_entry_status)`. D6: errors are state
//!   (toast + queue row), never exceptions across FFI.

use crate::kernel::closed_reason::{ERR_PERMANENT, ERR_TRANSIENT};
use crate::publish::PublishEngineError;

/// Split a NIP-20 `OK-false` reason into a `(code, message)` pair.
///
/// NIP-20 specs the reason as `<prefix>: <message>` for retryable /
/// permanent classes (`"blocked"`, `"pow"`, `"rate-limited"`,
/// `"auth-required"`, …). Reasons without a colon become `("error", msg)`
/// — the engine's classifier treats the unknown `"error"` code as
/// `Transient` (conservative retry), matching the existing M7 behaviour.
pub(super) fn split_ok_message(msg: &str) -> (String, String) {
    if let Some((prefix, rest)) = msg.split_once(':') {
        let code = prefix.trim().to_ascii_lowercase();
        if code.is_empty() {
            return ("error".to_string(), msg.to_string());
        }
        return (code, rest.trim().to_string());
    }
    if msg.is_empty() {
        ("error".to_string(), String::new())
    } else {
        ("error".to_string(), msg.to_string())
    }
}

/// Map a `PublishEngineError` into the kernel's projection triple:
/// `(toast_string, queue_entry_status, error_category)`. D6: every engine
/// error has a snapshot-visible counterpart; no `Result<T, E>` ever crosses
/// FFI. The `error_category` is one of the typed FFI contract keys
/// (`kernel::closed_reason::ERR_*`) so iOS branches on error class without
/// parsing the English toast.
///
/// Category rationale:
/// - `NoTargets` → `permanent` — retrying the same publish will not help
///   until the user declares a write-relay (a config change, not a retry).
/// - `DuplicateHandle` → `transient` — the same publish is already in
///   flight; the in-flight attempt will settle on its own.
/// - `Store` → `permanent` — a durable-store backend failure will not
///   resolve by re-issuing the publish.
/// - `UnsupportedAction` → `permanent` — a wiring bug (the engine was handed
///   an action it does not service); retrying cannot fix a code-level miswire.
pub(super) fn describe_engine_error(err: &PublishEngineError) -> (String, String, &'static str) {
    match err {
        PublishEngineError::NoTargets => (
            "active account has no write-relays declared — add a relay in \
             Accounts → Relays and publish a fresh kind:10002"
                .to_string(),
            "pending_relays_unknown".to_string(),
            ERR_PERMANENT,
        ),
        PublishEngineError::DuplicateHandle(handle) => (
            format!("publish already in flight: {handle}"),
            "duplicate".to_string(),
            ERR_TRANSIENT,
        ),
        PublishEngineError::Store(store_err) => (
            format!("publish store error: {store_err:?}"),
            "store_error".to_string(),
            ERR_PERMANENT,
        ),
        PublishEngineError::UnsupportedAction(detail) => (
            format!("publish engine received an unsupported action: {detail}"),
            "unsupported_action".to_string(),
            ERR_PERMANENT,
        ),
    }
}

/// Wall-clock epoch milliseconds. The engine accepts any monotonic clock
/// source as `now_ms` — production uses `SystemTime::now()`; tests inject
/// `now_ms` directly via `*_at` variants on the `Kernel` engine surface.
pub(super) fn now_epoch_ms() -> u64 {
    use crate::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{PublishEngineError, PublishStoreError};

    #[test]
    fn split_ok_message_parses_nip20_prefix() {
        assert_eq!(
            split_ok_message("blocked: spam"),
            ("blocked".to_string(), "spam".to_string())
        );
        assert_eq!(
            split_ok_message("auth-required: please AUTH"),
            ("auth-required".to_string(), "please AUTH".to_string())
        );
        assert_eq!(split_ok_message(""), ("error".to_string(), String::new()));
        assert_eq!(
            split_ok_message("just a notice"),
            ("error".to_string(), "just a notice".to_string())
        );
    }

    #[test]
    fn describe_engine_error_covers_every_variant() {
        let (toast_nt, status_nt, cat_nt) = describe_engine_error(&PublishEngineError::NoTargets);
        assert!(toast_nt.contains("write-relays"));
        assert_eq!(status_nt, "pending_relays_unknown");
        assert_eq!(cat_nt, ERR_PERMANENT);

        let (toast_dup, status_dup, cat_dup) =
            describe_engine_error(&PublishEngineError::DuplicateHandle("h".to_string()));
        assert!(toast_dup.contains("already in flight"));
        assert_eq!(status_dup, "duplicate");
        assert_eq!(cat_dup, ERR_TRANSIENT);

        let (toast_store, status_store, cat_store) = describe_engine_error(
            &PublishEngineError::Store(PublishStoreError::Backend("oom".into())),
        );
        assert!(toast_store.contains("store error"));
        assert_eq!(status_store, "store_error");
        assert_eq!(cat_store, ERR_PERMANENT);

        let (toast_unsupported, status_unsupported, cat_unsupported) =
            describe_engine_error(&PublishEngineError::UnsupportedAction("PublishProfile"));
        assert!(toast_unsupported.contains("unsupported action"));
        assert_eq!(status_unsupported, "unsupported_action");
        assert_eq!(cat_unsupported, ERR_PERMANENT);
    }
}
