//! Decoder half — `CommentRecord` from a standalone kind:1111 event.
//!
//! NIP-22 tag scoping:
//! - **Uppercase** (`K` / `E` / `A` / `I` / `P`) names the thread **root** —
//!   the original article / note / external resource this comment thread is
//!   attached to.
//! - **Lowercase** (`k` / `e` / `a` / `i` / `p`) names the **parent** — the
//!   immediate thing being replied to. Top-level comments emit root and
//!   parent pointing at the same target.
//!
//! Kind:1111 events carrying an `h` tag belong to NIP-29 (group comments);
//! this decoder returns `None` for them — the `(kind, h-tag)` D4
//! discriminator from `kind-wrappers.md` §6.

use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use nmp_core::tags::first_tag_value;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_COMMENT;

/// What a NIP-22 comment is anchored to. Either a Nostr event (by id), a
/// parameterized-replaceable address (`<kind>:<author>:<d>`), or an external
/// URI (`I`/`i` tag — e.g. `https://...`).
///
/// Aliased onto [`nmp_threading::ThreadPointer`] so the same anchor type is
/// shared between NIP-10 and NIP-22 wrappers without an FFI-visible
/// duplicate. Serde shape is byte-identical to the historic local enum —
/// existing wire formats and tests round-trip unchanged.
pub type CommentPointer = nmp_threading::ThreadPointer;

/// Decoded NIP-22 standalone comment. Immutable per `kind-wrappers.md` §1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentRecord {
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub content: String,
    /// Thread root (uppercase NIP-22 tags).
    pub root: CommentPointer,
    /// Direct parent (lowercase NIP-22 tags). For top-level comments this
    /// equals `root` semantically — same pointer kind and target.
    pub parent: CommentPointer,
}

/// Decode a stored event into a [`CommentRecord`].
///
/// Returns `None` when:
/// - `event.kind != 1111`, or
/// - the event carries an `h` tag (NIP-29 group comments live in `nmp-nip29`), or
/// - neither a root nor a parent pointer can be formed (the event is too
///   malformed to be a NIP-22 comment).
#[must_use] 
pub fn try_from_event(event: &StoredEvent) -> Option<CommentRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(&raw.id, &raw.pubkey, raw.kind, raw.created_at, &raw.tags, &raw.content)
}

/// Hot-path decoder over a borrowed [`KernelEvent`].
#[must_use] 
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<CommentRecord> {
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
) -> Option<CommentRecord> {
    if kind != KIND_COMMENT {
        return None;
    }
    if has_h_tag(tags) {
        return None;
    }

    let root = pointer_from_tags(tags, /* uppercase= */ true)?;
    // Top-level comments may omit lowercase pointers — fall back to the root.
    let parent = pointer_from_tags(tags, /* uppercase= */ false).unwrap_or_else(|| root.clone());

    Some(CommentRecord {
        event_id: id.to_string(),
        author: pubkey.to_string(),
        created_at,
        content: content.to_string(),
        root,
        parent,
    })
}

fn has_h_tag(tags: &[Vec<String>]) -> bool {
    tags.iter().any(|t| t.first().map(String::as_str) == Some("h"))
}

fn pointer_from_tags(tags: &[Vec<String>], uppercase: bool) -> Option<CommentPointer> {
    let (e_key, a_key, i_key, k_key) = if uppercase {
        ("E", "A", "I", "K")
    } else {
        ("e", "a", "i", "k")
    };

    let kind_hint = first_tag_value(tags, k_key).and_then(|s| s.parse::<u32>().ok());

    // Tag lookup needs both column-1 (id/coord/uri) and column-2 (relay)
    // when present. `first_tag_value` only returns column-1, so we re-scan
    // to grab the relay slot for `e`/`a` tags.
    if let Some(tag) = find_tag(tags, e_key) {
        let id = tag.get(1)?.clone();
        if id.is_empty() {
            return None;
        }
        let relay = tag.get(2).filter(|s| !s.is_empty()).cloned();
        return Some(CommentPointer::Event { id, relay, kind: kind_hint });
    }
    if let Some(tag) = find_tag(tags, a_key) {
        let coord = tag.get(1)?.clone();
        if coord.is_empty() {
            return None;
        }
        let relay = tag.get(2).filter(|s| !s.is_empty()).cloned();
        return Some(CommentPointer::Address { coord, relay, kind: kind_hint });
    }
    if let Some(uri) = first_tag_value(tags, i_key) {
        if !uri.is_empty() {
            return Some(CommentPointer::External { uri: uri.to_string() });
        }
    }
    None
}

