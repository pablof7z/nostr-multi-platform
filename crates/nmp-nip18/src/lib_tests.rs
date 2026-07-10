use std::collections::BTreeSet;

use super::*;

fn event(kind: u32, content: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: "repost".to_string(),
        author: "alice".to_string(),
        kind,
        created_at: 42,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn rejects_non_repost_kind() {
    assert!(try_from_kernel_event(&event(1, "hello", vec![])).is_none());
}

#[test]
fn primary_kind_1_acquires_kind_6_reposts_and_deletes() {
    let kinds = acquisition_kinds_for_primary([1]);
    assert_eq!(kinds, BTreeSet::from([1, KIND_REPOST, KIND_DELETE]));
}

#[test]
fn non_kind_1_primary_acquires_kind_16_reposts_and_deletes() {
    let kinds = acquisition_kinds_for_primary([20]);
    assert_eq!(
        kinds,
        BTreeSet::from([20, KIND_GENERIC_REPOST, KIND_DELETE])
    );
}

#[test]
fn addressable_primary_acquires_kind_16_and_deletes() {
    let kinds = acquisition_kinds_for_primary([30_023]);
    assert_eq!(
        kinds,
        BTreeSet::from([30_023, KIND_GENERIC_REPOST, KIND_DELETE]),
        "addressable feeds must subscribe to deletes that retract coordinates"
    );
}

#[test]
fn mixed_primary_kinds_acquire_both_repost_wrappers_and_deletes() {
    let kinds = acquisition_kinds_for_primary([1, 20]);
    assert_eq!(
        kinds,
        BTreeSet::from([1, 20, KIND_REPOST, KIND_GENERIC_REPOST, KIND_DELETE])
    );
}

#[test]
fn wrapper_kinds_are_rejected_as_primary_feed_kinds() {
    assert_eq!(
        try_acquisition_kinds_for_primary([1, KIND_REPOST]),
        Err(PrimaryKindError::RepostWrapper { kind: KIND_REPOST })
    );
    assert_eq!(
        try_acquisition_kinds_for_primary([KIND_GENERIC_REPOST]),
        Err(PrimaryKindError::RepostWrapper {
            kind: KIND_GENERIC_REPOST,
        })
    );
}

#[test]
fn empty_primary_set_stays_empty_to_preserve_clear_feed_signal() {
    // An empty primary set is the canonical "clear this feed" signal; the
    // delete kind must NOT be injected, or a clear would become a
    // deletes-only subscription (regression of the FFI/kernel clear path).
    let kinds = acquisition_kinds_for_primary([]);
    assert!(
        kinds.is_empty(),
        "an empty primary set must compile to an empty acquisition set"
    );
}

#[test]
fn delete_kind_is_rejected_as_primary_feed_kind() {
    assert_eq!(
        try_acquisition_kinds_for_primary([1, KIND_DELETE]),
        Err(PrimaryKindError::DeleteKind)
    );
    assert_eq!(
        try_acquisition_kinds_for_primary([KIND_DELETE]),
        Err(PrimaryKindError::DeleteKind)
    );
}

#[test]
#[should_panic(expected = "primary feed kinds must not include repost-wrapper or delete kinds")]
fn infallible_acquisition_panics_on_wrapper_primary_kind() {
    let _ = acquisition_kinds_for_primary([1, KIND_REPOST]);
}

#[test]
fn validate_primary_kinds_matches_the_permissive_transform_for_valid_input() {
    // The strict open-a-feed validator derives the SAME acquisition set as
    // the permissive transform for any non-empty valid primary declaration.
    assert_eq!(
        validate_primary_kinds([1]),
        Ok(BTreeSet::from([1, KIND_REPOST, KIND_DELETE]))
    );
    assert_eq!(
        validate_primary_kinds([20]),
        Ok(BTreeSet::from([20, KIND_GENERIC_REPOST, KIND_DELETE]))
    );
}

#[test]
fn validate_primary_kinds_rejects_wrapper_delete_and_empty() {
    assert_eq!(
        validate_primary_kinds([1, KIND_REPOST]),
        Err(PrimaryKindError::RepostWrapper { kind: KIND_REPOST })
    );
    assert_eq!(
        validate_primary_kinds([1, KIND_DELETE]),
        Err(PrimaryKindError::DeleteKind)
    );
    // The strict twin REJECTS empty (unlike the permissive transform, which
    // treats it as the clear-feed signal): an open feed must declare at least
    // one primary content kind.
    assert_eq!(
        validate_primary_kinds(std::iter::empty::<u32>()),
        Err(PrimaryKindError::EmptyPrimaryKinds)
    );
}

#[test]
fn decodes_repost_with_event_tag_only() {
    let record =
        try_from_kernel_event(&event(KIND_REPOST, "", vec![vec!["e", "target"]])).unwrap();

    assert_eq!(record.target_event_id.as_deref(), Some("target"));
    assert!(record.embedded_event.is_none());
}

#[test]
fn decodes_generic_repost_with_event_tag_only() {
    let record = try_from_kernel_event(&event(
        KIND_GENERIC_REPOST,
        "",
        vec![vec!["e", "target"], vec!["k", "20"]],
    ))
    .unwrap();

    assert_eq!(record.target_event_id.as_deref(), Some("target"));
    assert_eq!(record.target_kind, Some(20));
    assert!(record.embedded_event.is_none());
}

#[test]
fn decodes_embedded_event_payload() {
    let content = r#"{
        "id":"inner",
        "pubkey":"bob",
        "kind":1,
        "created_at":123,
        "tags":[["p","alice"]],
        "content":"hello #nostr",
        "sig":"ignored"
    }"#;
    let record = try_from_kernel_event(&event(KIND_REPOST, content, vec![])).unwrap();
    let inner = record.embedded_event.as_ref().unwrap();

    assert_eq!(record.target_event_id.as_deref(), Some("inner"));
    assert_eq!(record.target_kind, Some(1));
    assert_eq!(inner.author, "bob");
    assert_eq!(inner.kind, 1);
    assert_eq!(inner.tags, vec![vec!["p".to_string(), "alice".to_string()]]);
    assert_eq!(inner.content, "hello #nostr");
}

