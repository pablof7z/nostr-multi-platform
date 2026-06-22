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

/// Mirror of the interest-inspection helper in `refs_tests_event.rs` — collect
/// every active registry interest that addresses the addressable coordinate
/// `(kind, author, d_tag)`.  Used in `assert_coord_key_accepted` to prove that
/// the resolver registered the interest with the correct fields end-to-end.
impl Kernel {
    fn coord_interest_shapes_for_test(
        &self,
        kind: u32,
        author: &str,
        d_tag: &str,
    ) -> Vec<crate::planner::InterestShape> {
        self.lifecycle
            .registry()
            .iter_active()
            .into_iter()
            .filter(|i| {
                i.shape.kinds.contains(&kind)
                    && i.shape.authors.contains(author)
                    && i.shape.tags.get("d").is_some_and(|v| v.contains(d_tag))
            })
            .map(|i| i.shape.clone())
            .collect()
    }
}

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
fn assert_coord_key_accepted(
    key: &str,
    expected_kind: u32,
    expected_pubkey: &str,
    expected_d: &str,
) {
    // Parse-level assertion: the key must round-trip to the expected coord fields.
    let target = parse_event_key(key).unwrap_or_else(|| {
        panic!("coord key {key:?} was rejected by parse_event_key — expected acceptance")
    });
    let (kind, pubkey, d_tag) = target
        .replaceable_coord
        .expect("coord key must produce a replaceable_coord, not None");
    assert_eq!(kind, expected_kind, "coord key {key:?}: kind mismatch");
    assert_eq!(
        pubkey, expected_pubkey,
        "coord key {key:?}: pubkey mismatch"
    );
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

    // Resolver-level assertion: the registry must hold exactly ONE active
    // interest whose shape encodes the expected coordinate fields.  This
    // proves the interest is correct end-to-end, not merely that a claim
    // row exists in `event_claims`.
    let shapes = kernel.coord_interest_shapes_for_test(expected_kind, expected_pubkey, expected_d);
    assert_eq!(
        shapes.len(),
        1,
        "coord key {key:?}: expected exactly 1 active interest for \
         kinds={{{}}} authors={{{}}} #d={{{}}} — got {}",
        expected_kind,
        expected_pubkey,
        expected_d,
        shapes.len(),
    );
    let shape = &shapes[0];
    assert!(
        shape.kinds.contains(&expected_kind),
        "coord key {key:?}: interest kinds {:?} does not contain expected kind {}",
        shape.kinds,
        expected_kind,
    );
    assert!(
        shape.authors.contains(expected_pubkey),
        "coord key {key:?}: interest authors {:?} does not contain expected pubkey {}",
        shape.authors,
        expected_pubkey,
    );
    assert!(
        shape.tags.get("d").is_some_and(|v| v.contains(expected_d)),
        "coord key {key:?}: interest #d tag {:?} does not contain expected d {:?}",
        shape.tags.get("d"),
        expected_d,
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

// ─── #1654: NIP-73 external-ref keys (`i:<external-id>`) ──────────────────────

#[test]
fn external_ref_key_resolves_with_i_tag_filter() {
    // #1654: an `i:<external-id>` key (NIP-73) must parse to an EventTarget whose
    // wire filter carries the `#i` tag with the verbatim external id, NO
    // replaceable coord (external refs are never addressable-replaceable), and
    // record a claim row. Proven-red: before the `i:` arm in `parse_event_key`,
    // the key fell through to the coordinate split (`splitn(3,':')` → kind="i",
    // rejected by `.parse::<u32>()`) and FAILED CLOSED — no claim row, no filter.
    let external_id = "podcast:item:guid:e1d2c3b4-0000-0000-0000-aaaabbbbcccc";
    let key = format!("i:{external_id}");

    let target = parse_event_key(&key)
        .unwrap_or_else(|| panic!("external-ref key {key:?} must parse, got None"));
    assert_eq!(
        target.primary_id, key,
        "the projection key is the FULL `i:<external-id>` form (renderer round-trip)"
    );
    assert!(
        target.replaceable_coord.is_none(),
        "an external ref is never replaceable — no coord"
    );
    assert!(target.author.is_none(), "an external ref carries no author");
    let i_tag = target
        .filter
        .tags
        .get("i")
        .expect("the wire filter must carry an `#i` tag dimension");
    assert!(
        i_tag.contains(external_id),
        "the `#i` filter must match the verbatim external id {external_id:?}, got {i_tag:?}"
    );
    assert_eq!(target.filter.limit, Some(1), "a ref fetch is limit:1");
    assert!(
        target.filter.event_ids.is_empty() && target.filter.kinds.is_empty(),
        "an external-ref filter matches by `#i` tag only — no id/kind narrowing"
    );

    // Kernel-level: resolve_ref records a claim row keyed by the full `i:` form.
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
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
        "external-ref key {key:?}: expected a claim row but event_claims is empty"
    );
    // An external ref is never replaceable, so a `CacheOk` request must NOT
    // register a tailing slot.
    assert!(
        kernel.live_event_claims.is_empty(),
        "external-ref CacheOk claim registers no live tailing slot"
    );
}

#[test]
fn external_ref_live_never_tails() {
    // #1654: `Live` on an external ref must degrade to one-shot (no tailing slot),
    // exactly like an immutable event-id — external refs are not replaceable, so
    // there is no newer-replacement to tail. Proven-red guard against a future
    // change that wires external refs into the addressable Live path.
    let key = "i:isbn:9780375704024".to_string();
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert!(
        kernel.event_claims.contains_key(&key),
        "external-ref Live claim still records a refcount row"
    );
    assert!(
        kernel.live_event_claims.is_empty(),
        "external-ref Live claim must NOT register a tailing slot (not replaceable)"
    );
}

#[test]
fn malformed_external_ref_empty_id_fails_closed() {
    // #1654: a bare `i:` (present prefix, EMPTY external id) must fail closed —
    // an empty `#i` value would match nothing and pollute the registry. Proven-red:
    // without the `is_valid_external_id` empty check the `i:` arm would build a
    // `{"#i":[""], limit:1}` filter and record a bogus claim.
    assert_event_key_fails_closed("i:");
}

#[test]
fn malformed_external_ref_whitespace_id_fails_closed() {
    // #1654: external ids carrying ASCII control / space bytes can never appear in
    // a wire `i`-tag value (the tag is whitespace-free); reject them fail-closed.
    assert_event_key_fails_closed("i:podcast item guid");
    assert_event_key_fails_closed("i:has\ttab");
    assert_event_key_fails_closed("i:has\nnewline");
}

#[test]
fn malformed_external_ref_unknown_scheme_fails_closed() {
    // codex lead-gate HIGH 1: an `i:<value>` whose scheme is NOT one of the NIP-73
    // external-id forms must FAIL CLOSED — no `#i` REQ for an arbitrary/unknown id,
    // no fabricated preview. Proven-red: with the pre-fix `is_valid_external_id`
    // (non-empty + whitespace-free only, NO scheme allowlist) every one of these
    // parsed to a `{"#i":[…], limit:1}` filter and recorded a bogus claim row.
    assert_event_key_fails_closed("i:unknown:whatever");
    assert_event_key_fails_closed("i:ftp://example.com/file"); // not http/https
    assert_event_key_fails_closed("i:mailto:nobody@example.com");
    assert_event_key_fails_closed("i:javascript:alert(1)");
    assert_event_key_fails_closed("i:bareword"); // no scheme prefix, not a URL
    assert_event_key_fails_closed("i:isbn:"); // known prefix, EMPTY value
    assert_event_key_fails_closed("i:#"); // hashtag prefix, empty topic
}

#[test]
fn malformed_blockchain_external_ref_fails_closed() {
    // FIX #1 (codex re-gate): the generic `<blockchain>[:<chainId>]:{tx,address}:`
    // form must still fail closed when the SELECTOR is missing/garbage or the value
    // is empty — generalising the chain token must not weaken the selector/value
    // checks. Proven-red: with the blockchain arm hardcoded to bitcoin/ethereum the
    // generic positives (solana) were rejected; these negatives stay rejected.
    // bad middle segment (not tx/address, and no further selector):
    assert_event_key_fails_closed("i:bitcoin:nonsense:deadbeef");
    // chainId ok, bad selector:
    assert_event_key_fails_closed("i:ethereum:1:badselector:0xabc");
    // selector ok, EMPTY value:
    assert_event_key_fails_closed("i:bitcoin:tx:");
    // selector ok, value segment MISSING entirely:
    assert_event_key_fails_closed("i:bitcoin:tx");
    // uppercase blockchain token (must be lowercase alnum):
    assert_event_key_fails_closed("i:Bitcoin:tx:abc");
    // chainId + selector ok, EMPTY value:
    assert_event_key_fails_closed("i:ethereum:1:address:");
}

#[test]
fn well_formed_external_ref_known_schemes_resolve() {
    // codex lead-gate HIGH 1 complement: the canonical NIP-73 external-id schemes
    // MUST still parse — the fail-closed allowlist is not vacuously rejecting every
    // external ref. One representative per NIP-73 scheme family.
    for external_id in [
        "https://blog.example.com/post/hello",
        "http://example.com/x",
        "#bitcoin",
        "isbn:9780765382030",
        "geo:ezs42e44yx96",
        "iso3166:US-CA",
        "isan:0000-0000-401A-0000-7",
        "doi:10.1000/xyz123",
        "podcast:guid:c90e609a-df1e-596a-bd5e-57bcc8aad6cc",
        "podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f",
        "podcast:publisher:guid:18bcbf10-6701-4ffb-b255-bc057390d738",
        "bitcoin:address:1HQ3Go3ggs8pFnXuHVHRytPCq5fGG8Hbhx", // blockchain, no chainId
        "bitcoin:tx:a1075db55d416d3ca199f55b6084e2115b9345e16c5cf302fc80e9d5fbf5d48d",
        "ethereum:1:address:0xd8da6bf26964af9d7eed9e03e53415d37aa96045", // blockchain WITH chainId
        "ethereum:1:tx:0xabc123",
        "solana:tx:5j7s8...signature", // FIX #1: generic blockchain — solana resolves with no code change
        "solana:address:7nYa...mintAddr",
        "ethereum:11155111:tx:0xfeed", // multi-digit (non-mainnet) chainId
    ] {
        let key = format!("i:{external_id}");
        let target = parse_event_key(&key).unwrap_or_else(|| {
            panic!("known NIP-73 scheme {key:?} was rejected by parse_event_key")
        });
        assert!(
            target.replaceable_coord.is_none(),
            "external ref {key:?} is never replaceable"
        );
        assert!(
            target
                .filter
                .tags
                .get("i")
                .is_some_and(|v| v.contains(external_id)),
            "external ref {key:?} must build an `#i` filter for the verbatim id"
        );
    }
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