fn find_tag<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a Vec<String>> {
    tags.iter().find(|t| t.first().map(String::as_str) == Some(key))
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
    fn rejects_non_kind_1111() {
        assert!(try_from_event(&make_stored(1, vec![], "")).is_none());
        assert!(try_from_event(&make_stored(7, vec![], "")).is_none());
    }

    #[test]
    fn rejects_when_h_tag_present() {
        // Belongs to nmp-nip29.
        let tags = vec![
            vec!["E".into(), "ROOT".into()],
            vec!["e".into(), "ROOT".into()],
            vec!["h".into(), "my-group".into()],
        ];
        assert!(try_from_event(&make_stored(1111, tags, "x")).is_none());
    }

    #[test]
    fn top_level_event_comment_has_matching_root_and_parent() {
        let tags = vec![
            vec!["E".into(), "ARTICLE".into(), "wss://r".into()],
            vec!["K".into(), "30023".into()],
            vec!["P".into(), "alice".into()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "first!")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::Event {
                id: "ARTICLE".into(),
                relay: Some("wss://r".into()),
                kind: Some(30023)
            }
        );
        // Parent falls back to root when lowercase pointer is absent.
        assert_eq!(r.parent, r.root);
        assert_eq!(r.content, "first!");
    }

    #[test]
    fn nested_event_comment_has_distinct_parent() {
        let tags = vec![
            vec!["E".into(), "ARTICLE".into()],
            vec!["K".into(), "30023".into()],
            vec!["P".into(), "alice".into()],
            vec!["e".into(), "PARENT_COMMENT".into()],
            vec!["k".into(), "1111".into()],
            vec!["p".into(), "bob".into()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "nested")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::Event { id: "ARTICLE".into(), relay: None, kind: Some(30023) }
        );
        assert_eq!(
            r.parent,
            CommentPointer::Event { id: "PARENT_COMMENT".into(), relay: None, kind: Some(1111) }
        );
    }

    #[test]
    fn address_pointer_for_addressable_root() {
        let tags = vec![
            vec!["A".into(), "30023:alice:intro".into(), "wss://r".into()],
            vec!["K".into(), "30023".into()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "x")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::Address {
                coord: "30023:alice:intro".into(),
                relay: Some("wss://r".into()),
                kind: Some(30023)
            }
        );
    }

    #[test]
    fn external_pointer_for_uri_root() {
        let tags = vec![
            vec!["I".into(), "https://example.com/post".into()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "good read")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::External { uri: "https://example.com/post".into() }
        );
    }

    #[test]
    fn no_root_pointer_means_none() {
        let r = try_from_event(&make_stored(1111, vec![vec!["K".into(), "1".into()]], ""));
        assert!(r.is_none());
    }

    #[test]
    fn try_from_kernel_event_mirrors_try_from_event() {
        let ke = KernelEvent {
            id: "id".into(),
            author: "pk".into(),
            kind: 1111,
            created_at: 1,
            tags: vec![vec!["E".into(), "ROOT".into()]],
            content: "c".into(),
        };
        let r = try_from_kernel_event(&ke).unwrap();
        assert_eq!(r.event_id, "id");
    }

    #[test]
    fn missing_uppercase_k_tag_decodes_with_no_kind_hint() {
        // NIP-22 `K` (root kind) is a hint, not a hard requirement. An `E`
        // tag with no accompanying `K` must still decode — the pointer just
        // carries `kind: None`.
        let tags = vec![vec!["E".into(), "ROOT".into()]];
        let r = try_from_event(&make_stored(1111, tags, "body")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::Event { id: "ROOT".into(), relay: None, kind: None }
        );
        // Parent falls back to root, also kind-less.
        assert_eq!(r.parent, r.root);
    }

    #[test]
    fn non_numeric_k_tag_is_ignored_not_fatal() {
        // A `K` whose value does not parse as u32 must not abort the decode;
        // the kind hint silently becomes `None`.
        let tags = vec![
            vec!["E".into(), "ROOT".into()],
            vec!["K".into(), "not-a-number".into()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "body")).unwrap();
        assert_eq!(
            r.root,
            CommentPointer::Event { id: "ROOT".into(), relay: None, kind: None }
        );
    }

    #[test]
    fn empty_root_event_id_rejected() {
        // Malformed root coordinate: `E` tag present but its id column is "".
        // Decoder must treat the event as non-decodable, not emit a pointer
        // with an empty id.
        let tags = vec![
            vec!["E".into(), String::new()],
            vec!["K".into(), "30023".into()],
        ];
        assert!(try_from_event(&make_stored(1111, tags, "body")).is_none());
    }

    #[test]
    fn empty_root_address_coord_rejected() {
        let tags = vec![
            vec!["A".into(), String::new()],
            vec!["K".into(), "30023".into()],
        ];
        assert!(try_from_event(&make_stored(1111, tags, "body")).is_none());
    }

    #[test]
    fn empty_root_uri_rejected() {
        let tags = vec![vec!["I".into(), String::new()]];
        assert!(try_from_event(&make_stored(1111, tags, "body")).is_none());
    }

    #[test]
    fn malformed_parent_falls_back_to_root() {
        // An `e` tag with an empty id column yields no parent pointer; the
        // decoder must fall back to the root rather than dropping the event.
        let tags = vec![
            vec!["E".into(), "ROOT".into()],
            vec!["e".into(), String::new()],
        ];
        let r = try_from_event(&make_stored(1111, tags, "body")).unwrap();
        assert_eq!(r.parent, r.root);
        assert_eq!(
            r.root,
            CommentPointer::Event { id: "ROOT".into(), relay: None, kind: None }
        );
    }
}
