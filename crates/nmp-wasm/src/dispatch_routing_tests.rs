//! Tests for `dispatch_routing` — ADR-0063 reference-resolution parsing and the
//! stable reason strings.
//!
//! Moved out of `dispatch_routing.rs` (NMP #169-style file-size split) to
//! keep the module root under the 500-LOC hard ceiling.

use super::*;

#[test]
fn ref_dispatch_routes_resolve_profile() {
    let request = ResolveRef {
        namespace: 0,
        key: "abc123".to_string(),
        consumer_id: "chirp-web-author-1".to_string(),
        shape: 0,
        liveness: 0,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_resolve(&request),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "chirp-web-author-1".to_string(),
            shape: RefShape::Profile(ProfileShape::Ref),
            liveness: RefLiveness::CacheOk,
            metadata: RefResolveMetadata::default(),
        })
    );
}

#[test]
fn ref_dispatch_routes_resolve_profile_card_live() {
    let request = ResolveRef {
        namespace: 0,
        key: "abc123".to_string(),
        consumer_id: "screen".to_string(),
        shape: 1,
        liveness: 1,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_resolve(&request),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "screen".to_string(),
            shape: RefShape::Profile(ProfileShape::Card),
            liveness: RefLiveness::Live,
            metadata: RefResolveMetadata::default(),
        })
    );
}

#[test]
fn ref_dispatch_routes_resolve_event() {
    let request = ResolveRef {
        namespace: 1,
        key: "deadbeef".to_string(),
        consumer_id: "embed-1".to_string(),
        shape: 0,
        liveness: 0,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_resolve(&request),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Event,
            key: "deadbeef".to_string(),
            consumer_id: "embed-1".to_string(),
            shape: RefShape::Event(EventShape::Embed),
            liveness: RefLiveness::CacheOk,
            metadata: RefResolveMetadata::default(),
        })
    );
}

#[test]
fn ref_dispatch_routes_resolve_event_with_hints() {
    let request = ResolveRef {
        namespace: 1,
        key: "deadbeef".to_string(),
        consumer_id: "embed-1".to_string(),
        shape: 0,
        liveness: 0,
        hints: vec![
            "wss://relay.a.example".to_string(),
            "wss://relay.b.example".to_string(),
        ],
        event_author: Some("ab".repeat(32)),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_resolve(&request),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Event,
            key: "deadbeef".to_string(),
            consumer_id: "embed-1".to_string(),
            shape: RefShape::Event(EventShape::Embed),
            liveness: RefLiveness::CacheOk,
            metadata: RefResolveMetadata {
                hints: vec![
                    "wss://relay.a.example".to_string(),
                    "wss://relay.b.example".to_string(),
                ],
                event_author: Some("ab".repeat(32)),
            },
        })
    );
}

#[test]
fn ref_dispatch_fails_closed_on_invalid_event_author() {
    let request = ResolveRef {
        namespace: 1,
        key: "deadbeef".to_string(),
        consumer_id: "embed-1".to_string(),
        shape: 0,
        liveness: 0,
        hints: Vec::new(),
        event_author: Some("not-a-hex-pubkey".to_string()),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_resolve(&request).is_none());
}

#[test]
fn ref_dispatch_routes_release_ref() {
    let request = ReleaseRef {
        namespace: 0,
        key: "abc123".to_string(),
        consumer_id: "c".to_string(),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_release(&request),
        Some(RefDispatch::Release {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "c".to_string(),
        })
    );
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_namespace_discriminant() {
    // namespace = 99 is neither Profile (0) nor Event (1). It MUST reject, NOT
    // coerce to a default namespace (D6, fail-closed).
    let request = ResolveRef {
        namespace: 99,
        key: "abc".to_string(),
        consumer_id: "c".to_string(),
        shape: 0,
        liveness: 0,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_resolve(&request).is_none());
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_shape_discriminant() {
    // shape = 7 is not a valid profile shape (0 = ref, 1 = card). Reject,
    // never coerce to a default shape (D6).
    let request = ResolveRef {
        namespace: 0,
        key: "abc".to_string(),
        consumer_id: "c".to_string(),
        shape: 7,
        liveness: 0,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_resolve(&request).is_none());
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_liveness_discriminant() {
    // liveness = 5 is neither CacheOk (0) nor Live (1). Reject (D6).
    let request = ResolveRef {
        namespace: 0,
        key: "abc".to_string(),
        consumer_id: "c".to_string(),
        shape: 0,
        liveness: 5,
        hints: Vec::new(),
        event_author: None,
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_resolve(&request).is_none());
}

#[test]
fn release_ref_fails_closed_on_unknown_namespace_discriminant() {
    let request = ReleaseRef {
        namespace: 99,
        key: "abc".to_string(),
        consumer_id: "c".to_string(),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_release(&request).is_none());
}

#[test]
fn signer_not_installed_reason_is_stable() {
    let reason = signer_not_installed_reason();
    assert!(
        reason.starts_with("signer_not_installed"),
        "typed writes before SetIdentity must emit signer_not_installed; got: {reason}"
    );
    assert!(
        !reason.starts_with("publish_not_supported_in_web_preview"),
        "legacy publish_not_supported_in_web_preview token must be gone; got: {reason}"
    );
    assert!(
        !reason.starts_with("publish_path_not_wired"),
        "legacy publish_path_not_wired token must be gone; got: {reason}"
    );
}
