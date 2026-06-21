//! ADR-0063 (#1671 Lane D) — raw event-key parse coverage for the `resolve_ref`
//! Event seam. The FFI/JNI contract documents the event `key` as a raw 64-char
//! LOWERCASE hex event-id OR a `kind:pubkey:d` coordinate — NOT a `nostr:`/NIP-21
//! URI. The original Lane D bug parsed the key as a URI, so a host passing the
//! documented raw key hit a swallowed parse error (D6) and blank-avatared. These
//! tests assert the canonical raw keys resolve and that every malformed shape
//! fails closed (no claim, no discovery REQ, no live slot, no parking, no panic).
//! Split into its own file to keep each test file under the 500-LOC ceiling.

use super::refs::{EventShape, RefLiveness, RefNamespace, RefShape};
use super::requests::parse_event_key;
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// The canonical raw `kind:pubkey:d` coordinate key (ADR-0063 / FFI contract).
fn coord_key(kind: u32, author: &str, d_tag: &str) -> String {
    format!("{kind}:{author}:{d_tag}")
}

/// Resolve a malformed raw Event key and assert it failed closed: no refcount
/// row, no in-flight discovery REQ, no live tailing slot — and (implicitly) no
/// panic. This is the #1671 Lane D coverage hole: a host passing the documented
/// raw key must resolve, and a malformed key must silently no-op rather than
/// blank-avatar through a swallowed URI parse error.
fn assert_event_key_fails_closed(key: &str) {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content); // can_send = true: not a parking no-op
    let out = kernel.resolve_ref(
        RefNamespace::Event,
        key.to_string(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(out.is_empty(), "malformed key {key:?}: no outbound");
    assert!(
        kernel.event_claims.is_empty(),
        "malformed key {key:?}: records no claim row"
    );
    assert!(
        kernel.event_claim_requested.is_empty(),
        "malformed key {key:?}: registers no discovery interest"
    );
    assert!(
        kernel.pending_discovery_oneshots.is_empty(),
        "malformed key {key:?}: fires no discovery REQ"
    );
    assert!(
        kernel.live_event_claims.is_empty(),
        "malformed key {key:?}: registers no live tailing slot"
    );
    assert!(
        kernel.pending_event_claims.is_empty(),
        "malformed key {key:?}: parks nothing"
    );

    // A release of the same malformed key is also a clean no-op (no panic, no row).
    let out = kernel.release_ref(RefNamespace::Event, key, "view");
    assert!(out.is_empty(), "malformed key {key:?}: release no-op");
    assert!(kernel.event_claims.is_empty());
}

#[test]
fn malformed_event_key_bad_hex_length_fails_closed() {
    // 63 hex chars — not a 64-hex id, and no `:` so not a coordinate.
    assert_event_key_fails_closed(&"a".repeat(63));
    // 65 hex chars.
    assert_event_key_fails_closed(&"a".repeat(65));
}

#[test]
fn malformed_event_key_uppercase_hex_fails_closed() {
    // A 64-char UPPERCASE hex id — the raw-key contract is lowercase-only, so
    // this must NOT be accepted as an event-id (the bug `is_hex_pubkey` would
    // have masked). It also is not a valid coordinate (no `:`).
    assert_event_key_fails_closed(&"A".repeat(64));
    // Mixed case is equally rejected.
    let mut mixed = "a".repeat(63);
    mixed.push('B');
    assert_event_key_fails_closed(&mixed);
}

#[test]
fn malformed_event_key_missing_coord_segment_fails_closed() {
    let author = hex64("a47e");
    // Only `kind:pubkey` — the `d` segment is missing entirely.
    assert_event_key_fails_closed(&format!("30023:{author}"));
    // A bare kind with nothing else.
    assert_event_key_fails_closed("30023");
}

#[test]
fn malformed_event_key_non_decimal_kind_fails_closed() {
    let author = hex64("a47f");
    // Non-numeric kind.
    assert_event_key_fails_closed(&format!("kind:{author}:doc"));
    // Hex-looking but non-decimal kind.
    assert_event_key_fails_closed(&format!("0x30023:{author}:doc"));
    // Non-canonical leading-zero kind must not round-trip to the primary_id.
    assert_event_key_fails_closed(&format!("030023:{author}:doc"));
}

#[test]
fn malformed_event_key_bad_pubkey_fails_closed() {
    // Pubkey too short.
    assert_event_key_fails_closed("30023:abc:doc");
    // Pubkey UPPERCASE (lowercase-only contract).
    assert_event_key_fails_closed(&format!("30023:{}:doc", "A".repeat(64)));
    // Pubkey non-hex.
    assert_event_key_fails_closed(&format!("30023:{}:doc", "z".repeat(64)));
}

#[test]
fn well_formed_event_id_key_resolves() {
    // Control: a canonical lowercase-64-hex id IS accepted (the documented raw
    // key the host passes), so the malformed-key guard above is not vacuously
    // rejecting everything.
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let id = hex64("c0ffee");
    kernel.resolve_ref(
        RefNamespace::Event,
        id.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.event_claims.contains_key(&id),
        "a well-formed lowercase-64-hex id records a claim row"
    );
    assert!(
        kernel.event_claim_requested.contains(&id),
        "a well-formed id with no cached event registers a discovery interest"
    );
}

#[test]
fn well_formed_coord_key_resolves() {
    // Control: a canonical `kind:pubkey:d` coordinate IS accepted.
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let author = hex64("a570");
    let key = coord_key(30023, &author, "my-doc");
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.event_claims.contains_key(&key),
        "a well-formed coordinate records a claim row keyed by the coord string"
    );
}

