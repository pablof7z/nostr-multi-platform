//! Decoder half (read side) per `docs/design/kind-wrappers.md` §3.1.
//!
//! Pure, allocation-bounded, no I/O. ONE decoder, classified output: rather
//! than the §9 #3 anti-pattern (one class, many kinds, kind-discriminated
//! getters), the six NIP-51 kinds decode into a single immutable
//! [`ListRecord`] carrying a [`ListKind`] classifier the consumer reads. Each
//! kind's invariants (the three `3000x` sets require a `d` tag; the three
//! `1000x` lists do not) are enforced *inside* the one decoder, not pushed onto
//! callers.
//!
//! Decoder name is the uniform `try_from_event` per PD-010.

mod items;

pub use items::{ListItems, RelayEntry};

use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::{
    is_parameterized, KIND_BOOKMARK_LIST, KIND_BOOKMARK_SETS, KIND_FOLLOW_SETS, KIND_MUTE_LIST,
    KIND_RELAY_LIST, KIND_RELAY_SETS,
};

/// Classifier discriminating the six NIP-51 kinds this crate owns. The record
/// is otherwise uniform — consumers branch on this enum, never on `kind`
/// integers, and never via per-kind getters (the §9 #3 anti-pattern).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListKind {
    /// kind 10000 — replaceable mute list.
    Mute,
    /// kind 10003 — replaceable bookmark list.
    Bookmark,
    /// kind 10002 — replaceable relay list (NIP-65 overlap; read-side only).
    RelayList,
    /// kind 30000 — parameterized-replaceable follow set.
    FollowSet,
    /// kind 30002 — parameterized-replaceable relay set.
    RelaySet,
    /// kind 30003 — parameterized-replaceable bookmark set.
    BookmarkSet,
}

impl ListKind {
    /// Map a Nostr `kind` integer to its [`ListKind`], or `None` if the kind is
    /// not one of the six this crate owns.
    #[must_use]
    pub fn from_kind(kind: u32) -> Option<Self> {
        match kind {
            KIND_MUTE_LIST => Some(Self::Mute),
            KIND_BOOKMARK_LIST => Some(Self::Bookmark),
            KIND_RELAY_LIST => Some(Self::RelayList),
            KIND_FOLLOW_SETS => Some(Self::FollowSet),
            KIND_RELAY_SETS => Some(Self::RelaySet),
            KIND_BOOKMARK_SETS => Some(Self::BookmarkSet),
            _ => None,
        }
    }

    /// The Nostr `kind` integer this classifier corresponds to.
    #[must_use]
    pub fn kind(self) -> u32 {
        match self {
            Self::Mute => KIND_MUTE_LIST,
            Self::Bookmark => KIND_BOOKMARK_LIST,
            Self::RelayList => KIND_RELAY_LIST,
            Self::FollowSet => KIND_FOLLOW_SETS,
            Self::RelaySet => KIND_RELAY_SETS,
            Self::BookmarkSet => KIND_BOOKMARK_SETS,
        }
    }
}

/// Decoded NIP-51 list. Immutable; produced once at ingest, read everywhere
/// (no read-side mutable wrapper — that is the D4 violation NDK's `NDKList`
/// setter pattern commits).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListRecord {
    /// Hex event id (64 chars).
    pub event_id: String,
    /// Hex pubkey of the list author (64 chars).
    pub author: String,
    /// Which of the six NIP-51 kinds this is.
    pub list_kind: ListKind,
    /// NIP-33 `d` identifier. Empty string for the three replaceable kinds
    /// (10000 / 10002 / 10003); the per-author set identifier for the three
    /// parameterized kinds (30000 / 30002 / 30003).
    pub d_tag: String,
    /// `title` tag (set kinds surface it; replaceable kinds rarely do, but it
    /// is extracted uniformly if present).
    pub title: Option<String>,
    /// `description` tag, if present.
    pub description: Option<String>,
    /// `image` tag, if present.
    pub image: Option<String>,
    /// Typed public list entries (p/e/a/t/r/relay/word).
    pub items: ListItems,
    /// **Encrypted private payload, preserved VERBATIM and NOT decrypted.**
    ///
    /// NIP-51 stores private list entries as NIP-04-encrypted JSON in the
    /// event `.content` field. Decoding here is pure (no signer, no I/O — the
    /// §9 #5 rule: decoders take `&StoredEvent`, never the world); decryption
    /// is therefore an *actor* concern, performed downstream by code that holds
    /// the user's key. This field carries the raw ciphertext (empty string
    /// when the list has no private entries) so that actor can decrypt it
    /// later without re-fetching the event.
    pub encrypted_payload: String,
    /// `created_at` from the event header (unix seconds). Always set.
    pub created_at: u64,
    /// Raw tags preserved verbatim for callers that need unknown-tag access.
    pub tags: Vec<Vec<String>>,
}

