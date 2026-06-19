//! Pure helpers around the runtime's dispatch routing surface.
//!
//! Three responsibilities:
//!
//! 1. [`kernel_action_from_dispatch`] — map a generic
//!    [`crate::protocol::ActionDispatch`] to a [`nmp_core::KernelAction`] if
//!    (and only if) the `action_type` is in the kernel namespace. Returns
//!    `None` for app-namespaced actions, which the runtime surfaces through
//!    the write-path-unavailable error path.
//!
//! 2. [`claim_dispatch_from_action`] / [`execute_claim_dispatch`] — parse and
//!    execute `nmp.kernel.claim_*` / `nmp.kernel.release_*` operations via the
//!    `KernelReducer` claim surface.
//!
//! 3. [`interest_dispatch_from_action`] / [`execute_interest_dispatch`] —
//!    parse and execute PR-3 feed-verb operations:
//!    `nmp.kernel.open_interest`, `nmp.kernel.close_interest`,
//!    `nmp.kernel.open_contact_feed`, `nmp.kernel.close_contact_feed`.
//!
//! 4. Stable, host-pattern-matchable reason strings for the two
//!    write-unavailability states the wasm runtime can honestly report
//!    (`signer_not_installed`, `publish_path_not_wired`) plus the
//!    capability-completion failure reason (`browser_actor_driver_missing`).
//!
//! Split out of `runtime.rs` so the file stays under the 500-LOC ceiling and
//! the routing table has a single owner that codegen / kernel-namespace
//! additions touch directly.

use std::sync::Arc;

use nmp_core::{KernelAction, KernelReducer, OutboundMessage};
use nmp_signers::Signer;
use serde_json::Value;

use crate::protocol::ActionDispatch;

/// Decoded claim/release operation extracted from an `ActionDispatch` whose
/// `action_type` is in the `nmp.kernel.claim_*` / `nmp.kernel.release_*`
/// namespace.
///
/// Claims are NOT `KernelAction`s — they are a separate concern (claim
/// registry vs. interest registry). `kernel_action_from_dispatch` returns
/// `None` for claim action types; this function handles them instead.
///
/// D6 — total: a missing or non-string payload field returns `None`; the
/// caller treats `None` as "not a claim dispatch" and falls through to the
/// write-path-unavailable path. No panic.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClaimDispatch {
    ClaimProfile {
        pubkey: String,
        consumer_id: String,
    },
    ReleaseProfile {
        pubkey: String,
        consumer_id: String,
    },
    ClaimEvent {
        uri: String,
        consumer_id: String,
    },
    ReleaseEvent {
        uri: String,
        consumer_id: String,
    },
}

/// Parse an `ActionDispatch` as a claim/release operation. Returns `None`
/// if the `action_type` is not a claim namespace or a required payload field
/// is absent / non-string (D6: malformed → `None`, never a panic).
pub(crate) fn claim_dispatch_from_action(action: &ActionDispatch) -> Option<ClaimDispatch> {
    match action.action_type.as_str() {
        "nmp.kernel.claim_profile" => {
            let pubkey = str_field(&action.payload, "pubkey")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(ClaimDispatch::ClaimProfile { pubkey, consumer_id })
        }
        "nmp.kernel.release_profile" => {
            let pubkey = str_field(&action.payload, "pubkey")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(ClaimDispatch::ReleaseProfile { pubkey, consumer_id })
        }
        "nmp.kernel.claim_event" => {
            let uri = str_field(&action.payload, "uri")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(ClaimDispatch::ClaimEvent { uri, consumer_id })
        }
        "nmp.kernel.release_event" => {
            let uri = str_field(&action.payload, "uri")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(ClaimDispatch::ReleaseEvent { uri, consumer_id })
        }
        _ => None,
    }
}

