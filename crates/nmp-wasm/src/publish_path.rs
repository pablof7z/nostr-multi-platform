//! V-01 Stage 3c — async publish path for app-level write actions on wasm32.
//!
//! Stage 3b landed the signer slot (`Option<Arc<dyn Signer>>`) and the
//! `Nip07Signer::sign()` JS bridge; every app-level write still returned
//! `publish_path_not_wired` because the kernel's publish surface needed
//! exposure on `KernelReducer` and the trait-level `SignerOp::Pending(rx)`
//! mpsc receiver could not be awaited cleanly on wasm32 (the wasm thread is
//! the JS event-loop thread; `recv_timeout` deadlocks it, `try_recv` in a
//! loop is a polling busy-wait and violates D8).
//!
//! Stage 3c closes both gaps:
//!
//! 1. [`nmp_core::KernelReducer::publish_signed_event`] is the new public
//!    publish-from-signed-event surface (delegates to `Kernel::publish_signed`
//!    + `partition_auth_paused`, byte-identical with the native actor path
//!    `actor::commands::publish::publish_note` reaches after sign).
//! 2. [`nmp_signers::sign_event_via_extension`] is the pure-async twin of
//!    `Nip07Signer::sign()` — returns a real `Future<Output = Result<...>>`
//!    the wasm Promise wrapper can `.await` through `JsFuture` (yields to JS
//!    every await, no busy-poll).
//!
//! [`publish_app_action`] (this module) composes those two seams plus the
//! relay-pool fan-out into a single `js_sys::Promise`-friendly async fn.
//!
//! # Scope (PR boundary, BACKLOG F-01)
//!
//! This module wires **`PublishNote` (kind:1)** only. `React` / `Follow` /
//! `Unfollow` are scoped down to an honest `publish_path_not_wired_for_kind`
//! error pointing at the BACKLOG follow-up. Adding them is small (build the
//! unsigned event for kind:7 / kind:3 / kind:3-edit and the same async path
//! handles them) but each kind has tag-construction subtleties the native
//! `react` / `follow` commands own (e.g. NIP-25 `k` tag derivation, kind:3
//! follow-set merging) that we want to land kind-by-kind so the wasm path
//! does not silently drift from native.

use nmp_signers::SignerBackend;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use nmp_core::{KernelReducer, OutboundMessage};
#[cfg(target_arch = "wasm32")]
use nmp_signers::Signer;

#[cfg(target_arch = "wasm32")]
use crate::protocol::{AppAction, CapabilityFailure, WorkerEvent};
#[cfg(target_arch = "wasm32")]
use crate::relay_driver::BrowserRelayDriver;
#[cfg(target_arch = "wasm32")]
use crate::snapshot::{push_snapshot_if_callback, RuntimeMeta};

/// Stable error-code prefix returned when an `AppAction` variant has not yet
/// been wired through the wasm publish path. The JS host pattern-matches the
/// underscore-snake-case prefix the same way it does for
/// `signer_not_installed` / `publish_path_not_wired`.
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
         wasm publish path. V-01 Stage 3c first PR wired `nmp.publish` (kind:1 notes) only — \
         React / Follow / Unfollow follow up. See BACKLOG F-01."
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

/// Fan a publish-engine outbound batch out to the matching browser-relay
/// drivers. Same URL-match pattern `relay_pool::build_sink` uses for the
/// inbound→outbound (kernel-reply) path — extracted here so the publish path
/// and the relay-pool sink share one implementation.
///
/// The sink is wasm32-only because the underlying `BrowserRelayDriver` is
/// wasm32-only (the native runtime never instantiates one). On native targets
/// this module exposes a no-op shim so the call sites in `runtime.rs` stay
/// shim-free.
#[cfg(target_arch = "wasm32")]
pub(crate) fn fan_out_outbound(
    drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    outbound: &[OutboundMessage],
) {
    let drivers = drivers.borrow();
    for message in outbound {
        if let Some(driver) = drivers.iter().find(|d| d.url() == message.relay_url()) {
            let _ = driver.send_text(message.text());
        }
    }
}