/// Decode a stored event into a [`ListRecord`].
///
/// Returns `None` when:
/// - `event.kind` is not one of the six NIP-51 kinds this crate owns, or
/// - the kind is parameterized (30000 / 30002 / 30003) and the required NIP-33
///   `d` tag is missing.
///
/// `title` / `description` / `image` are optional for every kind. The event
/// content is preserved verbatim into `encrypted_payload` and never decrypted.
#[must_use]
pub fn try_from_event(event: &StoredEvent) -> Option<ListRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(
        &raw.id,
        &raw.pubkey,
        raw.kind,
        raw.created_at,
        &raw.tags,
        &raw.content,
    )
}

/// Decode directly from a borrowed [`KernelEvent`] — the view-substrate event
/// shape — without re-wrapping it in a `StoredEvent`/`Arc<RawEvent>`.
///
/// The hot insert path in `view::accumulator` calls this per delivered event;
/// per D8 (zero per-event alloc after warmup) it must not allocate an
/// intermediate `RawEvent` + `Arc` just to satisfy [`try_from_event`]'s
/// `&StoredEvent` signature. The only allocations are the owned `String`s the
/// immutable `ListRecord` output unavoidably owns.
#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<ListRecord> {
    decode_borrowed(
        &event.id,
        &event.author,
        event.kind,
        event.created_at,
        &event.tags,
        &event.content,
    )
}

/// Shared decode core over borrowed event fields. Both `try_from_event` (store
/// side) and `try_from_kernel_event` (view side) funnel through here so the
/// kind classification, `d`-tag gating and item extraction stay defined exactly
/// once.
fn decode_borrowed(
    id: &str,
    pubkey: &str,
    kind: u32,
    created_at: u64,
    tags: &[Vec<String>],
    content: &str,
) -> Option<ListRecord> {
    let list_kind = ListKind::from_kind(kind)?;

    // Parameterized-replaceable set kinds require a non-empty NIP-33 `d` tag.
    // Replaceable list kinds carry `d_tag == ""`.
    let d_tag = if is_parameterized(kind) {
        let d = first_tag_value(tags, "d")?;
        if d.trim().is_empty() {
            return None;
        }
        d.to_string()
    } else {
        String::new()
    };

    Some(ListRecord {
        event_id: id.to_string(),
        author: pubkey.to_string(),
        list_kind,
        d_tag,
        title: first_tag_value(tags, "title").map(str::to_string),
        description: first_tag_value(tags, "description").map(str::to_string),
        image: first_tag_value(tags, "image").map(str::to_string),
        items: ListItems::from_tags(tags),
        // Verbatim — never decrypted here (see field doc-comment).
        encrypted_payload: content.to_string(),
        created_at,
        tags: tags.to_vec(),
    })
}

