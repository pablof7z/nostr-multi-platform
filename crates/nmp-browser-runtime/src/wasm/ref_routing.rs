// Ref-routing helpers are fully used on wasm32; on native they are exercised
// only from `#[cfg(test)]` blocks in this file and transitively from `core.rs`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! ADR-0063 reference-resolution routing helpers for `NmpRuntimeCore` (#2038).
//!
//! Mirrors `nmp-wasm/src/dispatch_routing.rs` (the `RefDispatch` arm only).
//! Defined here so `nmp-browser-runtime` can handle `resolve_ref` /
//! `release_ref` without depending on the `nmp-wasm` ABI crate. The
//! discriminant encoding is identical to the native FFI (namespace 0=profile,
//! 1=event; shape per-namespace; liveness 0=CacheOk, 1=Live).
//!
//! Always-compiled: `NmpRuntimeCore` exercises this logic on native CI.

use nmp_core::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape};

use crate::wasm::protocol::{ReleaseRef, ResolveRef};

/// Decoded reference-resolution operation extracted from a structured control
/// request. D6 — `None` when any discriminant is unknown (fail closed, never
/// coerced to a default).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RefDispatch {
    /// `resolve_ref` — increment refcount + schedule fetch if needed.
    Resolve {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        metadata: RefResolveMetadata,
    },
    /// `release_ref` — decrement refcount.
    Release {
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
    },
}

/// Parse a structured `resolve_ref` request into a [`RefDispatch`].
///
/// Returns `None` when any numeric discriminant is unknown or the
/// namespace/shape combination is invalid (D6: fail closed).
pub(crate) fn ref_dispatch_from_resolve(request: &ResolveRef) -> Option<RefDispatch> {
    let namespace = ref_namespace_from_int(request.namespace)?;
    let shape = ref_shape_from_int(namespace, request.shape)?;
    let liveness = ref_liveness_from_int(request.liveness)?;
    if request
        .event_author
        .as_deref()
        .is_some_and(|a| !is_hex64(a))
    {
        return None;
    }
    Some(RefDispatch::Resolve {
        namespace,
        key: request.key.clone(),
        consumer_id: request.consumer_id.clone(),
        shape,
        liveness,
        metadata: RefResolveMetadata {
            hints: request.hints.clone(),
            event_author: request.event_author.clone(),
        },
    })
}

/// Parse a structured `release_ref` request into a [`RefDispatch`].
///
/// Returns `None` when the namespace discriminant is unknown (D6: fail closed).
pub(crate) fn ref_dispatch_from_release(request: &ReleaseRef) -> Option<RefDispatch> {
    let namespace = ref_namespace_from_int(request.namespace)?;
    Some(RefDispatch::Release {
        namespace,
        key: request.key.clone(),
        consumer_id: request.consumer_id.clone(),
    })
}

// ── Discriminant decoders (fail closed) ──────────────────────────────────────

fn ref_namespace_from_int(value: u32) -> Option<RefNamespace> {
    match value {
        0 => Some(RefNamespace::Profile),
        1 => Some(RefNamespace::Event),
        _ => None,
    }
}

fn ref_shape_from_int(namespace: RefNamespace, value: u32) -> Option<RefShape> {
    match (namespace, value) {
        (RefNamespace::Profile, 0) => Some(RefShape::Profile(ProfileShape::Ref)),
        (RefNamespace::Profile, 1) => Some(RefShape::Profile(ProfileShape::Card)),
        (RefNamespace::Event, 0) => Some(RefShape::Event(EventShape::Embed)),
        (RefNamespace::Event, 1) => Some(RefShape::Event(EventShape::Raw)),
        _ => None,
    }
}

fn ref_liveness_from_int(value: u32) -> Option<RefLiveness> {
    match value {
        0 => Some(RefLiveness::CacheOk),
        1 => Some(RefLiveness::Live),
        _ => None,
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Stable reason strings for fail-closed ref-request rejection.
pub(crate) fn invalid_ref_request_reason(capability: &str) -> String {
    format!(
        "invalid_ref_request: {capability} carried an unknown namespace, shape, \
         or liveness discriminant"
    )
}

/// Stable reason string when a write is attempted before `SetIdentity`.
pub(crate) fn signer_not_installed_reason() -> String {
    "signer_not_installed: no active account; send WorkerRequest::SetIdentity \
     with kind = \"nip07\" before dispatching app-level writes."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::protocol::{ReleaseRef, ResolveRef};

    #[test]
    fn resolve_profile_ref_shape() {
        let req = ResolveRef {
            namespace: 0,
            key: "aabbcc".to_string(),
            consumer_id: "c1".to_string(),
            shape: 0,
            liveness: 0,
            hints: vec![],
            event_author: None,
            correlation_id: "cid".to_string(),
        };
        let dispatch = ref_dispatch_from_resolve(&req).expect("must parse");
        assert!(matches!(
            dispatch,
            RefDispatch::Resolve {
                namespace: RefNamespace::Profile,
                shape: RefShape::Profile(ProfileShape::Ref),
                liveness: RefLiveness::CacheOk,
                ..
            }
        ));
    }

    #[test]
    fn unknown_namespace_returns_none() {
        let req = ResolveRef {
            namespace: 99,
            key: "k".to_string(),
            consumer_id: "c".to_string(),
            shape: 0,
            liveness: 0,
            hints: vec![],
            event_author: None,
            correlation_id: "cid".to_string(),
        };
        assert!(ref_dispatch_from_resolve(&req).is_none());
    }

    #[test]
    fn release_ref_parses_correctly() {
        let req = ReleaseRef {
            namespace: 1,
            key: "evid".to_string(),
            consumer_id: "c2".to_string(),
            correlation_id: "cid2".to_string(),
        };
        let dispatch = ref_dispatch_from_release(&req).expect("must parse");
        assert!(matches!(
            dispatch,
            RefDispatch::Release {
                namespace: RefNamespace::Event,
                ..
            }
        ));
    }
}
