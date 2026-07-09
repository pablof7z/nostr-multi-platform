//! Proofs for the FFI-marshalable [`ReplyTargetParams`] bridge input
//! (#2899 Part A) and the stable error codes each target error carries.

use super::*;

const EVENT_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn decodes_event_params_matching_the_typed_constructor() {
    let json = format!(
        r#"{{"target_type":"event","event_id":"{EVENT_ID}","kind":1,"author_pubkey":"{AUTHOR}"}}"#
    );
    let decoded = decode_and_validate_reply_target(&json).unwrap();
    let typed = ReplyTarget::event(EVENT_ID, 1, Some(AUTHOR.to_string())).unwrap();
    assert_eq!(decoded, typed);
}

#[test]
fn decodes_address_params_matching_the_typed_constructor() {
    let json = r#"{"target_type":"address","coordinate":"30023:pk:essay","kind":30023}"#;
    let decoded = decode_and_validate_reply_target(json).unwrap();
    let typed = ReplyTarget::address("30023:pk:essay", 30023, None).unwrap();
    assert_eq!(decoded, typed);
}

#[test]
fn decodes_external_params_matching_the_typed_constructor() {
    let json = r#"{"target_type":"external","uri":"https://example.com/post/1"}"#;
    let decoded = decode_and_validate_reply_target(json).unwrap();
    let typed = ReplyTarget::external("https://example.com/post/1").unwrap();
    assert_eq!(decoded, typed);
}

#[test]
fn malformed_json_is_a_stable_typed_error() {
    let err = decode_and_validate_reply_target("{not json}").unwrap_err();
    assert_eq!(err, ReplyTargetParamsError::MalformedJson);
    assert_eq!(err.code(), "malformed_json");
}

#[test]
fn invalid_event_id_surfaces_the_inner_target_error_code() {
    let json = r#"{"target_type":"event","event_id":"not-hex","kind":1}"#;
    let err = decode_and_validate_reply_target(json).unwrap_err();
    assert_eq!(
        err,
        ReplyTargetParamsError::InvalidTarget(ReplyTargetError::InvalidEventId)
    );
    assert_eq!(err.code(), "invalid_event_id");
}

// ── kind:1111 (NIP-22 comment) targets ──────────────────────────────────────
//
// The genuinely load-bearing marshal proof: a bare `Event{event_id,kind:1111}`
// scalar cannot carry NIP-22 root/parent scope, so it must be rejected (not
// silently mis-decoded), and a real `Comment` payload must decode through the
// actual `nmp-nip22` grammar, not a hand-rolled approximation.

#[test]
fn event_variant_rejects_kind_1111_and_requires_the_comment_variant() {
    let json = format!(
        r#"{{"target_type":"event","event_id":"{EVENT_ID}","kind":1111,"author_pubkey":"{AUTHOR}"}}"#
    );
    let err = decode_and_validate_reply_target(&json).unwrap_err();
    assert_eq!(
        err,
        ReplyTargetParamsError::InvalidTarget(ReplyTargetError::CommentEventRequiresRecord),
        "a kind:1111 target cannot be supplied as a bare scalar Event"
    );
    assert_eq!(err.code(), "comment_event_requires_record");
}

#[test]
fn decodes_a_top_level_comment_target_through_the_real_nip22_decoder() {
    let root = "2222222222222222222222222222222222222222222222222222222222222222";
    let json = format!(
        r#"{{"target_type":"comment","event_id":"{EVENT_ID}","author_pubkey":"{AUTHOR}","created_at":42,"tags":[["E","{root}"],["K","1"],["e","{root}"],["k","1"]],"content":"nice post"}}"#
    );
    let decoded = decode_and_validate_reply_target(&json).unwrap();

    // Cross-checked against the SAME decoder `ReplyTarget::from_kernel_event`
    // uses internally (`nmp_nip22::try_from_kernel_event`) — the marshal must
    // not re-implement NIP-22 tag grammar, only feed the real decoder.
    let event = nmp_core::substrate::KernelEvent {
        id: EVENT_ID.to_string(),
        author: AUTHOR.to_string(),
        kind: 1111,
        created_at: 42,
        tags: vec![
            vec!["E".to_string(), root.to_string()],
            vec!["K".to_string(), "1".to_string()],
            vec!["e".to_string(), root.to_string()],
            vec!["k".to_string(), "1".to_string()],
        ],
        content: "nice post".to_string(),
        relay_provenance: Vec::new(),
    };
    let expected = ReplyTarget::Comment(nmp_nip22::try_from_kernel_event(&event).unwrap());
    assert_eq!(decoded, expected);
}

#[test]
fn comment_variant_with_no_root_scope_tag_is_a_stable_typed_error() {
    // Missing the uppercase root scope tag entirely — not a well-formed
    // NIP-22 comment, so the real decoder returns `None` and the marshal must
    // fail closed with the same code the bare-scalar rejection above uses.
    let json = format!(
        r#"{{"target_type":"comment","event_id":"{EVENT_ID}","author_pubkey":"{AUTHOR}","created_at":1,"tags":[],"content":""}}"#
    );
    let err = decode_and_validate_reply_target(&json).unwrap_err();
    assert_eq!(
        err,
        ReplyTargetParamsError::InvalidTarget(ReplyTargetError::CommentEventRequiresRecord)
    );
}

#[test]
fn comment_variant_rejects_a_non_hex_event_id() {
    let json = r#"{"target_type":"comment","event_id":"not-hex","author_pubkey":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","created_at":1,"tags":[["E","root"]],"content":""}"#;
    let err = decode_and_validate_reply_target(json).unwrap_err();
    assert_eq!(
        err,
        ReplyTargetParamsError::InvalidTarget(ReplyTargetError::InvalidEventId)
    );
}

#[test]
fn every_reply_target_error_has_a_distinct_stable_code() {
    let codes = [
        ReplyTargetError::EmptyTarget.code(),
        ReplyTargetError::InvalidEventId.code(),
        ReplyTargetError::InvalidAuthorPubkey.code(),
        ReplyTargetError::MissingTargetAuthor.code(),
        ReplyTargetError::CommentEventRequiresRecord.code(),
        ReplyTargetError::ParentNotLocallyKnown.code(),
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len(), "codes must be pairwise distinct");
}

// ── #3099 Bug A: never fabricate a root/reply shape from a cache miss ──────

#[test]
fn reject_uncached_note_parent_requires_author_before_failing_closed() {
    // No author at all is a distinct, more specific reject than "parent
    // unknown" — it fires even before thread-shape knowledge would matter.
    let target = ReplyEventTarget {
        event_id: EVENT_ID.to_string(),
        kind: 1,
        author_pubkey: None,
    };
    assert_eq!(
        ReplyTarget::reject_uncached_note_parent(&target),
        ReplyTargetError::MissingTargetAuthor
    );
}

#[test]
fn reject_uncached_note_parent_fails_closed_even_with_a_valid_author() {
    // #3099: this is the load-bearing proof. A well-formed kind:1 `Event`
    // target — valid event id, valid author — must STILL be rejected rather
    // than silently built into a reply that fabricates a root marker. The
    // ONLY way to build a valid published reply to a kind:1 parent is via
    // `ReplyTarget::Note`/`ReplyTarget::Comment`, carrying the parent's real
    // decoded refs.
    let target = ReplyEventTarget {
        event_id: EVENT_ID.to_string(),
        kind: 1,
        author_pubkey: Some(AUTHOR.to_string()),
    };
    assert_eq!(
        ReplyTarget::reject_uncached_note_parent(&target),
        ReplyTargetError::ParentNotLocallyKnown
    );
}
