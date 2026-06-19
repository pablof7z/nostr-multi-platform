//! Tests for [`super`] — the kind:9735 zap-receipt decoder.
//!
//! Split out of `decode.rs` to keep that file under the hard LOC cap; wired
//! back in via `#[path = "decode_tests.rs"] mod tests;` (the same pattern
//! `wire::typed_fb` uses). `use super::*` still resolves to the `decode`
//! module, so the tests reach the private `decode_borrowed` /
//! `amount_from_embedded_request` helpers unchanged.

use super::*;
use nmp_core::store::{RawEvent, StoredEvent};
use std::sync::Arc;

fn make_stored(kind: u32, tags: Vec<Vec<String>>) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            created_at: 1_700_000_000,
            kind,
            tags,
            content: String::new(),
            sig: "c".repeat(128),
        }),
        received_at_ms: 0,
    }
}

fn embedded_request(pubkey: &str, amount_msats: u64) -> String {
    format!(
        r#"{{"pubkey":"{pk}","tags":[["amount","{amt}"]]}}"#,
        pk = pubkey,
        amt = amount_msats
    )
}

fn embedded_request_with_id(id: &str, pubkey: &str, amount_msats: u64) -> String {
    format!(
        r#"{{"id":"{id}","pubkey":"{pk}","tags":[["amount","{amt}"]]}}"#,
        id = id,
        pk = pubkey,
        amt = amount_msats
    )
}

#[test]
fn rejects_non_9735() {
    assert!(try_from_event(&make_stored(9734, vec![])).is_none());
    assert!(try_from_event(&make_stored(1, vec![])).is_none());
}

#[test]
fn rejects_when_no_recipient() {
    assert!(try_from_event(&make_stored(9735, vec![])).is_none());
}

#[test]
fn extracts_recipient_and_optional_event_target() {
    let tags = vec![
        vec!["p".into(), "alice".into()],
        vec!["e".into(), "ZAPPED_NOTE".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.provider_pubkey, "b".repeat(64));
    assert_eq!(r.recipient_pubkey, "alice");
    assert_eq!(r.zapped_event_id.as_deref(), Some("ZAPPED_NOTE"));
    assert!(r.zapped_address.is_none());
    assert!(r.sender_pubkey.is_none());
    assert!(r.amount_msats.is_none());
}

#[test]
fn embedded_request_id_is_extracted_for_provider_validation() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec![
            "description".into(),
            embedded_request_with_id("zap-request-1", "embedded_sender", 1000),
        ],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.zap_request_id.as_deref(), Some("zap-request-1"));
}

#[test]
fn uppercase_p_tag_wins_over_embedded_request() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["P".into(), "explicit_sender".into()],
        vec!["description".into(), embedded_request("embedded_sender", 0)],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.sender_pubkey.as_deref(), Some("explicit_sender"));
}

#[test]
fn embedded_request_pubkey_fills_sender_when_no_uppercase_p() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec![
            "description".into(),
            embedded_request("embedded_sender", 1000),
        ],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.sender_pubkey.as_deref(), Some("embedded_sender"));
}

#[test]
fn bolt11_amount_wins_over_embedded_amount_tag() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
        vec!["description".into(), embedded_request("s", 999)],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(50_000_000));
}

#[test]
fn embedded_amount_used_when_bolt11_unparseable() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc1pvj...".into()], // no amount HRP → None
        vec!["description".into(), embedded_request("s", 1234)],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(1234));
}

