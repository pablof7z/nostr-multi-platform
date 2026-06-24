//! Pure helpers around the runtime's dispatch routing surface.
//!
//! Three responsibilities:
//!
//! 1. [`ref_dispatch_from_resolve`] / [`ref_dispatch_from_release`] /
//!    [`execute_ref_dispatch`] — parse and execute the ADR-0063 unified
//!    reference-resolution operations (`resolve_ref` / `release_ref`, raw-key
//!    seam) via the `KernelReducer`.
//!
//! 2. **RETIRED (#1740 step 8):** the raw feed-verb dispatch
//!    (`interest_dispatch_from_action` / `execute_interest_dispatch` and the
//!    `nmp.kernel.open_interest` / `close_interest` +
//!    `nmp.feed.declare_active_follows` / `clear_active_follows` action strings)
//!    is DELETED from this public router. The web app drives the kernel
//!    reducer's feed methods directly through the `WasmRuntime` Rust facade
//!    (internal composition glue); the only public way to open a feed is the
//!    typed `open_feed` doorway (native today — a wasm `nmp.feed.open` awaits
//!    porting the native session registry + perspective compiler to wasm).
//!
//! 3. Stable, host-pattern-matchable reason strings for the no-active-account
//!    state the typed write path can honestly report (`signer_not_installed`)
//!    plus the capability-completion failure reason
//!    (`browser_actor_driver_missing`). The pre-#1008
//!    `publish_not_supported_in_web_preview` disable token is retired —
//!    publish routing is live via `WasmOutboxResolver`.
//!
//! Split out of `runtime.rs` so the file stays under the 500-LOC ceiling and
//! the routing table has a single owner that codegen / kernel-namespace
//! additions touch directly.

use nmp_core::{
    EventShape, KernelReducer, OutboundMessage, ProfileShape, RefLiveness, RefNamespace, RefShape,
};

use crate::protocol::{ReleaseRef, ResolveRef};

/// Decoded reference-resolution operation extracted from a structured control
/// request.
///
/// The ADR-0063 unified seam (`resolve_ref` / `release_ref`) carries a RAW key
/// (a hex pubkey for `profile`, a hex event-id / `kind:pubkey:d` coordinate for
/// `event`) plus the namespace/shape/liveness discriminants. Event callers that
/// decoded a NIP-19/NIP-21 URI before entering this raw-key seam may also pass
/// relay `hints`; absent hints are byte-identical to the bare-key path.
///
/// Refs are NOT `KernelAction`s — they operate on the resolver's refcount table,
/// separate from the M2 interest registry.
///
/// D6 — total: an unknown namespace/shape/liveness discriminant returns `None`
/// (never coerced to a default). No panic.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RefDispatch {
    /// Unified raw-key resolve (`nmp.kernel.resolve_ref`).
    Resolve {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        hints: Vec<String>,
    },
    /// Unified raw-key release (`nmp.kernel.release_ref`).
    Release {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
    },
}

/// Parse a structured `resolve_ref` request. Returns `None` when any numeric
/// discriminant is unknown or mismatched for its namespace (D6: fail closed,
/// never coerce).
pub(crate) fn ref_dispatch_from_resolve(request: &ResolveRef) -> Option<RefDispatch> {
    let namespace = ref_namespace_from_int(request.namespace)?;
    let shape = ref_shape_from_int(namespace, request.shape)?;
    let liveness = ref_liveness_from_int(request.liveness)?;
    Some(RefDispatch::Resolve {
        namespace,
        key: request.key.clone(),
        consumer_id: request.consumer_id.clone(),
        shape,
        liveness,
        hints: request.hints.clone(),
    })
}

/// Parse a structured `release_ref` request. Returns `None` when the namespace
/// discriminant is unknown.
pub(crate) fn ref_dispatch_from_release(request: &ReleaseRef) -> Option<RefDispatch> {
    let namespace = ref_namespace_from_int(request.namespace)?;
    Some(RefDispatch::Release {
        namespace,
        key: request.key.clone(),
        consumer_id: request.consumer_id.clone(),
    })
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

/// Single-source reason string for typed app-level writes attempted before the
/// host has seeded an active account.
pub(crate) fn signer_not_installed_reason() -> String {
    "signer_not_installed: no active account; send WorkerRequest::SetIdentity \
     with kind = \"nip07\" and the pubkey from window.nostr.getPublicKey() \
     before dispatching app-level writes."
        .to_string()
}

/// Reason string for malformed structured ref control messages.
pub(crate) fn invalid_ref_request_reason(capability: &str) -> String {
    format!(
        "invalid_ref_request: {capability} carried an unknown namespace, shape, \
         or liveness discriminant"
    )
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

// ─── ADR-0063 reference-resolution arm executor ──────────────────────────────
//
// The routing table and the execution logic stay co-located (dispatch_routing is
// the single owner) so runtime.rs delegates in one line.
//
// `_can_send` is retained only to keep the executor signature stable for the
// runtime call site while #1946 retires the legacy URI front doors in parallel.
// The structured raw-key `resolve_ref` seam reads relay readiness inside the
// kernel; release calls always return empty vecs.

/// Execute a decoded `RefDispatch` against the live kernel, returning any
/// immediately-sendable `Vec<OutboundMessage>` (already `partition_auth_paused`
/// inside the `KernelReducer` methods).
///
/// Refs are not `KernelAction`s — they operate on the resolver refcount table,
/// separate from the M2 interest registry.
/// `force = false`: web-component resolves on mount are background, not
/// user-navigation force-refreshes (F-TTL lazy path).
pub(crate) fn execute_ref_dispatch(
    reducer: &mut KernelReducer,
    dispatch: RefDispatch,
    _can_send: bool,
) -> Vec<OutboundMessage> {
    match dispatch {
        RefDispatch::Resolve {
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            hints,
        } => reducer.resolve_ref_with_hints(namespace, key, consumer_id, shape, liveness, hints),
        RefDispatch::Release {
            namespace,
            key,
            consumer_id,
        } => reducer.release_ref(namespace, &key, &consumer_id),
    }
}

// #1740 step 8: the raw feed-verb dispatch (`InterestDispatch` /
// `interest_dispatch_from_action` / `execute_interest_dispatch`) is DELETED. The
// public action strings `nmp.kernel.open_interest` / `close_interest` and
// `nmp.feed.declare_active_follows` / `clear_active_follows` are retired — no
// host (JS) reached them, and the only public way to open a feed is the typed
// `open_feed` doorway (native today; a wasm `nmp.feed.open` awaits porting the
// native session registry + perspective compiler — see #1740). The wasm
// reducer's `open_interest` / `declare_active_follows_feed` methods remain as
// INTERNAL composition glue the web app's `WasmRuntime` facade drives directly.

#[cfg(test)]
#[path = "dispatch_routing_tests.rs"]
mod tests;
