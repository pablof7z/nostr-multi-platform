//! V-01 Stage 3c — async publish path for app-level write actions on wasm32.
//!
//! **Publishing is disabled in the web preview build** (see `#1202`):
//! `publish_app_action` returns a `publish_not_supported_in_web_preview`
//! `CapabilityFailure` for every app-level write action before any NIP-07 sign
//! step is attempted. The full composition root — including a real
//! `Nip65OutboxResolver` — is deferred to the post-v1 web milestone (#1007).
//!
//! Without this gate the wasm kernel's `NoopOutboxResolver` default would
//! resolve zero relays (`PublishTarget::Auto` → `NoTargets`) and silently
//! swallow every publish: the host receives `ActionAccepted` but no event
//! reaches the wire. That "works-but-wrong" state violates the zero-tolerance
//! rule (AGENTS.md §zero-debt) and aim.md's "make it nearly impossible to
//! build a broken app" goal.
//!
//! The gate is intentionally first — before signer-backend checks, before any
//! `sign_and_publish` call — so a JS host cannot accidentally believe a publish
//! succeeded. The pattern-matchable prefix `publish_not_supported_in_web_preview`
//! lets the host surface an honest "publishing is not available in this preview"
//! banner rather than a spinner that never resolves.
//!
//! The remaining reason helpers (`write_path_not_wired_for_kind`,
//! `reply_target_unknown`, `react_target_invalid`, `unsupported_signer_backend`,
//! `follow_list_not_loaded`, `follow_target_invalid`) are kept compiled and
//! tested so they remain correct when the real publish path is wired in #1007.

use nmp_signers::SignerBackend;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use nmp_core::KernelReducer;
#[cfg(target_arch = "wasm32")]
use nmp_signers::Signer;

#[cfg(target_arch = "wasm32")]
use crate::protocol::{AppAction, CapabilityFailure, WorkerEvent};
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};
#[cfg(target_arch = "wasm32")]
use crate::snapshot::{push_snapshot_if_callback, RuntimeMeta};

/// Stable error-code prefix returned when an `AppAction` variant has not yet
/// been wired through the wasm publish path. The JS host pattern-matches the
/// underscore-snake-case prefix the same way it does for
/// `signer_not_installed` / `publish_not_supported_in_web_preview`.
///
/// Always-compiled (the native test target compiles the runtime + protocol-
/// conformance tests through the same crate, and the tests below pin the
/// prefix contract on both targets). On native the production call site
/// inside `publish_app_action` is `cfg(wasm32)`-gated, so without
/// `cfg_attr(test, …)` rustc flags this as dead — the lint suppression keeps
/// the helper available for the wasm path AND the cross-target tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn write_path_not_wired_for_kind_reason(action_type: &str) -> String {
    format!(
        "publish_path_not_wired_for_kind: action {action_type:?} is not yet wired through the \
         wasm publish path."
    )
}

/// Stable error-code prefix returned when the `reply_to_id` supplied to
/// `PublishNote` is not a repliable event in the kernel's local store.
///
/// "Not repliable" covers two cases: the event is absent from the store, or
/// it is present but not a kind:1 note (kind:0 profiles, kind:6 reposts, etc.
/// are not valid NIP-10 reply targets). The wasm path fails closed in both
/// cases — silently dropping the reply marker would produce a malformed
/// NIP-10 thread. The prefix is pinned by the test below.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn reply_target_unknown_reason(reply_to_id: &str) -> String {
    format!(
        "reply_target_unknown: event {reply_to_id:?} is not a repliable note in the \
         kernel's local store (absent or not kind:1). The parent must be a kind:1 \
         note in the local feed before a reply can be composed."
    )
}

/// Stable error-code prefix returned when the `target_event_id` supplied to
/// `React` is not a valid 64-char hex event id.
///
/// `build_reaction_draft` returns `None` for this case; the wasm path fails
/// closed — silently ignoring the reaction would publish an ill-formed
/// kind:7 event. The prefix is pinned by the test below.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn react_target_invalid_reason(target_event_id: &str) -> String {
    format!(
        "react_target_invalid: target event id {target_event_id:?} is not a valid \
         64-char hex event id."
    )
}

