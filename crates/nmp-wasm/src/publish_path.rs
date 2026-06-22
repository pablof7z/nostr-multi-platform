//! Honest write-path disable token for the wasm runtime.
//!
//! **Publishing is disabled in the web preview build** (see `#1202`/`#1008`):
//! every app-level write surfaces a `publish_not_supported_in_web_preview`
//! `CapabilityFailure` because the wasm composition root has no real
//! `OutboxResolver` wired. Without that gate the wasm kernel's
//! `NoopOutboxResolver` default would resolve zero relays
//! (`PublishTarget::Auto` → `NoTargets`) and silently swallow every publish:
//! the host would receive `ActionAccepted` but no event would reach the wire.
//! That "works-but-wrong" state violates the zero-tolerance rule
//! (AGENTS.md §zero-debt) and aim.md's "make it nearly impossible to build a
//! broken app" goal.
//!
//! ADR-0064 §5 removed the wasm `Arc<dyn Signer>.await`-inside-the-publish-flow
//! path (`publish_app_action` / `start_publish_app_action` /
//! `dispatch_app_action_async`): a signed wasm write is the ADR-0050 capability
//! round-trip (`BeginSign` → `SignRequest` → `DeliverSignerResponse`), driven by
//! pure message re-entry. The reducer never awaits the world (D7/D8). The full
//! wasm publish composition root is deferred to the post-v1 web milestone
//! (#1008); until then this single token is the only honest answer.

/// Stable error-code prefix returned when any app-level write action is
/// dispatched while the wasm composition root does not wire a real
/// `OutboxResolver`.
///
/// The wasm kernel starts with `NoopOutboxResolver` as its default: every
/// `publish_signed_event` call would resolve zero targets for
/// `PublishTarget::Auto` → `PublishEngineError::NoTargets` → silent drop with
/// `ActionAccepted` returned to the host. That is a silent always-fail, which
/// the zero-debt rule forbids (AGENTS.md §zero-debt; aim.md §honesty).
///
/// The host should pattern-match this prefix to surface a banner such as
/// "Publishing is not available in this web preview" and disable compose
/// controls until the real composition root ships in #1008.
pub(crate) fn publish_not_supported_in_web_preview_reason(action_type: &str) -> String {
    format!(
        "publish_not_supported_in_web_preview: action {action_type:?} cannot be published \
         from the web preview build. The wasm runtime has no outbox resolver wired \
         (NoopOutboxResolver resolves zero relay targets), so every publish would be \
         silently dropped. The full wasm composition root with a real OutboxResolver \
         is deferred to the post-v1 web milestone (#1008). Disable compose controls \
         and surface an honest 'not available in this preview' state to the user."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the `publish_not_supported_in_web_preview` prefix so JS host
    /// pattern-matching is stable across refactors (#1202 regression guard).
    ///
    /// The gate fires for every app-level write while the wasm runtime has no
    /// real `OutboxResolver` wired. JS hosts must pattern-match this prefix to
    /// surface an honest "not available in this preview" state; any refactor
    /// that changes the prefix breaks that contract.
    #[test]
    fn publish_not_supported_in_web_preview_reason_has_stable_prefix() {
        let reason = publish_not_supported_in_web_preview_reason("nmp.publish");
        assert!(
            reason.starts_with("publish_not_supported_in_web_preview:"),
            "prefix must be 'publish_not_supported_in_web_preview:'; got: {reason:?}"
        );
        assert!(
            reason.contains("nmp.publish"),
            "reason must echo the action type so the host can log it; got: {reason:?}"
        );
        // Verify the prefix is stable regardless of the action variant.
        let reason2 = publish_not_supported_in_web_preview_reason("nmp.nip25.react");
        assert!(
            reason2.starts_with("publish_not_supported_in_web_preview:"),
            "prefix must be consistent across action types; got: {reason2:?}"
        );
    }
}