/// V-01 Stage 3c — async publish path executed inside a `js_sys::Promise`.
///
/// Lifecycle:
/// 1. Validate the action variant. Non-`PublishNote` variants return an
///    honest `publish_path_not_wired_for_kind` failure inline (no sign call).
/// 2. Validate the installed signer's backend. Non-`Nip07` backends return
///    an `unsupported_signer_backend_for_writes` failure inline.
/// 3. Build the `UnsignedEvent` (kind:1, no NIP-10 reply-thread structure
///    yet — `reply_to_id` falls into the same follow-up bucket as
///    React/Follow/Unfollow).
/// 4. Await `nmp_signers::sign_event_via_extension(...)` — the JS Promise
///    bridge that yields control to the JS event loop. On rejection we
///    surface the signer error verbatim through `CapabilityFailure`.
/// 5. Borrow the reducer briefly, call `publish_signed_event(&signed, &[])`,
///    drop the borrow. Fan the resulting outbound through the driver pool,
///    then push a fresh snapshot through the callback (same `push_snapshot_if_callback`
///    helper the relay-pool sink uses on every inbound kernel mutation).
/// 6. Resolve the Promise with `WorkerEvent::ActionAccepted` — the host's
///    spinner clears immediately; per-relay terminal verdicts arrive later
///    via the `action_results` projection on the next snapshot push.
///
/// # D4 / D8 / borrow discipline
///
/// - The `reducer: Rc<RefCell<KernelReducer>>` borrow is held only during
///   the synchronous `publish_signed_event` call AFTER the `.await` resolves.
///   No borrow lives across an `.await` boundary — that would panic on the
///   next entry to the reducer through any other call site (snapshot push,
///   inbound relay frame).
/// - The single `.await` (the JS Promise) yields control to the JS event
///   loop in the standard wasm-bindgen-futures way (`JsFuture::from(p).await`).
///   No mpsc `recv_timeout`, no `try_recv` loop — D8 holds.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_app_action(
    action: AppAction,
    correlation_id: String,
    signer: Arc<dyn Signer>,
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<RuntimeMeta>>,
    now_secs: u64,
) -> WorkerEvent {
    use nmp_core::substrate::UnsignedEvent;
    use nmp_signers::sign_event_via_extension;

    // Step 1 — variant gate. Only PublishNote is wired in this PR.
    let (action_type, _payload) = action.clone().into_dispatch_parts();
    let (content, reply_to_id) = match action {
        AppAction::PublishNote {
            content,
            reply_to_id,
        } => (content, reply_to_id),
        _ => {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type.clone(),
                correlation_id,
                reason: write_path_not_wired_for_kind_reason(&action_type),
            });
        }
    };

    // Step 2 — signer backend gate. Only NIP-07 has a wired wasm async path.
    let backend = signer.backend();
    if !matches!(backend, SignerBackend::Nip07) {
        return WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: unsupported_signer_backend_reason(&backend),
        });
    }
    let cached_pubkey = signer.pubkey();

    // `reply_to_id` is intentionally unsupported in this PR — the native
    // `publish_note` builder walks the kernel's `events` read-cache for
    // NIP-10 root/parent reply tags. Wiring that through the wasm async
    // path is a follow-up; failing closed here is more honest than
    // silently dropping the reply marker.
    if reply_to_id.is_some() {
        return WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: write_path_not_wired_for_kind_reason("nmp.publish.reply"),
        });
    }

    // Step 3 — build the unsigned kind:1 event. No tags (a fresh note has
    // none); the extension fills in id/sig.
    let unsigned = UnsignedEvent {
        pubkey: cached_pubkey.to_hex(),
        kind: 1,
        tags: Vec::new(),
        content,
        created_at: now_secs,
    };

    // Step 4 — await the extension. This is the only `.await` in the path;
    // the reducer is NOT borrowed across it.
    let signed = match sign_event_via_extension(cached_pubkey, unsigned).await {
        Ok(s) => s,
        Err(error) => {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type,
                correlation_id,
                reason: format!("nip07_sign_failed: {error}"),
            });
        }
    };

    // Step 5 — synchronous publish + fan-out. Borrows are short-lived and
    // sequential; nothing crosses an `.await`.
    //
    // `correlation_id` is threaded into the publish engine via
    // `KernelReducer::publish_signed_event(... , Some(id))` so per-relay
    // terminal verdicts land in the `action_results` projection keyed on
    // the dispatch id the JS host knows — matching the native dispatched
    // `publish_note` path's `publish_signed_to_with_correlation` call.
    // Without this thread the host's partial-success UX (`"2/3 relays
    // accepted"`) would have no key to correlate on, since the synthetic
    // event id never leaves the wasm runtime.
    let outbound = {
        let mut r = reducer.borrow_mut();
        r.publish_signed_event(&signed, &[], Some(correlation_id.clone()))
    };
    fan_out_outbound(&drivers, &outbound);

    // Push a fresh snapshot so the host sees the new publish-queue entry
    // (status: "accepted_locally") immediately. The same helper the relay
    // sink uses on every inbound frame — single source of truth for the
    // push contract.
    push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);

    // Step 6 — resolve with ActionAccepted. Per-relay terminal verdicts
    // (OK acks) arrive later through the snapshot-push channel via the
    // `action_results` projection drained on the next inbound that mutates
    // kernel state.
    WorkerEvent::ActionAccepted {
        action_type,
        correlation_id,
    }
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
        let reason = write_path_not_wired_for_kind_reason("nmp.nip25.react");
        assert!(reason.starts_with("publish_path_not_wired_for_kind"));
        assert!(reason.contains("nmp.nip25.react"));
    }

    #[test]
    fn unsupported_signer_backend_reason_has_stable_prefix() {
        let reason = unsupported_signer_backend_reason(&SignerBackend::LocalKey);
        assert!(reason.starts_with("unsupported_signer_backend_for_writes"));
    }
}