#[test]
fn carries_preimage_and_bolt11_through() {
    let tags = vec![
        vec!["p".into(), "r".into()],
        vec!["bolt11".into(), "lnbc1m1pvj...".into()],
        vec!["preimage".into(), "abcd".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.bolt11.as_deref(), Some("lnbc1m1pvj..."));
    assert_eq!(r.preimage.as_deref(), Some("abcd"));
    assert_eq!(r.amount_msats, Some(100_000_000));
}

#[test]
fn malformed_description_does_not_panic() {
    let tags = vec![
        vec!["p".into(), "r".into()],
        vec!["description".into(), "{not json}".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.sender_pubkey.is_none());
    assert!(r.amount_msats.is_none());
}

#[test]
fn no_amount_source_yields_none_amount_not_panic() {
    // A receipt with neither a `bolt11` tag nor a `description` carries no
    // amount at all — the field must be `None`, never a panic or a guess.
    let tags = vec![vec!["p".into(), "recipient".into()]];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
    assert!(r.bolt11.is_none());
    assert!(r.sender_pubkey.is_none());
}

#[test]
fn malformed_bolt11_without_embedded_amount_yields_none_amount() {
    // bolt11 is present but unparseable (no `ln*` prefix) and there is no
    // embedded request to fall back on → amount is `None`, bolt11 still
    // carried through verbatim for diagnostics.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "not-a-real-invoice".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
    assert_eq!(r.bolt11.as_deref(), Some("not-a-real-invoice"));
}

#[test]
fn empty_bolt11_string_yields_none_amount() {
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), String::new()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
}

#[test]
fn embedded_amount_non_numeric_falls_through_to_none() {
    // The embedded zap-request's `amount` tag holds a non-numeric string;
    // the `.parse::<u64>()` fails and the decoder yields `None` rather than
    // surfacing a bogus amount.
    let bad = r#"{"pubkey":"s","tags":[["amount","not-a-number"]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["description".into(), bad.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
    // The sender pubkey is still recovered from the embedded request.
    assert_eq!(r.sender_pubkey.as_deref(), Some("s"));
}

#[test]
fn embedded_amount_negative_string_falls_through_to_none() {
    // A negative amount cannot parse as `u64` — must not panic, yields None.
    let bad = r#"{"pubkey":"s","tags":[["amount","-500"]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["description".into(), bad.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
}

#[test]
fn addressable_target_a_tag_is_extracted() {
    // A zap aimed at a long-form / addressable event carries an `a`
    // coordinate instead of (or alongside) an `e` id.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["a".into(), "30023:authorpk:my-article".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(
        r.zapped_address.as_deref(),
        Some("30023:authorpk:my-article")
    );
    assert!(r.zapped_event_id.is_none());
}

#[test]
fn zap_to_profile_has_no_event_or_address_target() {
    // A direct profile zap names only the recipient `p` — no `e`, no `a`.
    // This is a valid receipt and must decode cleanly.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc21n1pvj...".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.recipient_pubkey, "recipient");
    assert!(r.zapped_event_id.is_none());
    assert!(r.zapped_address.is_none());
    assert_eq!(r.amount_msats, Some(2_100));
}

#[test]
fn private_zap_with_opaque_encrypted_description_exposes_no_sender() {
    // NIP-57 private zaps replace the JSON request in `description` with an
    // opaque encrypted blob. It is not valid JSON, so neither a sender nor
    // an amount can be recovered — and there is no uppercase `P` tag.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec![
            "description".into(),
            "A1B2C3D4E5F6==encrypted-private-zap-payload==".into(),
        ],
        vec!["bolt11".into(), "lnbc10n1pvj...".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    // Sender stays hidden — the private-zap invariant.
    assert!(r.sender_pubkey.is_none());
    // The settled amount still comes from the authoritative bolt11 HRP.
    assert_eq!(r.amount_msats, Some(1_000));
}

#[test]
fn first_e_tag_wins_when_receipt_lists_multiple() {
    // A malformed/relay-mangled receipt with two `e` tags: the decoder is
    // deterministic — it pins the first.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["e".into(), "FIRST_NOTE".into()],
        vec!["e".into(), "SECOND_NOTE".into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.zapped_event_id.as_deref(), Some("FIRST_NOTE"));
}

#[test]
fn description_amount_mismatch_distrusts_embedded_fields() {
    // NIP-57 MUST: the bolt11 invoice amount equals the embedded zap
    // request's `amount`. Here bolt11 settles 50_000_000 msat but the
    // embedded request claims 999 — the `description` was rewritten. The
    // decoder must distrust the description: the embedded sender is dropped
    // and the amount falls back to the authoritative bolt11 HRP value.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
        vec!["description".into(), embedded_request("forged_sender", 999)],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(50_000_000));
    assert!(
        r.sender_pubkey.is_none(),
        "a contradicted description must not surface its embedded sender"
    );
}

#[test]
fn description_amount_match_keeps_embedded_sender() {
    // When the bolt11 amount and the embedded `amount` agree, the
    // description is consistent and its embedded sender stays trusted.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
        vec![
            "description".into(),
            embedded_request("real_sender", 50_000_000),
        ],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(50_000_000));
    assert_eq!(r.sender_pubkey.as_deref(), Some("real_sender"));
}

