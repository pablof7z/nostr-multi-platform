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
//! 2. [`ref_dispatch_from_action`] / [`execute_ref_dispatch`] — parse and
//!    execute the ADR-0063 unified reference-resolution operations
//!    (`nmp.kernel.resolve_ref` / `nmp.kernel.release_ref`, raw-key seam) plus
//!    the legacy URI front door (`nmp.kernel.claim_event` /
//!    `nmp.kernel.release_event`) via the `KernelReducer`. Both converge on the
//!    SAME `Kernel::resolve_ref` resolver body — there is no divergent web-only
//!    resolution path.
//!
//! 3. [`interest_dispatch_from_action`] / [`execute_interest_dispatch`] —
//!    parse and execute feed-verb operations:
//!    `nmp.kernel.open_interest`, `nmp.kernel.close_interest`,
//!    `nmp.feed.declare_active_follows`, `nmp.feed.clear_active_follows`.
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

use nmp_core::{
    EventShape, KernelAction, KernelReducer, OutboundMessage, ProfileShape, RefLiveness,
    RefNamespace, RefShape,
};
use nmp_signers::Signer;
use serde_json::Value;

use crate::protocol::ActionDispatch;

/// Decoded reference-resolution operation extracted from an `ActionDispatch`.
///
/// The ADR-0063 unified seam (`resolve_ref` / `release_ref`) carries a RAW key
/// (a hex pubkey for `profile`, a hex event-id / `kind:pubkey:d` coordinate for
/// `event`) plus the namespace/shape/liveness discriminants. The legacy URI
/// front door (`claim_event` / `release_event`) carries a `nostr:` URI that the
/// kernel's `claim_event` adapter decodes to a raw key before delegating to the
/// SAME `resolve_event_ref` body — it is the URI adapter of `resolve_ref`, not a
/// separate resolver.
///
/// Refs are NOT `KernelAction`s — they operate on the resolver's refcount table,
/// separate from the M2 interest registry. `kernel_action_from_dispatch` returns
/// `None` for these action types; this function handles them instead.
///
/// D6 — total: a missing/non-string field OR an unknown namespace/shape/liveness
/// discriminant returns `None` (never coerced to a default); the caller treats
/// `None` as "not a ref dispatch" and falls through to the write-path-unavailable
/// path. No panic.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RefDispatch {
    /// Unified raw-key resolve (`nmp.kernel.resolve_ref`).
    Resolve {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
    },
    /// Unified raw-key release (`nmp.kernel.release_ref`).
    Release {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
    },
    /// Legacy URI front door of the event resolver (`nmp.kernel.claim_event`).
    ClaimEventUri { uri: String, consumer_id: String },
    /// Legacy URI release (`nmp.kernel.release_event`).
    ReleaseEventUri { uri: String, consumer_id: String },
}

/// Parse an `ActionDispatch` as a reference-resolution operation. Returns `None`
/// if the `action_type` is not a ref namespace or a required field is absent /
/// malformed (D6: malformed → `None`, never a panic, never a coerced default).
pub(crate) fn ref_dispatch_from_action(action: &ActionDispatch) -> Option<RefDispatch> {
    match action.action_type.as_str() {
        "nmp.kernel.resolve_ref" => {
            let namespace = ref_namespace_from_int(int_field(&action.payload, "namespace")?)?;
            let key = str_field(&action.payload, "key")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            // Decode the shape against the SAME namespace so a (namespace, shape)
            // mismatch fails closed here (rather than relying on the kernel's
            // downstream guard). An unknown shape int returns None.
            let shape = ref_shape_from_int(namespace, int_field(&action.payload, "shape")?)?;
            let liveness = ref_liveness_from_int(int_field(&action.payload, "liveness")?)?;
            Some(RefDispatch::Resolve {
                namespace,
                key,
                consumer_id,
                shape,
                liveness,
            })
        }
        "nmp.kernel.release_ref" => {
            let namespace = ref_namespace_from_int(int_field(&action.payload, "namespace")?)?;
            let key = str_field(&action.payload, "key")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(RefDispatch::Release {
                namespace,
                key,
                consumer_id,
            })
        }
        "nmp.kernel.claim_event" => {
            let uri = str_field(&action.payload, "uri")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(RefDispatch::ClaimEventUri { uri, consumer_id })
        }
        "nmp.kernel.release_event" => {
            let uri = str_field(&action.payload, "uri")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            Some(RefDispatch::ReleaseEventUri { uri, consumer_id })
        }
        _ => None,
    }
}

