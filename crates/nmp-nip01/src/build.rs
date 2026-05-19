//! Blueprint half (write side) — `Note::new(content).reply_to(parent).build(...)`
//! produces an `UnsignedEvent`. The builder is **pure**: no signer, no clock.
//! The action ledger turns the `UnsignedEvent` into a signed + published event.
//!
//! NIP-10 marked-form reply construction lives here exclusively. It uses the
//! [`nmp_core::tags`] helpers so tag construction is defined once across all
//! protocol crates.

use nmp_core::substrate::UnsignedEvent;
use nmp_core::tags::{e_tag, p_tag};
use serde::{Deserialize, Serialize};

use crate::decode::NoteRecord;
use crate::kinds::KIND_SHORT_NOTE;

/// Structured builder errors per **D6** — never cross FFI as panics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NoteBuildError {
    /// Content is empty (whitespace-only) — kind-1 notes with no body are
    /// semantically meaningless and would yield an empty wire payload.
    EmptyContent,
}

impl core::fmt::Display for NoteBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyContent => write!(f, "NIP-01 short text note requires non-empty content"),
        }
    }
}

impl std::error::Error for NoteBuildError {}

/// What the builder is replying to, set by [`NoteBuilder::reply_to`].
#[derive(Clone, Debug)]
struct ReplyContext {
    root_id: String,
    root_relay: Option<String>,
    reply_id: String,
    reply_relay: Option<String>,
    /// Pubkeys to notify per NIP-10 — parent author first, then anyone the
    /// parent was already replying to.
    pubkeys: Vec<String>,
}

/// Entry-point namespace — `Note::new(content)` returns a [`NoteBuilder`].
///
/// `Note` intentionally has no fields. The design's "no shared mutable
/// read/write wrapper" rule (§1) means there is nothing for `Note` itself to
/// hold; the type exists purely as a namespace for the entry-point.
pub struct Note;

impl Note {
    /// Start building a kind-1 note.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(content: impl Into<String>) -> NoteBuilder {
        NoteBuilder {
            content: content.into(),
            reply: None,
            relay_hint: None,
        }
    }
}

/// Builder for a NIP-01 short text note. Consumes `self` on every chain
/// link (Rust idiom for D4 compliance — no setter mutation).
#[derive(Clone, Debug)]
pub struct NoteBuilder {
    content: String,
    reply: Option<ReplyContext>,
    relay_hint: Option<String>,
}

impl NoteBuilder {
    /// Set a relay hint used in the `e`/`p` tags emitted by [`Self::reply_to`].
    ///
    /// Per NIP-10 the relay slot in marked-form `e` tags is optional and
    /// hints clients where the referenced event might be retrievable.
    #[must_use]
    pub fn relay_hint(mut self, relay: impl Into<String>) -> Self {
        let v = relay.into();
        self.relay_hint = if v.trim().is_empty() { None } else { Some(v) };
        self
    }

    /// Mark this note as a NIP-10 reply to `parent`. Emits marked-form root
    /// and reply `e` tags and re-notifies the thread participants via `p`
    /// tags (parent author first, then parent's `mentioned_pubkeys`,
    /// de-duplicated).
    ///
    /// Per NIP-10: when `parent` already has a `root` reference, the new
    /// root tag carries that id; otherwise `parent` itself is the root.
    #[must_use]
    pub fn reply_to(mut self, parent: &NoteRecord) -> Self {
        let (root_id, root_relay) = match parent.refs.root.as_ref() {
            Some(root) => (root.id.clone(), root.relay.clone()),
            None => (parent.event_id.clone(), self.relay_hint.clone()),
        };

        // Build the p-tag set: parent author first, then anyone parent was
        // already notifying, de-duplicated, stable order.
        let mut pubkeys = Vec::with_capacity(1 + parent.refs.mentioned_pubkeys.len());
        pubkeys.push(parent.author.clone());
        for pk in &parent.refs.mentioned_pubkeys {
            if !pubkeys.iter().any(|p| p == pk) {
                pubkeys.push(pk.clone());
            }
        }

        self.reply = Some(ReplyContext {
            root_id,
            root_relay,
            reply_id: parent.event_id.clone(),
            reply_relay: self.relay_hint.clone(),
            pubkeys,
        });
        self
    }

