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
/// NIP-20 specs the reason as `<prefix>: <message>` for its standardized
/// prefixes (`"blocked"`, `"pow"`, `"rate-limited"`, `"auth-required"`, …).
/// This split is policy-neutral — it only extracts the prefix; the engine's
/// `classify_ack` decides which prefixes are permanent vs. retryable (e.g.
/// `rate-limited` is retryable, `blocked` is permanent). Reasons without a
/// colon become `("error", msg)` — the classifier treats the unknown
/// `"error"` code as `Transient` (conservative retry), matching the existing
/// M7 behaviour.
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
/// `resolver_composed` is [`PublishEngine::resolver_composed`] — whether a
/// real `OutboxResolver` was ever installed via `set_outbox` (production
/// composition, or the test kernel's own auto-install). It ONLY changes the
/// `NoTargets` toast (#2937): every other variant's message is unconditional.
///
/// Category rationale:
/// - `NoTargets` → `permanent` — retrying the same publish will not help
///   until either the missing composition step runs (uncomposed case) or the
///   user declares a write-relay (composed-but-empty case) — neither is a
///   retry, both are a config/composition change.
/// - `DuplicateHandle` → `transient` — the same publish is already in
///   flight; the in-flight attempt will settle on its own.
/// - `Store` → `permanent` — a durable-store backend failure will not
///   resolve by re-issuing the publish.
/// - `UnsupportedAction` → `permanent` — a wiring bug (the engine was handed
///   an action it does not service); retrying cannot fix a code-level miswire.
pub(super) fn describe_engine_error(
    err: &PublishEngineError,
    resolver_composed: bool,
) -> (String, String, &'static str) {
    match err {
        // #2937: under the kernel's fail-closed `NoopOutboxResolver` default
        // (no `Kernel::set_publish_resolver` / `NmpApp::set_publish_resolver_factory`
        // ever ran), the write-relays advice below is actively wrong —
        // `NoopOutboxResolver` ignores write-relays and mailbox data
        // entirely, so following that advice can never produce a target.
        // Point at the real missing step instead. Once a real resolver is
        // composed (even if it legitimately resolves zero relays for this
        // author), the original message is accurate again.
        PublishEngineError::NoTargets if !resolver_composed => (
            "no publish resolver composed — call `nmp_substrate::install(...)` \
             (or `set_publish_resolver_factory`) at your composition root \
             before publishing"
                .to_string(),
            "pending_relays_unknown".to_string(),
            ERR_PERMANENT,
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{
        InMemoryPublishStore, NoopOutboxResolver, PublishAction, PublishEngine, PublishEngineError,
        PublishStoreError, PublishTarget, RelayDispatcher, ReplayDispatcher, RetryPolicy,
        StaticOutbox,
    };
    use nmp_signer_iface::{SignedEvent, UnsignedEvent};
    use std::sync::Arc;

    fn signed_event(author: &str) -> SignedEvent {
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: author.to_string(),
                kind: 1,
                tags: Vec::new(),
                content: String::new(),
                created_at: 1_000,
            },
        }
    }

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
        let (toast_nt, status_nt, cat_nt) =
            describe_engine_error(&PublishEngineError::NoTargets, true);
        assert!(toast_nt.contains("write-relays"));
        assert_eq!(status_nt, "pending_relays_unknown");
        assert_eq!(cat_nt, ERR_PERMANENT);

        let (toast_dup, status_dup, cat_dup) =
            describe_engine_error(&PublishEngineError::DuplicateHandle("h".to_string()), true);
        assert!(toast_dup.contains("already in flight"));
        assert_eq!(status_dup, "duplicate");
        assert_eq!(cat_dup, ERR_TRANSIENT);

        let (toast_store, status_store, cat_store) = describe_engine_error(
            &PublishEngineError::Store(PublishStoreError::Backend("oom".into())),
            true,
        );
        assert!(toast_store.contains("store error"));
        assert_eq!(status_store, "store_error");
        assert_eq!(cat_store, ERR_PERMANENT);

        let (toast_unsupported, status_unsupported, cat_unsupported) = describe_engine_error(
            &PublishEngineError::UnsupportedAction("PublishProfile"),
            true,
        );
        assert!(toast_unsupported.contains("unsupported action"));
        assert_eq!(status_unsupported, "unsupported_action");
        assert_eq!(cat_unsupported, ERR_PERMANENT);
    }

    /// #2937 — the `NoTargets` toast is the ONLY message that branches on
    /// `resolver_composed`. Uncomposed (no `set_outbox` ever ran) must point
    /// at the missing composition step, never the write-relays advice (which
    /// is provably wrong under `NoopOutboxResolver`). Composed-but-empty
    /// keeps the original, accurate advice verbatim.
    #[test]
    fn describe_engine_error_no_targets_branches_on_resolver_composed() {
        let (toast_uncomposed, status_uncomposed, cat_uncomposed) =
            describe_engine_error(&PublishEngineError::NoTargets, false);
        assert!(
            toast_uncomposed.contains("nmp_substrate::install"),
            "uncomposed NoTargets must name the missing composition step: {toast_uncomposed}"
        );
        assert!(
            !toast_uncomposed.contains("write-relays"),
            "uncomposed NoTargets must NOT tell the caller to add a write-relay \
             (NoopOutboxResolver ignores it): {toast_uncomposed}"
        );
        assert_eq!(status_uncomposed, "pending_relays_unknown");
        assert_eq!(cat_uncomposed, ERR_PERMANENT);

        let (toast_composed, status_composed, cat_composed) =
            describe_engine_error(&PublishEngineError::NoTargets, true);
        assert!(toast_composed.contains("write-relays"));
        assert!(!toast_composed.contains("nmp_substrate::install"));
        assert_eq!(status_composed, "pending_relays_unknown");
        assert_eq!(cat_composed, ERR_PERMANENT);
    }

    /// #2937 end-to-end: an engine that never had `set_outbox` called (the
    /// bare `new_app()` shape — `NoopOutboxResolver`) reports
    /// `resolver_composed() == false`, and the resulting `NoTargets` maps to
    /// the composition-aware message. Once a real resolver is installed
    /// (even one that legitimately resolves zero relays for this author),
    /// `resolver_composed()` flips to `true` and the SAME `NoTargets` error
    /// maps to the original write-relays message. Mirrors the harness style
    /// of `crates/nmp-testing/tests/framework_magic_contract/c7_c11.rs`.
    #[test]
    fn uncomposed_engine_reports_composition_aware_no_targets_message() {
        // --- Uncomposed: bare engine, NoopOutboxResolver, set_outbox never called.
        let dispatcher: Arc<dyn RelayDispatcher> = Arc::new(ReplayDispatcher::new());
        let mut uncomposed_engine = PublishEngine::new(
            Arc::new(NoopOutboxResolver),
            dispatcher,
            Arc::new(InMemoryPublishStore::new()),
            RetryPolicy::default(),
        );
        assert!(!uncomposed_engine.resolver_composed());
        let err = uncomposed_engine
            .start_publish(
                PublishAction::Publish {
                    handle: "h-uncomposed".to_string(),
                    event: signed_event("alice"),
                    target: PublishTarget::Auto,
                },
                0,
                None,
            )
            .expect_err("NoopOutboxResolver resolves nothing under PublishTarget::Auto");
        assert!(matches!(err, PublishEngineError::NoTargets));
        let (toast, _, _) = describe_engine_error(&err, uncomposed_engine.resolver_composed());
        assert!(
            toast.contains("nmp_substrate::install"),
            "uncomposed engine's NoTargets must be composition-aware: {toast}"
        );

        // --- Composed but legitimately zero: a real resolver, no author writes seeded.
        let dispatcher2: Arc<dyn RelayDispatcher> = Arc::new(ReplayDispatcher::new());
        let mut composed_engine = PublishEngine::new(
            Arc::new(NoopOutboxResolver),
            dispatcher2,
            Arc::new(InMemoryPublishStore::new()),
            RetryPolicy::default(),
        );
        composed_engine.set_outbox(Arc::new(StaticOutbox::default()));
        assert!(composed_engine.resolver_composed());
        let err2 = composed_engine
            .start_publish(
                PublishAction::Publish {
                    handle: "h-composed".to_string(),
                    event: signed_event("bob"),
                    target: PublishTarget::Auto,
                },
                0,
                None,
            )
            .expect_err("StaticOutbox::default() has no seeded writes for bob either");
        assert!(matches!(err2, PublishEngineError::NoTargets));
        let (toast2, _, _) = describe_engine_error(&err2, composed_engine.resolver_composed());
        assert!(
            toast2.contains("write-relays"),
            "composed-but-empty engine keeps the accurate write-relays advice: {toast2}"
        );
    }
}