/// Extract a string-valued field from a JSON payload. Returns `None` when
/// the payload is not a JSON object, the key is absent, or the value is not
/// a string — all defensively treated as "not a valid claim payload" (D6).
fn str_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Single-source reason string for app-level writes that cannot complete on
/// the **synchronous** wasm runtime path. Distinguishes the two honest
/// failure modes the synchronous `handle()` arm can surface:
///
/// - **No signer installed.** The host hasn't called `SetSigner` yet — the
///   user has not signed in. Banner: "sign in to publish".
/// - **Signer installed but synchronous-path-only.** A signer IS installed
///   and the wasm runtime CAN publish — through the asynchronous
///   `NmpWasmRuntime::dispatch_app_action_async(...)` entrypoint Stage 3c
///   added. The synchronous `handle_json` cannot route the same action
///   because `Nip07Signer::sign()` needs to `await` a JS Promise (`window.
///   nostr.signEvent(...)`) the wasm thread cannot block on. The reason
///   string points the host at the async entrypoint so the integration is
///   self-documenting.
///
/// Both strings start with a stable underscore-snake-case prefix the JS host
/// can pattern-match without parsing the full reason text.
pub(crate) fn write_path_unavailable_reason(signer: Option<&Arc<dyn Signer>>) -> String {
    if signer.is_none() {
        return "signer_not_installed: no signer installed; send WorkerRequest::SetSigner \
                with kind = \"nip07\" and the pubkey from window.nostr.getPublicKey() \
                before dispatching app-level writes."
            .to_string();
    }
    "publish_path_not_wired: a signer is installed but app-level writes \
     cannot be routed through the synchronous `handle_json` path — the \
     NIP-07 sign step requires awaiting `window.nostr.signEvent(...)`, \
     which the wasm thread cannot block on. Use \
     `NmpWasmRuntime.dispatch_app_action_async(requestJson)` (returns a \
     Promise) instead. V-01 Stage 3c wired PublishNote (kind:1) and React \
     (kind:7); Follow / Unfollow follow up."
        .to_string()
}

/// Reason string for non-app-action capability completions that cannot be
/// honored without the native actor. Kept stable so JS hosts can
/// pattern-match the `browser_actor_driver_missing` prefix.
pub(crate) fn browser_driver_missing_reason() -> String {
    "browser_actor_driver_missing: capability completions require the native \
     actor (gated behind feature = \"native\"). The wasm runtime accepts the \
     completion to drain the JS-side pending state but cannot route it into \
     a capability handler."
        .to_string()
}

/// Map a generic `ActionDispatch` to its `KernelAction` if (and only if) the
/// `action_type` is in the kernel namespace. Returns `None` for app-namespaced
/// actions, which the caller surfaces via [`write_path_unavailable_reason`]
/// until Stage 3c wires a publish path.
///
/// Kept narrow on purpose: only the actions whose entire implementation lives
/// in the pure reducer are routed. Anything that needs the actor (signed-event
/// publication, capability dispatch, planner driver) returns `None`.
pub(crate) fn kernel_action_from_dispatch(action: &ActionDispatch) -> Option<KernelAction> {
    match action.action_type.as_str() {
        "nmp.kernel.start" => Some(KernelAction::Start),
        "nmp.kernel.stop" => Some(KernelAction::Stop),
        "nmp.kernel.diagnostics" => Some(KernelAction::RunDiagnostics),
        "nmp.kernel.open_uri" => action
            .payload
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| KernelAction::OpenUri { uri: uri.to_string() }),
        "nmp.kernel.open_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::OpenView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        "nmp.kernel.close_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::CloseView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        _ => None,
    }
}

// ─── F-CR-00 claim arm executor ──────────────────────────────────────────────
//
// Extracted from the former inline arm in `runtime.rs::dispatch()` so the
// routing table and the execution logic stay co-located (dispatch_routing is
// the single owner) and runtime.rs can replace the 37-line block with a
// one-line delegation.
//
// `can_send` mirrors the native `claim_send_gate` semantics: true when any
// relay lane has reported `Connected`. Release calls always return empty vecs.

/// Execute a decoded `ClaimDispatch` against the live kernel, returning any
/// immediately-sendable `Vec<OutboundMessage>` (already `partition_auth_paused`
/// inside the `KernelReducer` methods).
///
/// Claims are not `KernelAction`s — they operate on the claim-refcount table
/// that is separate from the M2 interest registry. `force = false` throughout:
/// web-component claims on mount are background/`.onAppear`-equivalent, not
/// user-navigation force-refreshes (F-TTL lazy path).
pub(crate) fn execute_claim_dispatch(
    reducer: &mut KernelReducer,
    claim: ClaimDispatch,
    can_send: bool,
) -> Vec<OutboundMessage> {
    match claim {
        ClaimDispatch::ClaimProfile { pubkey, consumer_id } => {
            // Web preview: no liveness hint on the JSON action — default to
            // CacheOk (OneShot kind:0 fetch; no tailing sub).
            reducer.claim_profile(
                pubkey,
                consumer_id,
                can_send,
                false,
                nmp_core::ProfileLiveness::CacheOk,
            )
        }
        ClaimDispatch::ReleaseProfile { pubkey, consumer_id } => {
            reducer.release_profile(&pubkey, &consumer_id)
        }
        ClaimDispatch::ClaimEvent { uri, consumer_id } => {
            reducer.claim_event(uri, consumer_id, can_send, false)
        }
        ClaimDispatch::ReleaseEvent { uri, consumer_id } => {
            reducer.release_event(&uri, &consumer_id)
        }
    }
}

