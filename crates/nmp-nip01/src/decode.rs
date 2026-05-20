//! Decoder half (read side) — `NoteRecord` from a kind:1 event.
//!
//! Pure, allocation-bounded, no I/O. Uses [`nmp_core::tags::parse_nip10`] so
//! every NIP-10 reference (root, reply, mentions, mentioned pubkeys) is
//! parsed once and carried in the record alongside the raw fields.

use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::{parse_nip10, Nip10Refs};
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;

/// Decoded NIP-01 short text note. Immutable per `kind-wrappers.md` §1 — no
/// setters, no shared mutable wrapper (D4 violation). Apps that need a
/// modified event publish a new one through `NoteBuilder`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    /// Hex event id (64 chars).
    pub event_id: String,
    /// Hex pubkey of the note author (64 chars).
    pub author: String,
    /// `created_at` from the event header (unix seconds).
    pub created_at: u64,
    /// Plain-text content (NIP-01 doesn't constrain the format).
    pub content: String,
    /// NIP-10 thread references parsed once at decode time.
    pub refs: Nip10Refs,
}

impl NoteRecord {
    /// True when the note has no NIP-10 root/reply markers — it's a thread
    /// root itself, not a reply. Mirrors applesauce `Note.isRoot`.
    pub fn is_root(&self) -> bool {
        self.refs.is_root()
    }

    /// True when the note replies to something. Mirrors applesauce
    /// `Note.isReply`.
    pub fn is_reply(&self) -> bool {
        self.refs.is_reply()
    }
}

/// Decode a stored event into a [`NoteRecord`].
///
/// Returns `None` when `event.kind != 1`.
pub fn try_from_event(event: &StoredEvent) -> Option<NoteRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(&raw.id, &raw.pubkey, raw.kind, raw.created_at, &raw.tags, &raw.content)
}

/// Decode directly from a borrowed [`KernelEvent`] — the view-substrate event
/// shape — without first re-wrapping it in a `StoredEvent`/`Arc<RawEvent>`.
///
/// Hot path: every kind-1 event delivered to a `RepliesView`/`ThreadView`
/// runs through here.
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<NoteRecord> {
    decode_borrowed(
        &event.id,
        &event.author,
        event.kind,
        event.created_at,
        &event.tags,
        &event.content,
    )
}