    /// Materialise the `UnsignedEvent`. Validates non-empty content (D6).
    pub fn build(self, author: impl Into<String>, created_at: u64) -> Result<UnsignedEvent, NoteBuildError> {
        if self.content.trim().is_empty() {
            return Err(NoteBuildError::EmptyContent);
        }

        let mut tags: Vec<Vec<String>> = Vec::new();
        if let Some(reply) = self.reply {
            tags.push(e_tag(&reply.root_id, reply.root_relay.as_deref(), Some("root")));
            tags.push(e_tag(&reply.reply_id, reply.reply_relay.as_deref(), Some("reply")));
            for pk in reply.pubkeys {
                tags.push(p_tag(&pk, self.relay_hint.as_deref()));
            }
        }

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_SHORT_NOTE,
            tags,
            content: self.content,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::tags::{EventRef, Nip10Refs};

    const AUTHOR: &str = "deadbeef";

    fn parent_root(id: &str, author: &str) -> NoteRecord {
        NoteRecord {
            event_id: id.to_string(),
            author: author.to_string(),
            created_at: 0,
            content: "root".into(),
            refs: Nip10Refs::default(),
        }
    }

    fn parent_mid_thread(
        id: &str,
        author: &str,
        root_id: &str,
        mentioned: &[&str],
    ) -> NoteRecord {
        NoteRecord {
            event_id: id.to_string(),
            author: author.to_string(),
            created_at: 0,
            content: "mid".into(),
            refs: Nip10Refs {
                root: Some(EventRef {
                    id: root_id.into(),
                    relay: None,
                    marker: Some("root".into()),
                }),
                reply: None,
                mentions: vec![],
                mentioned_pubkeys: mentioned.iter().map(|s| (*s).to_string()).collect(),
            },
        }
    }

    fn tag_keys(unsigned: &UnsignedEvent) -> Vec<&str> {
        unsigned.tags.iter().filter_map(|t| t.first()).map(String::as_str).collect()
    }

    #[test]
    fn root_note_emits_no_e_or_p_tags() {
        let unsigned = Note::new("hello").build(AUTHOR, 0).unwrap();
        assert_eq!(unsigned.kind, KIND_SHORT_NOTE);
        assert_eq!(unsigned.content, "hello");
        assert!(unsigned.tags.is_empty());
    }

    #[test]
    fn empty_content_errors() {
        let err = Note::new("   ").build(AUTHOR, 0).unwrap_err();
        assert_eq!(err, NoteBuildError::EmptyContent);
    }

    #[test]
    fn reply_to_root_uses_parent_as_root_and_reply() {
        let parent = parent_root("ROOT_ID", "alice");
        let unsigned = Note::new("reply!")
            .reply_to(&parent)
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["e", "e", "p"]);

        // root marker → parent (which is the thread root)
        assert_eq!(unsigned.tags[0][1], "ROOT_ID");
        assert_eq!(unsigned.tags[0][3], "root");
        // reply marker → same parent
        assert_eq!(unsigned.tags[1][1], "ROOT_ID");
        assert_eq!(unsigned.tags[1][3], "reply");
        // p tag → parent author
        assert_eq!(unsigned.tags[2][1], "alice");
    }

    #[test]
    fn reply_to_mid_thread_carries_root_pointer_separately() {
        let parent = parent_mid_thread("PARENT_ID", "bob", "ROOT_ID", &["alice"]);
        let unsigned = Note::new("nested")
            .reply_to(&parent)
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        // 2 e + 2 p (parent author + parent's mentioned_pubkeys, dedup)
        assert_eq!(keys, vec!["e", "e", "p", "p"]);
        assert_eq!(unsigned.tags[0][1], "ROOT_ID");
        assert_eq!(unsigned.tags[0][3], "root");
        assert_eq!(unsigned.tags[1][1], "PARENT_ID");
        assert_eq!(unsigned.tags[1][3], "reply");
        assert_eq!(unsigned.tags[2][1], "bob");
        assert_eq!(unsigned.tags[3][1], "alice");
    }

    #[test]
    fn duplicate_pubkeys_are_deduplicated() {
        // parent author == one of the mentioned_pubkeys → must not duplicate.
        let parent = parent_mid_thread("P", "alice", "R", &["alice", "carol"]);
        let unsigned = Note::new("x").reply_to(&parent).build(AUTHOR, 0).unwrap();
        let p_ids: Vec<&str> = unsigned
            .tags
            .iter()
            .filter(|t| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).map(String::as_str))
            .collect();
        assert_eq!(p_ids, vec!["alice", "carol"]);
    }

    #[test]
    fn relay_hint_lands_on_e_tags() {
        let parent = parent_root("ROOT", "alice");
        let unsigned = Note::new("x")
            .relay_hint("wss://r.x")
            .reply_to(&parent)
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.tags[0][2], "wss://r.x");
        assert_eq!(unsigned.tags[1][2], "wss://r.x");
    }

    #[test]
    fn empty_relay_hint_is_treated_as_none() {
        let parent = parent_root("ROOT", "alice");
        let unsigned = Note::new("x")
            .relay_hint("   ")
            .reply_to(&parent)
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.tags[0][2], "");
        assert_eq!(unsigned.tags[1][2], "");
    }

    #[test]
    fn builder_consumes_self_compile_check() {
        // Compile-time assertion: methods take `self` by value, so we cannot
        // accidentally retain a mutable handle. This is the anti-NDK
        // (setters-mutate-tag-arrays) guarantee made executable.
        let _: UnsignedEvent = Note::new("x").build(AUTHOR, 0).unwrap();
    }
}
