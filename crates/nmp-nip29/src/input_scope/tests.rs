//! Tests for the NIP-29 group input-scope recognizer.

use nmp_core::substrate::{
    InputIntentTarget, InputScopeId, InputScopeRecognizer, ResolvedInput, ResolvedInputKind,
    TextSearchTargets,
};

use crate::input_scope::{GroupIdentPayload, GroupInputScopeRecognizer, GROUP_INPUT_SCOPE_LABEL};

fn recognizer() -> GroupInputScopeRecognizer {
    GroupInputScopeRecognizer::new()
}

/// Helper: make a `FreeText` `ResolvedInput`.
fn free_text(s: &str) -> ResolvedInput {
    ResolvedInput {
        raw: s.to_string(),
        kind: ResolvedInputKind::FreeText {
            text: s.to_string(),
        },
    }
}

/// Helper: make a `Reference` `ResolvedInput` with `entity_class`.
fn reference_input(uri: &str, entity_class: &str) -> ResolvedInput {
    ResolvedInput {
        raw: uri.to_string(),
        kind: ResolvedInputKind::Reference {
            uri: uri.to_string(),
            entity_class: entity_class.to_string(),
        },
    }
}

// ── scope identity ────────────────────────────────────────────────────────────

#[test]
fn scope_id_matches_label() {
    let r = recognizer();
    let scope = r.scope();
    assert_eq!(scope, InputScopeId::new("nip29", "groups"));
    assert_eq!(scope.label(), GROUP_INPUT_SCOPE_LABEL);
}

// ── NIP-29 URI form ───────────────────────────────────────────────────────────

#[test]
fn recognize_nip29_uri_returns_registered_with_payload() {
    let r = recognizer();
    let input = free_text("groups.nostr.com'abc-123");
    let result = r.recognize(&input);
    let Some(InputIntentTarget::Registered { payload_json }) = result else {
        panic!("expected Registered, got {result:?}");
    };
    let payload: GroupIdentPayload =
        serde_json::from_str(&payload_json).expect("payload must be valid JSON");
    assert_eq!(payload.host_relay_url, "wss://groups.nostr.com");
    assert_eq!(payload.local_id, "abc-123");
}

#[test]
fn recognize_nip29_uri_underscore_local_id() {
    let r = recognizer();
    let input = free_text("relay.example.org'my_room");
    let result = r.recognize(&input);
    let Some(InputIntentTarget::Registered { payload_json }) = result else {
        panic!("expected Registered, got {result:?}");
    };
    let payload: GroupIdentPayload = serde_json::from_str(&payload_json).unwrap();
    assert_eq!(payload.local_id, "my_room");
}

#[test]
fn recognize_nip29_uri_with_wss_prefix_roundtrips() {
    // The URI form strips the wss:// prefix in to_uri(); from_uri() adds it
    // back. Verify this round-trip is handled correctly in the recognizer path.
    let r = recognizer();
    // from_uri accepts bare host form (no scheme)
    let input = free_text("groups.nostr.com'testroom");
    let Some(InputIntentTarget::Registered { payload_json }) = r.recognize(&input) else {
        panic!("expected Registered");
    };
    let payload: GroupIdentPayload = serde_json::from_str(&payload_json).unwrap();
    assert_eq!(payload.host_relay_url, "wss://groups.nostr.com");
}

#[test]
fn recognize_rejects_invalid_nip29_uri_uppercase_local() {
    let r = recognizer();
    // NIP-29 local id charset is [a-z0-9-_]; uppercase is invalid.
    let input = free_text("groups.nostr.com'ABC");
    assert_eq!(r.recognize(&input), None);
}

#[test]
fn recognize_rejects_plain_text_without_tick() {
    let r = recognizer();
    let input = free_text("hello world");
    assert_eq!(r.recognize(&input), None);
}

#[test]
fn recognize_rejects_empty_local_id() {
    let r = recognizer();
    let input = free_text("groups.nostr.com'");
    assert_eq!(r.recognize(&input), None);
}

// ── naddr reference ───────────────────────────────────────────────────────────

#[test]
fn recognize_naddr_reference_returns_direct_ref() {
    let r = recognizer();
    let uri = "nostr:naddr1abc123";
    let input = reference_input(uri, "address");
    let result = r.recognize(&input);
    assert_eq!(
        result,
        Some(InputIntentTarget::DirectRef {
            uri: uri.to_string()
        })
    );
}

#[test]
fn recognize_profile_reference_is_ignored() {
    let r = recognizer();
    // A profile reference belongs to a profile scope, not nip29.groups.
    let input = reference_input("nostr:npub1abc", "profile");
    assert_eq!(r.recognize(&input), None);
}

#[test]
fn recognize_event_reference_is_ignored() {
    let r = recognizer();
    let input = reference_input("nostr:nevent1abc", "event");
    assert_eq!(r.recognize(&input), None);
}

// ── relay URL and NIP-05 shape pass through ──────────────────────────────────

#[test]
fn recognize_relay_url_is_ignored() {
    let r = recognizer();
    let input = ResolvedInput {
        raw: "wss://relay.example.com".to_string(),
        kind: ResolvedInputKind::RelayUrl {
            url: "wss://relay.example.com".to_string(),
        },
    };
    assert_eq!(r.recognize(&input), None);
}

#[test]
fn recognize_nip05_shape_is_ignored() {
    let r = recognizer();
    let input = ResolvedInput {
        raw: "alice@example.com".to_string(),
        kind: ResolvedInputKind::Nip05Shape {
            identifier: "alice@example.com".to_string(),
        },
    };
    assert_eq!(r.recognize(&input), None);
}

// ── text_candidate always returns None ───────────────────────────────────────

#[test]
fn text_candidate_always_none() {
    let r = recognizer();
    // Groups are not free-text searched through the input-scope path.
    assert_eq!(
        r.text_candidate("nostr groups", &TextSearchTargets::AppDefault),
        None
    );
    assert_eq!(
        r.text_candidate("", &TextSearchTargets::UserPreferred),
        None
    );
}

// ── registration smoke ────────────────────────────────────────────────────────

#[test]
fn register_input_scopes_installs_exactly_one_recognizer() {
    use nmp_core::substrate::InputScopeRegistry;

    let registry = InputScopeRegistry::new();
    crate::input_scope::register_input_scopes(&registry);
    assert_eq!(registry.len(), 1);

    // Duplicate call must yield (ADR-0069 — first wins).
    crate::input_scope::register_input_scopes(&registry);
    assert_eq!(registry.len(), 1, "duplicate registration must yield");
}

// ── payload JSON schema stability ────────────────────────────────────────────

#[test]
fn payload_json_has_stable_field_names() {
    let payload = GroupIdentPayload {
        host_relay_url: "wss://g.example.com".to_string(),
        local_id: "room1".to_string(),
    };
    let json = serde_json::to_string(&payload).unwrap();
    // Dispatch layer deserializes this; field names must be stable.
    assert!(json.contains("\"host_relay_url\""));
    assert!(json.contains("\"local_id\""));
    assert!(json.contains("wss://g.example.com"));
    assert!(json.contains("room1"));
}