/// Return the value column of the first tag whose key column equals `key`.
fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::{RawEvent, StoredEvent};
    use std::sync::Arc;

    fn make_stored(kind: u32, tags: Vec<Vec<String>>, content: &str) -> StoredEvent {
        StoredEvent {
            raw: Arc::new(RawEvent {
                id: "a".repeat(64),
                pubkey: "b".repeat(64),
                created_at: 1_700_000_000,
                kind,
                tags,
                content: content.into(),
                sig: "c".repeat(128),
            }),
            received_at_ms: 0,
        }
    }

    #[test]
    fn each_replaceable_kind_decodes_without_d_tag() {
        for kind in [KIND_MUTE_LIST, KIND_RELAY_LIST, KIND_BOOKMARK_LIST] {
            let ev = make_stored(kind, vec![vec!["p".into(), "pk".into()]], "");
            let rec = try_from_event(&ev).expect("replaceable list decodes without d");
            assert_eq!(rec.d_tag, "");
            assert_eq!(rec.list_kind.kind(), kind);
        }
    }

    #[test]
    fn each_set_kind_decodes_with_d_tag() {
        for kind in [KIND_FOLLOW_SETS, KIND_RELAY_SETS, KIND_BOOKMARK_SETS] {
            let ev = make_stored(kind, vec![vec!["d".into(), "myset".into()]], "");
            let rec = try_from_event(&ev).expect("set decodes with d");
            assert_eq!(rec.d_tag, "myset");
            assert_eq!(rec.list_kind.kind(), kind);
        }
    }

    #[test]
    fn wrong_kind_returns_none() {
        // kind 1 (short note) and 30023 (article) are not ours.
        assert!(try_from_event(&make_stored(1, vec![], "")).is_none());
        assert!(
            try_from_event(&make_stored(30023, vec![vec!["d".into(), "x".into()]], "")).is_none()
        );
        // kind 10001 (pin list) is a NIP-51 kind we deliberately do NOT own.
        assert!(try_from_event(&make_stored(10001, vec![], "")).is_none());
    }

    #[test]
    fn set_kind_missing_d_returns_none() {
        let ev = make_stored(KIND_FOLLOW_SETS, vec![vec!["p".into(), "pk".into()]], "");
        assert!(try_from_event(&ev).is_none());
    }

    #[test]
    fn set_kind_whitespace_d_returns_none() {
        let ev = make_stored(KIND_RELAY_SETS, vec![vec!["d".into(), "   ".into()]], "");
        assert!(try_from_event(&ev).is_none());
    }

    #[test]
    fn extracts_typed_items() {
        let ev = make_stored(
            KIND_BOOKMARK_LIST,
            vec![
                vec!["e".into(), "ev1".into()],
                vec!["a".into(), "30023:pk:slug".into()],
                vec!["t".into(), "nostr".into()],
                vec!["r".into(), "wss://relay".into(), "read".into()],
            ],
            "",
        );
        let rec = try_from_event(&ev).unwrap();
        assert_eq!(rec.items.events, vec!["ev1"]);
        assert_eq!(rec.items.addresses, vec!["30023:pk:slug"]);
        assert_eq!(rec.items.hashtags, vec!["nostr"]);
        assert_eq!(rec.items.relays[0].url, "wss://relay");
        assert_eq!(rec.items.relays[0].marker.as_deref(), Some("read"));
    }

    #[test]
    fn surfaces_set_metadata() {
        let ev = make_stored(
            KIND_FOLLOW_SETS,
            vec![
                vec!["d".into(), "friends".into()],
                vec!["title".into(), "Close Friends".into()],
                vec!["description".into(), "people I trust".into()],
                vec!["image".into(), "https://example.com/i.png".into()],
            ],
            "",
        );
        let rec = try_from_event(&ev).unwrap();
        assert_eq!(rec.title.as_deref(), Some("Close Friends"));
        assert_eq!(rec.description.as_deref(), Some("people I trust"));
        assert_eq!(rec.image.as_deref(), Some("https://example.com/i.png"));
    }

    #[test]
    fn encrypted_content_preserved_verbatim_and_not_decrypted() {
        let cipher = "AbCdEf==?iv=ZX=="; // opaque NIP-04 ciphertext shape
        let ev = make_stored(KIND_MUTE_LIST, vec![vec!["p".into(), "pk".into()]], cipher);
        let rec = try_from_event(&ev).unwrap();
        // Byte-for-byte identical — proves no decryption / no coercion.
        assert_eq!(rec.encrypted_payload, cipher);
    }

    #[test]
    fn tags_preserved_verbatim() {
        let tags = vec![
            vec!["d".into(), "s".into()],
            vec!["p".into(), "pk".into(), "wss://hint".into()],
            vec!["weird".into(), "x".into()],
        ];
        let ev = make_stored(KIND_RELAY_SETS, tags.clone(), "");
        let rec = try_from_event(&ev).unwrap();
        assert_eq!(rec.tags, tags);
    }

    #[test]
    fn list_kind_round_trips_through_kind_integer() {
        for k in crate::kinds::ALL_KINDS {
            let lk = ListKind::from_kind(*k).expect("known kind classifies");
            assert_eq!(lk.kind(), *k);
        }
    }
}
