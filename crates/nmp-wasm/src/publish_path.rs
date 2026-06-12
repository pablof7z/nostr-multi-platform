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
//! # Scope
//!
//! This module wires **`PublishNote` (kind:1)** including NIP-10 threaded
//! replies, and **`React` (kind:7)** via
//! [`nmp_core::KernelReducer::build_reaction_draft`]. `Follow` / `Unfollow`
//! remain scoped to an honest `publish_path_not_wired_for_kind` error —
//! kind:3 follow-set editing requires a `try_current_follows` kernel seam to
//! distinguish "kind:3 not yet loaded" from "zero follows", to avoid silently
//! destroying a user's existing contact list (PR-6b).
//!
//! Reply tags are resolved through [`nmp_core::KernelReducer::build_reply_tags`]
//! before the sign `.await` so no `RefCell` borrow ever lives across an async
//! boundary. An unknown parent (event not in the local store) fails closed with
//! a `reply_target_unknown:` reason rather than silently discarding the marker.

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
use nmp_network::browser_driver::BrowserRelayDriver;
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
         Follow / Unfollow follow up. See issue #1007."
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

// The fan-out helper previously lived here with a `.find()` loop (first
// matching driver only), which dropped frames addressed to `"both"`-role
// URLs that spawn two drivers sharing the same relay URL (issue #1143 fix 2).
// It has been unified into `crate::relay_pool::fan_out_outbound`, which uses
// `.filter()` so every matching driver receives the frame.  All callers
// (`tick::start_tick_interval`, `runtime::WasmRuntime::fan_outbound`, and
// `publish_app_action` below) now route through that single implementation.

/// V-01 Stage 3c — async publish path executed inside a `js_sys::Promise`.
///
/// Lifecycle:
/// 1. Validate the action variant. `React` (kind:7) and `PublishNote` (kind:1)
///    are wired; `Follow` / `Unfollow` and all other variants return an honest
///    `publish_path_not_wired_for_kind` failure inline (no sign call).
/// 2. Validate the installed signer's backend. Non-`Nip07` backends return
///    an `unsupported_signer_backend_for_writes` failure inline.
/// 3. Resolve NIP-10 reply tags (if `reply_to_id` is set) via
///    `reducer.borrow().build_reply_tags(id)` — a synchronous store lookup
///    that returns `None` for unknown parents (fail-closed with
///    `reply_target_unknown:` reason). Borrow is dropped before the await.
/// 4. Build the `UnsignedEvent` (kind:1) with the resolved tags.
/// 5. Await `nmp_signers::sign_event_via_extension(...)` — the JS Promise
///    bridge that yields control to the JS event loop. On rejection we
///    surface the signer error verbatim through `CapabilityFailure`.
/// 6. Borrow the reducer briefly, call `publish_signed_event(&signed, &[])`,
///    drop the borrow. Fan the resulting outbound through the driver pool,
///    then push a fresh snapshot through the callback (same `push_snapshot_if_callback`
///    helper the relay-pool sink uses on every inbound kernel mutation).
/// 7. Resolve the Promise with `WorkerEvent::ActionAccepted` — the host's
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

    // Step 1 — variant gate. React (kind:7) and PublishNote (kind:1) are
    // wired; Follow / Unfollow remain fail-closed (PR-6b).
    let (action_type, _payload) = action.clone().into_dispatch_parts();

    // Step 1a — React branch. Extract data by reference before `action` is
    // consumed by the Note match below. The borrow of `action` is released at
    // the end of the `if let` block; any path that enters the block returns,
    // so `action` is available for the Note match only when action is NOT React.
    if let AppAction::React {
        target_event_id,
        reaction,
    } = &action
    {
        let target_event_id = target_event_id.clone();
        let reaction = reaction.clone();
        // action borrow ends here (clones are owned values).

        // Signer backend gate — same constraint as Note.
        let backend = signer.backend();
        if !matches!(backend, SignerBackend::Nip07) {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type,
                correlation_id,
                reason: unsupported_signer_backend_reason(&backend),
            });
        }
        let cached_pubkey = signer.pubkey();

        // Build NIP-25 tags + normalised content via the shared kernel seam.
        // Synchronous read-cache lookup — borrow dropped before the sign
        // await (RefCell borrow discipline, D4/D8). Returns None only for
        // invalid hex — author absence degrades to e-tag-only (D6).
        let (tags, content) =
            match reducer.borrow().build_reaction_draft(&target_event_id, &reaction) {
                Some(draft) => draft,
                None => {
                    return WorkerEvent::CapabilityFailure(CapabilityFailure {
                        capability: action_type,
                        correlation_id,
                        reason: react_target_invalid_reason(&target_event_id),
                    });
                }
            };

        let unsigned = UnsignedEvent {
            pubkey: cached_pubkey.to_hex(),
            kind: 7,
            tags,
            content,
            created_at: now_secs,
        };

        // Await the extension — only await in this path; reducer is not
        // borrowed across it.
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

        // Publish + fan-out. Thread `correlation_id` into the engine so
        // per-relay terminal verdicts land in `action_results` under the
        // dispatch id the JS host is waiting on — same contract as the Note
        // branch below (Step 5). Without this the engine keys on the signed
        // event id the host never sees, breaking the action-result promise.
        let outbound = {
            let mut r = reducer.borrow_mut();
            r.publish_signed_event(&signed, &[], Some(correlation_id.clone()))
        };
        crate::relay_pool::fan_out_outbound(&drivers, &outbound);
        push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);

        return WorkerEvent::ActionAccepted {
            action_type,
            correlation_id,
        };
    }

    // Step 1b — Note branch. `action` was not React; destructure it here.
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

    // Step 3 — resolve NIP-10 reply tags (if replying) and build the
    // unsigned kind:1 event.
    //
    // The store lookup happens HERE — before the sign `.await` — so no
    // `RefCell` borrow lives across the async boundary (borrow discipline,
    // kernel_reducer.rs doc §D4/D8). `build_reply_tags` takes `&self` so
    // the borrow is released the moment the block exits.
    let tags = if let Some(ref rid) = reply_to_id {
        match reducer.borrow().build_reply_tags(rid) {
            Some(t) => t,
            None => {
                return WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: action_type,
                    correlation_id,
                    reason: reply_target_unknown_reason(rid),
                });
            }
        }
    } else {
        Vec::new()
    };

    // Step 3 — build the unsigned kind:1 event. Tags are empty for a fresh
    // root note; a reply carries the NIP-10 marked-form e/p tags resolved
    // above.
    let unsigned = UnsignedEvent {
        pubkey: cached_pubkey.to_hex(),
        kind: 1,
        tags,
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
    crate::relay_pool::fan_out_outbound(&drivers, &outbound);

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
        // React (nmp.nip25.react) is now wired; use a genuinely-unwired action.
        let reason = write_path_not_wired_for_kind_reason("nmp.follow");
        assert!(reason.starts_with("publish_path_not_wired_for_kind"));
        assert!(reason.contains("nmp.follow"));
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
}