/// Extract a string-valued field from a JSON payload. Returns `None` when
/// the payload is not a JSON object, the key is absent, or the value is not
/// a string — all defensively treated as "not a valid ref payload" (D6).
fn str_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Extract an unsigned-int discriminant field. Returns `None` for a missing /
/// non-integer / out-of-`u32`-range value (D6 — never a coerced default).
fn int_field(payload: &Value, key: &str) -> Option<u32> {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

/// Decode the namespace discriminant, FAILING CLOSED on an unknown value. The
/// codes mirror the Lane D FFI (`apps/chirp/chirp-tui` runtime.rs): 0 = profile,
/// 1 = event. An unknown value is NOT coerced to a default — it returns `None` so
/// the whole dispatch falls through (D6).
fn ref_namespace_from_int(value: u32) -> Option<RefNamespace> {
    match value {
        0 => Some(RefNamespace::Profile),
        1 => Some(RefNamespace::Event),
        _ => None,
    }
}

/// Decode the shape discriminant against `namespace`, FAILING CLOSED on an
/// unknown value or a namespace/shape mismatch. profile: 0 = ref, 1 = card;
/// event: 0 = embed, 1 = raw.
fn ref_shape_from_int(namespace: RefNamespace, value: u32) -> Option<RefShape> {
    match (namespace, value) {
        (RefNamespace::Profile, 0) => Some(RefShape::Profile(ProfileShape::Ref)),
        (RefNamespace::Profile, 1) => Some(RefShape::Profile(ProfileShape::Card)),
        (RefNamespace::Event, 0) => Some(RefShape::Event(EventShape::Embed)),
        (RefNamespace::Event, 1) => Some(RefShape::Event(EventShape::Raw)),
        _ => None,
    }
}

/// Decode the liveness discriminant, FAILING CLOSED on an unknown value.
/// 0 = CacheOk, 1 = Live.
fn ref_liveness_from_int(value: u32) -> Option<RefLiveness> {
    match value {
        0 => Some(RefLiveness::CacheOk),
        1 => Some(RefLiveness::Live),
        _ => None,
    }
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
            .map(|uri| KernelAction::OpenUri {
                uri: uri.to_string(),
            }),
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

// ─── ADR-0063 reference-resolution arm executor ──────────────────────────────
//
// The routing table and the execution logic stay co-located (dispatch_routing is
// the single owner) so runtime.rs delegates in one line.
//
// `can_send` mirrors the native `claim_send_gate` semantics: true when any relay
// lane has reported `Connected`; it gates only the legacy URI front door (the
// unified `resolve_ref` seam reads `any_relay_connected` inside the kernel —
// origin-blind, no caller flag). Release calls always return empty vecs.

/// Execute a decoded `RefDispatch` against the live kernel, returning any
/// immediately-sendable `Vec<OutboundMessage>` (already `partition_auth_paused`
/// inside the `KernelReducer` methods).
///
/// Refs are not `KernelAction`s — they operate on the resolver refcount table,
/// separate from the M2 interest registry. The two front doors (raw `resolve_ref`
/// seam + legacy `claim_event` URI adapter) converge on the SAME kernel resolver.
/// `force = false`: web-component resolves on mount are background, not
/// user-navigation force-refreshes (F-TTL lazy path).
pub(crate) fn execute_ref_dispatch(
    reducer: &mut KernelReducer,
    dispatch: RefDispatch,
    can_send: bool,
) -> Vec<OutboundMessage> {
    match dispatch {
        RefDispatch::Resolve {
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
        } => reducer.resolve_ref(namespace, key, consumer_id, shape, liveness),
        RefDispatch::Release {
            namespace,
            key,
            consumer_id,
        } => reducer.release_ref(namespace, &key, &consumer_id),
        RefDispatch::ClaimEventUri { uri, consumer_id } => {
            // URI front door of the event resolver: the kernel decodes the URI to
            // a raw key, then delegates to the same `resolve_event_ref` body.
            reducer.claim_event(uri, consumer_id, can_send, false)
        }
        RefDispatch::ReleaseEventUri { uri, consumer_id } => {
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
/// `action_type` is in the `nmp.kernel.open_interest` / `close_interest` or
/// active-follows declared-feed namespace.
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
    /// `nmp.feed.declare_active_follows {primary_kinds:[1]}` — compiles
    /// app-declared primary kinds into the follow-feed acquisition kind set and
    /// re-registers the active account's follow interests.
    DeclareActiveFollowsFeed {
        acquisition_kinds: std::collections::BTreeSet<u32>,
    },
    /// Clears the active-follows declaration (no relay CLOSE diff is emitted
    /// until `drain_lifecycle_outbound` / `tick`).
    ClearActiveFollowsFeed,
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
            Some(InterestDispatch::OpenInterest {
                filter_json,
                consumer_id,
                scope,
            })
        }
        "nmp.kernel.close_interest" => {
            let filter_json = str_field(&action.payload, "filter_json")?;
            let consumer_id = str_field(&action.payload, "consumer_id")?;
            let scope = action
                .payload
                .get("scope")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            Some(InterestDispatch::CloseInterest {
                filter_json,
                consumer_id,
                scope,
            })
        }
        "nmp.feed.declare_active_follows" => {
            let primary_kinds = action
                .payload
                .get("primary_kinds")
                .and_then(Value::as_array)?
                .iter()
                .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
                .collect::<Option<Vec<_>>>()?;
            let acquisition_kinds =
                nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds).ok()?;
            Some(InterestDispatch::DeclareActiveFollowsFeed { acquisition_kinds })
        }
        "nmp.feed.clear_active_follows" => Some(InterestDispatch::ClearActiveFollowsFeed),
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
        InterestDispatch::OpenInterest {
            filter_json,
            consumer_id,
            scope,
        } => reducer.open_interest(&filter_json, &consumer_id, scope),
        InterestDispatch::CloseInterest {
            filter_json,
            consumer_id,
            scope,
        } => reducer.close_interest(&filter_json, &consumer_id, scope),
        InterestDispatch::DeclareActiveFollowsFeed { acquisition_kinds } => {
            reducer.declare_active_follows_feed(acquisition_kinds)
        }
        InterestDispatch::ClearActiveFollowsFeed => reducer.clear_active_follows_feed(),
    }
}

#[cfg(test)]
#[path = "dispatch_routing_tests.rs"]
mod tests;