/// Resolve a well-formed raw Event key and assert it succeeded: a claim row
/// exists for `key`, and `parse_event_key` returns the expected replaceable
/// coord `(kind, pubkey, d_tag)`. The complement of `assert_event_key_fails_closed`.
fn assert_coord_key_accepted(key: &str, expected_kind: u32, expected_pubkey: &str, expected_d: &str) {
    // Parse-level assertion: the key must round-trip to the expected coord fields.
    let target = parse_event_key(key).unwrap_or_else(|| {
        panic!("coord key {key:?} was rejected by parse_event_key — expected acceptance")
    });
    let (kind, pubkey, d_tag) = target
        .replaceable_coord
        .expect("coord key must produce a replaceable_coord, not None");
    assert_eq!(kind, expected_kind, "coord key {key:?}: kind mismatch");
    assert_eq!(pubkey, expected_pubkey, "coord key {key:?}: pubkey mismatch");
    assert_eq!(d_tag, expected_d, "coord key {key:?}: d_tag mismatch");

    // Kernel-level assertion: resolve_ref must record a claim row (no fail-closed).
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.to_string(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.event_claims.contains_key(key),
        "coord key {key:?}: expected a claim row but event_claims is empty"
    );
}

#[test]
fn event_key_empty_d_segment_is_accepted() {
    // NIP-01 §"Replaceable events": the d-tag identifier of an addressable event
    // MAY be an empty string — `""` is a legal and distinct identity from any
    // non-empty d-tag.  A `kind:pubkey:` key (trailing colon, empty d segment)
    // must therefore be ACCEPTED, not failed-closed.
    //
    // Precedent: event_live.rs (`event_already_known`) and views.rs use the
    // identical `splitn(3, ':')` with a possibly-empty d_tag and call
    // `get_param_replaceable(pubkey, kind, d_tag)` without rejecting the empty
    // case.  Rejecting it here would diverge from the rest of the kernel.
    let author = hex64("a47e");
    let key = coord_key(30023, &author, "");
    assert_coord_key_accepted(&key, 30023, &author, "");
}

#[test]
fn event_key_d_tag_with_colons_is_accepted() {
    // NIP-01 §"Replaceable events": the d-tag value is an arbitrary string and
    // MAY itself contain colons (e.g. namespaced slugs like "doc:section:2").
    // `parse_event_key` uses `splitn(3, ':')` — splitting on only the FIRST TWO
    // colons — so everything after the second colon is preserved verbatim as the
    // d segment.  This matches the comment in event_live.rs: "d-tags can legally
    // contain `:` (rare but spec-allowed); split only on the first two colons".
    // A key like `30023:<pubkey>:doc:section:2` must resolve with d="doc:section:2".
    let author = hex64("b571");
    let key = coord_key(30023, &author, "doc:section:2");
    assert_coord_key_accepted(&key, 30023, &author, "doc:section:2");
}
