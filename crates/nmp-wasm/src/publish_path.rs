//! V-01 Stage 3c — async publish path for app-level write actions on wasm32.
//!
//! Wires `PublishNote` (kind:1, NIP-10 replies), `React` (kind:7), and
//! `Follow` / `Unfollow` (kind:3) through [`nmp_core::KernelReducer`] seams
//! and [`nmp_signers::sign_event_via_extension`] (the pure-`async` NIP-07
//! bridge). All borrows are dropped before the sign `.await` (D4/D8).
//! Follow/Unfollow fail closed with `follow_list_not_loaded` when kind:3 has
//! not been ingested yet; bad pubkey targets fail with `follow_target_invalid`.

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

/// Local mirror of `nmp_core::kernel::nostr::is_hex_pubkey` — that function is
/// not re-exported by `nmp-core` on wasm32 (it lives behind the native-only
/// `__ffi_internal` module). Same 64-char ASCII-hexdigit rule as the kernel.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn is_hex_pubkey(v: &str) -> bool {
    v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit())
}

// The fan-out helper previously lived here with a `.find()` loop (first
// matching driver only), which dropped frames addressed to `"both"`-role
// URLs that spawn two drivers sharing the same relay URL (issue #1143 fix 2).
// It has been unified into `crate::relay_pool::fan_out_outbound`, which uses
// `.filter()` so every matching driver receives the frame.  All callers
// (`tick::start_tick_interval`, `runtime::WasmRuntime::fan_outbound`, and
// `publish_app_action` below) now route through that single implementation.

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
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
async fn sign_and_publish(
    unsigned: nmp_core::substrate::UnsignedEvent,
    cached_pubkey: nmp_signers::PublicKey,
    action_type: String,
    correlation_id: String,
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
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
    crate::relay_pool::fan_out_outbound(&drivers, &outbound);
    push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
    WorkerEvent::ActionAccepted {
        action_type,
        correlation_id,
    }
}

/// V-01 Stage 3c — async publish path executed inside a `js_sys::Promise`.
///
/// Each arm validates its inputs synchronously (all borrows dropped before any
/// `.await`), builds the kind-specific [`UnsignedEvent`], then delegates to
/// [`sign_and_publish`] for sign / publish / fan-out / snapshot / `ActionAccepted`.
/// Unknown variants return `publish_path_not_wired_for_kind` without signing.
/// Non-NIP-07 signers return `unsupported_signer_backend_for_writes`.
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

    // Step 1 — variant gate: React (kind:7), Follow/Unfollow (kind:3), Note
    // (kind:1) are wired; anything else fails closed without signing.
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

        return sign_and_publish(unsigned, cached_pubkey, action_type, correlation_id,
            reducer, drivers, snapshot_callback, meta).await;
    }

    // Step 1b — Follow / Unfollow branch (kind:3). `action` was not React;
    // check for Follow/Unfollow before falling through to Note.
    if let AppAction::Follow { pubkey } | AppAction::Unfollow { pubkey } = &action {
        let is_add = matches!(action, AppAction::Follow { .. });

        // Hex-pubkey gate (matches native follow() in publish.rs): reject
        // npub/bech32/garbage targets before touching the follow list.
        // Canonicalize to lowercase so uppercase-hex doesn't create duplicate
        // or fail-to-remove against an already-lowercase entry.
        if !is_hex_pubkey(pubkey) {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type,
                correlation_id,
                reason: follow_target_invalid_reason(pubkey),
            });
        }
        let pubkey = pubkey.to_lowercase();

        // Signer backend gate — same constraint as Note and React.
        let backend = signer.backend();
        if !matches!(backend, SignerBackend::Nip07) {
            return WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_type,
                correlation_id,
                reason: unsupported_signer_backend_reason(&backend),
            });
        }
        let cached_pubkey = signer.pubkey();

        // Safety gate: read the active account's FULL existing kind:3 raw event
        // (every tag verbatim + content) before the sign await. `None` means
        // the kind:3 has not been ingested yet — fail closed to prevent
        // silently overwriting an unloaded contact list. The borrow is dropped
        // before the await (RefCell borrow discipline).
        let current_kind3 = {
            let r = reducer.borrow();
            r.try_current_kind3_event()
        };
        let (current_tags, current_content) = match current_kind3 {
            Some(event) => event,
            None => {
                return WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: action_type,
                    correlation_id,
                    reason: follow_list_not_loaded_reason(),
                });
            }
        };

        // Splice ONLY the `p` section via the shared canonical editors — every
        // non-`p` tag, every existing follow's relay-hint + petname, and the
        // original content are preserved on re-publish (issue #1246a).
        let tags = if is_add {
            nmp_core::tags::kind3_tags_after_add(&current_tags, &pubkey)
        } else {
            nmp_core::tags::kind3_tags_after_remove(&current_tags, &pubkey)
        };

        let unsigned = nmp_core::substrate::UnsignedEvent {
            pubkey: cached_pubkey.to_hex(),
            kind: 3,
            tags,
            content: current_content,
            created_at: now_secs,
        };

        return sign_and_publish(unsigned, cached_pubkey, action_type, correlation_id,
            reducer, drivers, snapshot_callback, meta).await;
    }

    // Step 1c — Note branch. `action` was not React or Follow/Unfollow.
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

    // Step 3 — resolve NIP-10 reply tags synchronously (borrow dropped before
    // the sign await, D4/D8), then build the unsigned kind:1 event.
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

    let unsigned = UnsignedEvent {
        pubkey: cached_pubkey.to_hex(),
        kind: 1,
        tags,
        content,
        created_at: now_secs,
    };

    // Steps 4-6 — sign via NIP-07, publish + fan-out, push snapshot, return
    // ActionAccepted. The `correlation_id` is threaded into the publish engine
    // so per-relay verdicts land in `action_results` keyed on the dispatch id
    // the JS host knows (matching the native `publish_note` path).
    sign_and_publish(unsigned, cached_pubkey, action_type, correlation_id,
        reducer, drivers, snapshot_callback, meta).await
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
}