// ─── PR-3 feed-verb dispatch ─────────────────────────────────────────────────
//
// Four new verb types that map to the `KernelReducer` feed-verb surface added
// in PR-3. Same D6 parse discipline as `ClaimDispatch`: missing/non-string
// payload fields return `None`; the caller treats `None` as "not this dispatch
// type" and falls through.

/// Decoded feed-subscription verb extracted from an `ActionDispatch` whose
/// `action_type` is in the `nmp.kernel.open_interest` / `close_interest` /
/// `open_contact_feed` / `close_contact_feed` namespace.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InterestDispatch {
    OpenInterest {
        filter_json: String,
        consumer_id: String,
        scope: u32,
    },
    CloseInterest {
        filter_json: String,
        consumer_id: String,
        scope: u32,
    },
    /// `open_contact_feed {kinds:[1]}` — compiles app-declared primary kinds
    /// into the follow-feed acquisition kind set and re-registers the active
    /// account's follow interests.
    OpenContactFeed {
        kinds: std::collections::BTreeSet<u32>,
    },
    /// `close_contact_feed {}` — clears the follow-feed kind set (no relay
    /// CLOSE diff is emitted until `drain_lifecycle_outbound` / `tick`).
    CloseContactFeed,
}

/// Parse an `ActionDispatch` as a feed-subscription verb. Returns `None` if
/// the `action_type` is not a feed-verb namespace or a required payload field
/// is absent / malformed (D6: malformed → `None`, never a panic).
pub(crate) fn interest_dispatch_from_action(action: &ActionDispatch) -> Option<InterestDispatch> {
    match action.action_type.as_str() {
        "nmp.kernel.open_interest" => {
            let filter_json = str_field(&action.payload, "filter_json")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            let scope = action
                .payload
                .get("scope")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            Some(InterestDispatch::OpenInterest { filter_json, consumer_id, scope })
        }
        "nmp.kernel.close_interest" => {
            let filter_json = str_field(&action.payload, "filter_json")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            let scope = action
                .payload
                .get("scope")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            Some(InterestDispatch::CloseInterest { filter_json, consumer_id, scope })
        }
        "nmp.kernel.open_contact_feed" => {
            let primary_kinds: Vec<u32> = action
                .payload
                .get("kinds")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or_else(|| Some(Vec::new()))?;
            let kinds = nmp_nip18::acquisition_kinds_for_primary(primary_kinds);
            Some(InterestDispatch::OpenContactFeed { kinds })
        }
        "nmp.kernel.close_contact_feed" => Some(InterestDispatch::CloseContactFeed),
        _ => None,
    }
}

/// Execute a decoded `InterestDispatch` against the live kernel, returning any
/// immediately-sendable `Vec<OutboundMessage>` (already
/// `partition_auth_paused` inside the `KernelReducer` methods).
pub(crate) fn execute_interest_dispatch(
    reducer: &mut KernelReducer,
    interest: InterestDispatch,
) -> Vec<OutboundMessage> {
    match interest {
        InterestDispatch::OpenInterest { filter_json, consumer_id, scope } => {
            reducer.open_interest(&filter_json, &consumer_id, scope)
        }
        InterestDispatch::CloseInterest { filter_json, consumer_id, scope } => {
            reducer.close_interest(&filter_json, &consumer_id, scope)
        }
        InterestDispatch::OpenContactFeed { kinds } => reducer.set_follow_feed_kinds(kinds),
        InterestDispatch::CloseContactFeed => {
            reducer.set_follow_feed_kinds(std::collections::BTreeSet::new())
        }
    }
}

#[cfg(test)]
#[path = "dispatch_routing_tests.rs"]
mod tests;