/// Stable error-code prefix returned when an installed signer is the wrong
/// backend kind for the wasm publish path. Stage 3c wires NIP-07 only —
/// LocalKey can't run in the wasm runtime (the runtime should not hold key
/// material) and NIP-46 bunker on wasm needs a wasm-native NIP-46 transport
/// the broker side does not yet expose.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn unsupported_signer_backend_reason(backend: &SignerBackend) -> String {
    format!(
        "unsupported_signer_backend_for_writes: installed signer backend {backend:?} cannot \
         drive wasm writes today. NIP-07 is the only wired kind in V-01 Stage 3c."
    )
}

/// Stable error-code prefix returned when a Follow or Unfollow action cannot
/// proceed because the active account's kind:3 contact list has not been
/// ingested yet.
///
/// Publishing a kind:3 without a loaded baseline would silently wipe the
/// user's existing contact list — so the wasm path fails closed with this
/// reason instead. The host should wait for the contact-list snapshot
/// projection to populate before retrying.
///
/// The prefix is pinned by a test below so JS host pattern-matching is stable.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn follow_list_not_loaded_reason() -> String {
    "follow_list_not_loaded: the active account's kind:3 contact list has not been \
     ingested yet. Retry after the contact-list snapshot projection is populated."
        .to_string()
}

/// Stable error-code prefix returned when the `pubkey` target supplied to a
/// `Follow` or `Unfollow` action is not a valid 64-hex lowercase pubkey.
///
/// The wasm path fails closed — signing and publishing a garbage/npub string
/// into the user's kind:3 would pollute it; the target must be a valid hex
/// pubkey before the follow list is edited. The prefix is pinned by a test
/// below so JS host pattern-matching is stable.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn follow_target_invalid_reason(target: &str) -> String {
    format!(
        "follow_target_invalid: follow/unfollow target {target:?} is not a valid 64-hex pubkey. \
         Supply a lowercase hex pubkey, not an npub or other encoding."
    )
}

/// Stable error-code prefix returned when any app-level write action is
/// dispatched to the wasm async publish path while the wasm composition root
/// does not wire a real `OutboxResolver`.
///
/// The wasm kernel starts with `NoopOutboxResolver` as its default: every
/// `publish_signed_event` call would resolve zero targets for
/// `PublishTarget::Auto` → `PublishEngineError::NoTargets` → silent drop with
/// `ActionAccepted` returned to the host. That is a silent always-fail, which
/// the zero-debt rule forbids (AGENTS.md §zero-debt; aim.md §honesty).
///
/// **This gate fires BEFORE the signer-backend check and BEFORE any sign
/// step** so it is impossible for a JS caller to receive `ActionAccepted`
/// when no event reaches the wire. The host should surface a banner such as
/// "Publishing is not available in this web preview" and disable compose
/// controls until the real composition root ships in #1007.
///
/// Always-compiled so the native integration-test suite can pin the prefix
/// without a `cfg(target_arch = "wasm32")` guard. The production call site
/// inside `publish_app_action` is already `cfg(wasm32)`-gated; the
/// `allow(dead_code)` on the non-wasm path is the standard pattern used by
/// every other reason-string helper in this module.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn publish_not_supported_in_web_preview_reason(action_type: &str) -> String {
    format!(
        "publish_not_supported_in_web_preview: action {action_type:?} cannot be published \
         from the web preview build. The wasm runtime has no outbox resolver wired \
         (NoopOutboxResolver resolves zero relay targets), so every publish would be \
         silently dropped. The full wasm composition root with a real OutboxResolver \
         is deferred to the post-v1 web milestone (#1007). Disable compose controls \
         and surface an honest 'not available in this preview' state to the user."
    )
}

/// Local mirror of `nmp_core::kernel::nostr::is_hex_pubkey` — that function is
/// not re-exported by `nmp-core` on wasm32 (it lives behind the native-only
/// `__ffi_internal` module). Same 64-char ASCII-hexdigit rule as the kernel.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn is_hex_pubkey(v: &str) -> bool {
    v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit())
}