fn decode_borrowed(
    id: &str,
    pubkey: &str,
    kind: u32,
    created_at: u64,
    tags: &[Vec<String>],
    content: &str,
) -> Option<NoteRecord> {
    if kind != KIND_SHORT_NOTE {
        return None;
    }
    Some(NoteRecord {
        event_id: id.to_string(),
        author: pubkey.to_string(),
        created_at,
        content: content.to_string(),
        refs: parse_nip10(tags),
    })
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
    fn rejects_non_kind_1() {
        assert!(try_from_event(&make_stored(7, vec![], "")).is_none());
        assert!(try_from_event(&make_stored(30023, vec![], "")).is_none());
    }

    #[test]
    fn rejects_nip01_sibling_kinds_0_and_3() {
        // Regression guard: kind 0 (profile metadata) and kind 3 (contact /
        // follow list) are NIP-01 kinds too, but `kinds.rs` deliberately scopes
        // this crate to kind 1 only — their ingest still lives in `nmp-core`.
        // If a future refactor lets them leak into the kind-1 decoder this
        // test fails loudly. kind 3 is the NIP-02 contact list specifically.
        assert!(
            try_from_event(&make_stored(0, vec![], "{\"name\":\"alice\"}")).is_none(),
            "kind 0 profile metadata must not decode as a short text note"
        );
        assert!(
            try_from_event(&make_stored(3, vec![vec!["p".into(), "alice".into()]], "")).is_none(),
            "kind 3 contact list (NIP-02) must not decode as a short text note"
        );
    }

    #[test]
    fn try_from_kernel_event_rejects_non_kind_1() {
        // The hot-path decoder must reject foreign kinds identically to the
        // StoredEvent path — pin both kind 3 (NIP-02) and a kind boundary.
        for kind in [0u32, 3, 7, 2, 30023] {
            let ke = KernelEvent {
                id: "id".into(),
                author: "pk".into(),
                kind,
                created_at: 0,
                tags: vec![],
                content: "".into(),
            };
            assert!(
                try_from_kernel_event(&ke).is_none(),
                "kind {kind} must not decode via try_from_kernel_event"
            );
        }
    }

    #[test]
    fn unmarked_single_e_tag_is_a_positional_reply() {
        // NIP-10 deprecated positional form: a single `e` tag with no marker
        // makes that event both root and direct parent. Decoder must surface
        // it as a reply, not a root.
        let tags = vec![vec!["e".into(), "ONLY".into()]];
        let r = try_from_event(&make_stored(1, tags, "positional reply")).unwrap();
        assert!(r.is_reply());
        assert!(!r.is_root());
        assert_eq!(r.refs.reply.as_ref().unwrap().id, "ONLY");
        assert_eq!(r.refs.root.as_ref().unwrap().id, "ONLY");
    }

    #[test]
    fn empty_e_tag_id_does_not_make_a_phantom_reply() {
        // An `e` tag whose id column is empty must be ignored — otherwise a
        // malformed event would masquerade as a reply to "".
        let tags = vec![vec!["e".into(), "".into()]];
        let r = try_from_event(&make_stored(1, tags, "hi")).unwrap();
        assert!(r.is_root(), "empty e-tag id yields no reply pointer");
        assert!(!r.is_reply());
    }

    #[test]
    fn preserves_unicode_and_empty_content_verbatim() {
        // NIP-01 does not constrain content; the decoder must not normalize,
        // trim, or reject it (only the *builder* enforces non-empty).
        let unicode = try_from_event(&make_stored(1, vec![], "héllo 🦀 \n\ttab")).unwrap();
        assert_eq!(unicode.content, "héllo 🦀 \n\ttab");
        let empty = try_from_event(&make_stored(1, vec![], "")).unwrap();
        assert_eq!(empty.content, "");
        assert!(empty.is_root());
    }

    #[test]
    fn root_note_has_no_refs() {
        let r = try_from_event(&make_stored(1, vec![], "hello")).unwrap();
        assert!(r.is_root());
        assert!(!r.is_reply());
        assert_eq!(r.content, "hello");
    }

    #[test]
    fn reply_note_carries_marked_refs() {
        let tags = vec![
            vec!["e".into(), "ROOT".into(), "".into(), "root".into()],
            vec!["e".into(), "PARENT".into(), "".into(), "reply".into()],
            vec!["p".into(), "alice".into()],
        ];
        let r = try_from_event(&make_stored(1, tags, "reply!")).unwrap();
        assert!(r.is_reply());
        assert!(!r.is_root());
        assert_eq!(r.refs.root.as_ref().unwrap().id, "ROOT");
        assert_eq!(r.refs.reply.as_ref().unwrap().id, "PARENT");
        assert_eq!(r.refs.mentioned_pubkeys, vec!["alice"]);
    }

    #[test]
    fn try_from_kernel_event_mirrors_try_from_event() {
        let ke = KernelEvent {
            id: "id".into(),
            author: "pk".into(),
            kind: 1,
            created_at: 42,
            tags: vec![vec!["e".into(), "X".into()]],
            content: "c".into(),
        };
        let r = try_from_kernel_event(&ke).unwrap();
        assert_eq!(r.event_id, "id");
        assert!(r.is_reply());
        assert_eq!(r.refs.reply.as_ref().unwrap().id, "X");
    }

    #[test]
    fn carries_header_fields_verbatim() {
        let r = try_from_event(&make_stored(1, vec![], "x")).unwrap();
        assert_eq!(r.event_id.len(), 64);
        assert_eq!(r.author.len(), 64);
        assert_eq!(r.created_at, 1_700_000_000);
    }
}