#[test]
fn generic_repost_a_tag_proves_target_coordinate() {
    let record = try_from_kernel_event(&event(
        KIND_GENERIC_REPOST,
        "",
        vec![vec!["a", "30023:bob:my-article"], vec!["k", "30023"]],
    ))
    .unwrap();
    assert_eq!(
        record.target_address,
        Some(AddressCoordinate::new(30_023, "bob", "my-article"))
    );
}

#[test]
fn embedded_addressable_event_yields_target_coordinate() {
    let content = r#"{
        "id":"inner",
        "pubkey":"bob",
        "kind":30023,
        "created_at":123,
        "tags":[["d","my-article"]],
        "content":"body",
        "sig":"ignored"
    }"#;
    let record = try_from_kernel_event(&event(KIND_GENERIC_REPOST, content, vec![])).unwrap();
    assert_eq!(
        record.target_address,
        Some(AddressCoordinate::new(30_023, "bob", "my-article"))
    );
}

#[test]
fn event_id_only_repost_has_no_target_coordinate() {
    // A bare `e`/`k` repost names an event id, which CANNOT prove a
    // coordinate. target_address must stay None — never guess.
    let record = try_from_kernel_event(&event(
        KIND_GENERIC_REPOST,
        "",
        vec![vec!["e", "target"], vec!["k", "30023"]],
    ))
    .unwrap();
    assert_eq!(record.target_event_id.as_deref(), Some("target"));
    assert_eq!(
        record.target_address, None,
        "an event id must never be fabricated into a coordinate"
    );
}

#[test]
fn p_tag_proves_target_author_for_tag_only_repost() {
    let record = try_from_kernel_event(&event(
        KIND_GENERIC_REPOST,
        "",
        vec![vec!["e", "target"], vec!["p", "bob"], vec!["k", "20"]],
    ))
    .unwrap();
    assert_eq!(record.target_author_pubkey.as_deref(), Some("bob"));
}

#[test]
fn embedded_event_author_proves_target_author_without_p_tag() {
    let content = r#"{
        "id":"inner",
        "pubkey":"bob",
        "kind":1,
        "created_at":123,
        "tags":[],
        "content":"hello",
        "sig":"ignored"
    }"#;
    let record = try_from_kernel_event(&event(KIND_REPOST, content, vec![])).unwrap();
    assert_eq!(record.target_author_pubkey.as_deref(), Some("bob"));
}

#[test]
fn tag_only_repost_without_p_tag_has_no_target_author() {
    // Non-compliant wrapper (NIP-18's `p` tag is only a SHOULD): the
    // author is unknown until the target itself is delivered. Never
    // fabricated via a by-id lookup (#3124).
    let record =
        try_from_kernel_event(&event(KIND_GENERIC_REPOST, "", vec![vec!["e", "target"]]))
            .unwrap();
    assert_eq!(record.target_author_pubkey, None);
}

#[test]
fn malformed_embedded_json_still_decodes_repost_record() {
    let record = try_from_kernel_event(&event(
        KIND_REPOST,
        r#"{"content":"missing required event fields"}"#,
        vec![vec!["e", "target"]],
    ))
    .unwrap();

    assert_eq!(record.target_event_id.as_deref(), Some("target"));
    assert!(record.embedded_event.is_none());
}