// The fan-out helper previously lived here with a `.find()` loop (first
// matching driver only). It has been unified into
// `crate::relay_pool::fan_out_outbound`, which is now URL-keyed (one driver
// per relay URL) and spawns a driver on demand for any kernel-targeted URL
// not yet in the pool. All callers (`tick::start_tick_interval`,
// `runtime::WasmRuntime::fan_outbound`, and `publish_app_action` below) route
// through that single implementation.

/// Shared sign-and-publish tail for all [`publish_app_action`] arms.
///
/// Every arm builds its kind-specific [`UnsignedEvent`] then delegates here:
/// 1. Await `sign_event_via_extension` (NIP-07 JS Promise — only `.await`).
/// 2. Publish the signed event through `publish_signed_event` (short borrow).
/// 3. Fan the outbound frame to every connected relay driver.
/// 4. Push a fresh snapshot so the host sees the queued entry immediately.
/// 5. Return `ActionAccepted` — per-relay verdicts arrive later via snapshots.
///
/// No borrow lives across the `.await` (D4/D8).
///
/// Currently unreachable while the honest-disable gate in `publish_app_action`
/// is active (#1202). Kept compiled so #1007 can restore the signing arms
/// from the git history without a separate compilation fix.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments, dead_code)]
async fn sign_and_publish(
    unsigned: nmp_core::substrate::UnsignedEvent,
    cached_pubkey: nmp_signers::PublicKey,
    action_type: String,
    correlation_id: String,
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    handlers_slot: Rc<RefCell<Option<BrowserKernelHandlers>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<RuntimeMeta>>,
) -> WorkerEvent {
    let signed = match nmp_signers::sign_event_via_extension(cached_pubkey, unsigned).await {
        Ok(s) => s,
        Err(error) => {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type,
                correlation_id,
                reason: format!("nip07_sign_failed: {error}"),
            });
        }
    };
    let outbound = {
        let mut r = reducer.borrow_mut();
        r.publish_signed_event(&signed, &[], Some(correlation_id.clone()))
    };
    crate::relay_pool::fan_out_outbound(&drivers, &handlers_slot, &outbound);
    push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
    WorkerEvent::ActionAccepted {
        action_type,
        correlation_id,
    }
}

/// V-01 Stage 3c — async publish path executed inside a `js_sys::Promise`.
///
/// **All arms return `publish_not_supported_in_web_preview` immediately** while
/// the wasm runtime has no real `OutboxResolver` wired (#1202). The
/// `NoopOutboxResolver` default resolves zero relay targets for
/// `PublishTarget::Auto`, so every `publish_signed_event` call would silently
/// produce `NoTargets` while returning `ActionAccepted` to the host — a silent
/// always-fail. The early gate fires BEFORE any NIP-07 sign step, making it
/// impossible for the JS caller to receive `ActionAccepted` for an event that
/// was never sent.
///
/// The remaining arms (React, Follow/Unfollow, Note) are intentionally kept in
/// place: they will be re-enabled when the real composition root ships in #1007
/// by removing the gate below.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_app_action(
    action: AppAction,
    correlation_id: String,
    // The parameters below are intentionally kept in the signature so the
    // wasm-bindgen entrypoint (`runtime::WasmRuntime::start_publish_app_action`)
    // continues to compile without changes when the gate is removed in #1007.
    _signer: Arc<dyn Signer>,
    _reducer: Rc<RefCell<KernelReducer>>,
    _drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    _snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    _meta: Rc<RefCell<RuntimeMeta>>,
    _now_secs: u64,
) -> WorkerEvent {
    // Step 0 — honest-disable gate (#1202).
    //
    // The wasm kernel starts with `NoopOutboxResolver` as its default: every
    // `publish_signed_event` call resolves zero relay targets for
    // `PublishTarget::Auto` → `PublishEngineError::NoTargets` → silent drop
    // with an empty outbound vec. Before this gate the caller received
    // `ActionAccepted` even though no event ever reached a relay — a silent
    // always-fail violating the zero-debt rule (AGENTS.md §zero-debt) and
    // aim.md's "make it nearly impossible to build a broken app" goal.
    //
    // Return an explicit `CapabilityFailure` BEFORE any NIP-07 sign step so
    // the JS host cannot mistake a dropped publish for a successful one. The
    // host should surface "publishing is not available in this web preview"
    // and disable compose controls until #1007 wires the real composition
    // root (Nip65OutboxResolver + write relays).
    //
    // To re-enable publishing in #1007: remove this early return and restore
    // the signing arms from the git history of this file.
    let (action_type, _payload) = action.into_dispatch_parts();
    WorkerEvent::CapabilityFailure(CapabilityFailure {
        capability: action_type.clone(),
        correlation_id,
        reason: publish_not_supported_in_web_preview_reason(&action_type),
    })
}