#[test]
fn embedded_request_without_amount_tag_is_not_a_contradiction() {
    // A zap request that carries a `pubkey` but no `amount` tag cannot
    // contradict the bolt11 amount — there is nothing to compare. The
    // embedded sender stays trusted.
    let no_amount_request = r#"{"pubkey":"real_sender","tags":[]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()],
        vec!["description".into(), no_amount_request.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(50_000_000));
    assert_eq!(r.sender_pubkey.as_deref(), Some("real_sender"));
}

#[test]
fn uppercase_p_sender_survives_a_contradicted_description() {
    // The uppercase `P` tag is set by the LN provider independently of the
    // `description`, so an amount contradiction in the description does not
    // taint it — the `P` sender still wins.
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["P".into(), "provider_attested_sender".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
        vec!["description".into(), embedded_request("forged_sender", 999)],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.sender_pubkey.as_deref(), Some("provider_attested_sender"));
    assert_eq!(r.amount_msats, Some(50_000_000));
}

// ---- Security: mixed-type tags must not suppress a later amount tag -----

#[test]
fn null_tag_before_amount_does_not_suppress_amount_extraction() {
    // A hostile relay sends `"tags":[null,["amount","1000"]]`.  The null
    // element is malformed but the `["amount","1000"]` that follows it is
    // well-formed.  The amount MUST still be extracted; returning `None`
    // would let the forgery-guard (`description_contradicted`) never fire.
    let description = r#"{"pubkey":"real_sender","tags":[null,["amount","1000"]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["description".into(), description.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    // Amount extracted despite the leading null tag.
    assert_eq!(r.amount_msats, Some(1000));
    // Sender is still recovered.
    assert_eq!(r.sender_pubkey.as_deref(), Some("real_sender"));
}

#[test]
fn forged_sender_with_mismatched_amount_still_distrusted_with_mixed_type_tags() {
    // A relay forges: embedded sender="forged", embedded amount=999, but
    // bolt11 settles 50_000_000.  It also prepends `null` to the tags array
    // hoping to suppress amount extraction.  The forgery guard must still
    // fire: embedded sender must be dropped, bolt11 amount used.
    let description = r#"{"pubkey":"forged_sender","tags":[null,["amount","999"]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["bolt11".into(), "lnbc500u1pvj...".into()], // 50_000_000 msat
        vec!["description".into(), description.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    // Bolt11 amount wins.
    assert_eq!(r.amount_msats, Some(50_000_000));
    // Forged sender is dropped because amounts contradict.
    assert!(
        r.sender_pubkey.is_none(),
        "a forged embedded sender with a contradicted amount must be dropped"
    );
}

#[test]
fn scalar_string_tag_before_amount_is_skipped() {
    // A string scalar (not an array) in the tags list is also malformed;
    // the amount entry after it must still be reachable.
    let description = r#"{"pubkey":"s","tags":["not-an-array",["amount","500"]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["description".into(), description.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert_eq!(r.amount_msats, Some(500));
}

#[test]
fn non_string_amount_value_in_mixed_tags_is_skipped() {
    // `["amount", 1000]` — the value is a number, not a string. The entry
    // is skipped (continue) and the function returns None for the amount.
    let description = r#"{"pubkey":"s","tags":[["amount",1000]]}"#;
    let tags = vec![
        vec!["p".into(), "recipient".into()],
        vec!["description".into(), description.into()],
    ];
    let r = try_from_event(&make_stored(9735, tags)).unwrap();
    assert!(r.amount_msats.is_none());
    // Sender still usable.
    assert_eq!(r.sender_pubkey.as_deref(), Some("s"));
}

#[test]
fn try_from_kernel_event_decodes_equivalently() {
    use nmp_core::substrate::KernelEvent;
    let kernel = KernelEvent {
        id: "k".repeat(64),
        author: "ln_node".into(),
        kind: 9735,
        created_at: 1_700_000_001,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), "NOTE".into()],
            vec!["bolt11".into(), "lnbc500u1pvj...".into()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    let r = try_from_kernel_event(&kernel).unwrap();
    assert_eq!(r.event_id, "k".repeat(64));
    assert_eq!(r.recipient_pubkey, "recipient");
    assert_eq!(r.zapped_event_id.as_deref(), Some("NOTE"));
    assert_eq!(r.amount_msats, Some(50_000_000));
    // A non-receipt kernel event is rejected.
    let not_receipt = KernelEvent { kind: 1, ..kernel };
    assert!(try_from_kernel_event(&not_receipt).is_none());
}
