//! `nmp-nip18` — NIP-18 repost decoding and read-surfacing primitives.
//!
//! This crate owns generic repost wire interpretation. It does not render UI,
//! choose relay policy, or depend on any app crate.

use nmp_core::substrate::KernelEvent;
use serde::Deserialize;

mod action;
mod coordinate;
mod delete;
mod primary_kind;
mod repost_projection;
mod wire;

pub use action::{
    build_repost_event, register_actions, Nip18Descriptor, QuoteRepostAction, QuoteRepostModule,
    RepostAction, RepostModule,
};
pub use coordinate::{is_addressable_kind, AddressCoordinate};
pub use delete::{DeleteRecord, KIND_DELETE};
pub use primary_kind::{
    acquisition_kinds_for_primary, try_acquisition_kinds_for_primary, validate_primary_kinds,
    PrimaryKindError,
};
pub use repost_projection::{
    repost_activity_interest_shape, RepostActivity, RepostActivityProjection, RepostObservation,
    RepostTarget,
};

/// NIP-18 repost event kind for kind:1 short-text notes.
pub const KIND_REPOST: u32 = 6;

/// NIP-18 generic repost event kind for non-kind:1 targets.
pub const KIND_GENERIC_REPOST: u32 = 16;

/// Return whether `kind` is a NIP-18 repost wrapper kind.
#[must_use]
pub const fn is_repost_kind(kind: u32) -> bool {
    kind == KIND_REPOST || kind == KIND_GENERIC_REPOST
}

/// Decoded inner event embedded in a repost `content` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedEvent {
    pub id: String,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Decoded NIP-18 repost record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepostRecord {
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub target_event_id: Option<String>,
    pub target_kind: Option<u32>,
    /// Address coordinate of the target, when the wrapper carries an `a` tag (a
    /// generic repost of a replaceable/addressable event) or embeds an
    /// addressable event. This is the canonical row identity for addressable
    /// targets — present means the coordinate is *proven*, never guessed from an
    /// event id. See [`crate::AddressCoordinate`].
    pub target_address: Option<AddressCoordinate>,
    pub embedded_event: Option<EmbeddedEvent>,
}

/// Decode a [`KernelEvent`] as a NIP-18 repost.
///
/// Returns `None` for every non-repost event. A repost with only an `e` tag and
/// no embedded event is still a repost record; consumers can render a
/// placeholder while the target is unresolved.
#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<RepostRecord> {
    if !is_repost_kind(event.kind) {
        return None;
    }

    let embedded_event = parse_embedded_event(&event.content);
    let target_event_id = first_event_tag(&event.tags)
        .or_else(|| embedded_event.as_ref().map(|inner| inner.id.clone()));
    let target_kind =
        first_kind_tag(&event.tags).or_else(|| embedded_event.as_ref().map(|inner| inner.kind));
    // Prefer the explicit `a` tag (proven coordinate). Otherwise derive the
    // coordinate from an embedded addressable event — its (kind, pubkey, d) is
    // fully known. Never derive a coordinate from a bare `e`/`k` pair: an event
    // id cannot prove a coordinate, so such a target stays address-unresolved.
    let target_address = first_address_tag(&event.tags).or_else(|| {
        embedded_event.as_ref().and_then(|inner| {
            AddressCoordinate::from_event(&KernelEvent {
                id: inner.id.clone(),
                author: inner.author.clone(),
                kind: inner.kind,
                created_at: inner.created_at,
                tags: inner.tags.clone(),
                content: inner.content.clone(),
                relay_provenance: Vec::new(),
            })
        })
    });

    Some(RepostRecord {
        event_id: event.id.clone(),
        author: event.author.clone(),
        created_at: event.created_at,
        target_event_id,
        target_kind,
        target_address,
        embedded_event,
    })
}

#[derive(Deserialize)]
struct EmbeddedEventWire {
    id: String,
    pubkey: String,
    kind: u32,
    created_at: u64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
    content: String,
}

fn parse_embedded_event(raw: &str) -> Option<EmbeddedEvent> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let wire: EmbeddedEventWire = serde_json::from_str(trimmed).ok()?;
    Some(EmbeddedEvent {
        id: wire.id,
        author: wire.pubkey,
        kind: wire.kind,
        created_at: wire.created_at,
        tags: wire.tags,
        content: wire.content,
    })
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
}

fn first_kind_tag(tags: &[Vec<String>]) -> Option<u32> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "k") {
            tag.get(1).and_then(|raw| raw.parse::<u32>().ok())
        } else {
            None
        }
    })
}

fn first_address_tag(tags: &[Vec<String>]) -> Option<AddressCoordinate> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "a") {
            tag.get(1).and_then(|raw| AddressCoordinate::parse(raw))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
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
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