// ─── native shims ────────────────────────────────────────────────────────────
//
// The async publish path is wasm32-only — there is no `js_sys::Function`,
// `BrowserRelayDriver`, or JS event loop to talk to on native. On native we
// expose nothing from this module beyond the pure-reason-string helpers
// (which `runtime.rs` uses through the same module path on both targets).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_path_not_wired_for_kind_reason_has_stable_prefix() {
        // React / Follow / Unfollow are now wired; use a genuinely-unwired action.
        let reason = write_path_not_wired_for_kind_reason("nmp.hypothetical_future_verb");
        assert!(reason.starts_with("publish_path_not_wired_for_kind"));
        assert!(reason.contains("nmp.hypothetical_future_verb"));
    }

    #[test]
    fn follow_list_not_loaded_reason_has_stable_prefix() {
        let reason = follow_list_not_loaded_reason();
        assert!(
            reason.starts_with("follow_list_not_loaded:"),
            "prefix must be 'follow_list_not_loaded:'; got: {reason:?}"
        );
    }

    #[test]
    fn react_target_invalid_reason_has_stable_prefix() {
        let reason = react_target_invalid_reason("not-hex");
        assert!(
            reason.starts_with("react_target_invalid:"),
            "prefix must be 'react_target_invalid:'; got: {reason:?}"
        );
        assert!(reason.contains("not-hex"), "reason must echo the event id");
    }

    #[test]
    fn unsupported_signer_backend_reason_has_stable_prefix() {
        let reason = unsupported_signer_backend_reason(&SignerBackend::LocalKey);
        assert!(reason.starts_with("unsupported_signer_backend_for_writes"));
    }

    #[test]
    fn reply_target_unknown_reason_has_stable_prefix() {
        let reason = reply_target_unknown_reason("deadbeef");
        assert!(
            reason.starts_with("reply_target_unknown:"),
            "prefix must be 'reply_target_unknown:'; got: {reason:?}"
        );
        assert!(reason.contains("deadbeef"), "reason must echo the event id");
    }

    #[test]
    fn follow_target_invalid_reason_has_stable_prefix() {
        // npub1 / bech32 inputs must be rejected with a stable prefix so the
        // JS host pattern-match does not break across refactors.
        let reason = follow_target_invalid_reason("npub1abc");
        assert!(
            reason.starts_with("follow_target_invalid:"),
            "prefix must be 'follow_target_invalid:'; got: {reason:?}"
        );
        assert!(reason.contains("npub1abc"), "reason must echo the target");
        // Non-64-char hex also fails.
        let reason2 = follow_target_invalid_reason("deadbeef");
        assert!(reason2.starts_with("follow_target_invalid:"));
        // Valid 64-hex (lowercase) does NOT produce this reason — it passes
        // the gate (verified via is_hex_pubkey in the wasm arm, not here).
    }

    /// Pin the `publish_not_supported_in_web_preview` prefix so JS host
    /// pattern-matching is stable across refactors (#1202 regression guard).
    ///
    /// The gate in `publish_app_action` fires for every app-level write while
    /// the wasm runtime has no real `OutboxResolver` wired. JS hosts must
    /// pattern-match this prefix to surface an honest "not available in this
    /// preview" state; any refactor that changes the prefix breaks that contract
    /// and surfaces a confusing raw reason string to the user.
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
