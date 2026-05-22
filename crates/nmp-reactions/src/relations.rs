//! Cross-NIP **Relations facade** — the applesauce-shape "give me everything
//! about this event" ergonomic, refactored into NMP idiom.
//!
//! Apps repeatedly ask the same questions of an event: "what reactions does
//! it have", "what's the reply chain", "how many sats has it been zapped",
//! "what are the comments on it", and on the write side "give me a reply",
//! "give me a reaction", "give me a repost", "give me a zap-request", "give
//! me a comment". Without a facade, each app re-wires four view specs and
//! reaches into four crates for the four builder entrypoints. This module
//! collapses that boilerplate into:
//!
//! - [`Relations::for_event`] returning a [`RelationSpecs`] bundle of
//!   pre-wired view spec values from `nmp-relations` (reactions summary +
//!   reposts), `nmp-nip01` (replies + thread), `nmp-nip22` (comments), and
//!   `nmp-nip57` (zaps).
//! - [`Relations::reply_to`], [`Relations::react_to`], [`Relations::repost`],
//!   [`Relations::zap_request`], [`Relations::comment_on`] — free-fn
//!   entrypoints into the per-NIP builders.
//!
//! ## Not NDK's `wrap.ts` registry
//!
//! `kind-wrappers.md` §9 anti-pattern #4 forbids a "centralized wrapper
//! registry" that forces every app to compile every wrapper. This module is
//! **not** that:
//!
//! - It is **pure free-fn composition** — no runtime registration, no type
//!   erasure, no module list.
//! - It holds **no state** — no store ref, no signer, no clock; every fn is
//!   a pure transform.
//! - It is **opt-in** at the cargo dep level — apps that don't want it don't
//!   depend on `nmp-relations`. This crate is the cross-NIP composition
//!   layer: the facade lives here alongside the relation kinds it composes,
//!   which is why the package is named `nmp-relations`.
//!
//! ## Mechanism, not magic
//!
//! Apps still open views and build events through the existing substrate.
//! The facade does the small boring task of picking the right `Spec` shape
//! per kind and forwarding builder constructors, so app code reads as
//! `Relations::react_to(&note)` instead of
//! `nmp_relations::Reaction::to_event(note.event_id.clone(), note.author.clone())`.

use nmp_nip01::{Note, NoteBuilder, NoteRecord, RepliesSpec, ThreadSpec};
use nmp_nip22::{Comment, CommentBuilder, CommentsSpec};
use nmp_nip57::{ZapRequest, ZapRequestBuilder, ZapsSpec};

use crate::build::{Reaction, ReactionBuilder, Repost, RepostBuilder};
use crate::decode::ReactionTarget;
use crate::view::{ReactionSummarySpec, RepostsSpec};

/// A bundle of pre-wired view specs targeting a single event.
///
/// Apps open whichever of these are relevant for the surface they're
/// rendering. `replies` and `thread` are populated only when the event kind
/// is a short text note (kind 1) — for other kinds those slots are `None`
/// because the views would have nothing to find.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationSpecs {
    /// NIP-25 reaction summary (emoji counts) for the event.
    pub reactions: ReactionSummarySpec,
    /// NIP-18 reposts of the event.
    pub reposts: RepostsSpec,
    /// NIP-57 zap aggregate (total msats, zappers) for the event.
    pub zaps: ZapsSpec,
    /// NIP-22 standalone comments on the event.
    pub comments: CommentsSpec,
    /// Direct replies — populated for kind:1 notes only.
    pub replies: Option<RepliesSpec>,
    /// Thread tree rooted at this event — populated for kind:1 notes only.
    pub thread: Option<ThreadSpec>,
}

/// Entry-point namespace. `Relations` has no fields — like [`Note`] /
/// [`Reaction`] / [`Comment`] / [`ZapRequest`], it exists purely as a
/// namespace for the entry points.
pub struct Relations;

impl Relations {
    /// Build a bundle of view specs pointed at `event_id`. `kind` controls
    /// the optional `replies`/`thread` slots — see [`RelationSpecs`].
    pub fn for_event(event_id: impl Into<String>, kind: u32) -> RelationSpecs {
        let id: String = event_id.into();
        let target = ReactionTarget::Event(id.clone());
        let replies = (kind == 1).then(|| RepliesSpec { target: id.clone() });
        let thread = (kind == 1).then(|| ThreadSpec { root_event: id.clone() });
        RelationSpecs {
            reactions: ReactionSummarySpec { target: target.clone() },
            reposts: RepostsSpec::OfTarget(target),
            zaps: ZapsSpec { target: id.clone() },
            comments: CommentsSpec { target: id },
            replies,
            thread,
        }
    }

    /// Start building a NIP-10 reply to `parent`. Forwards to
    /// [`Note::new`] + [`NoteBuilder::reply_to`] from `nmp-nip01`.
    pub fn reply_to(parent: &NoteRecord, content: impl Into<String>) -> NoteBuilder {
        Note::new(content).reply_to(parent)
    }

    /// Start a NIP-25 reaction targeting `target.event_id` (with the
    /// target's author for the `p` tag). Forwards to
    /// [`Reaction::to_event`].
    pub fn react_to(target: &NoteRecord) -> ReactionBuilder {
        Reaction::to_event(target.event_id.clone(), target.author.clone())
    }

    /// Start a NIP-18 repost of a kind-1 note. Forwards to [`Repost::of`].
    /// For non-kind-1 events use the lower-level
    /// [`crate::GenericRepost::of`] which takes the reposted kind
    /// explicitly.
    pub fn repost(target: &NoteRecord) -> RepostBuilder {
        Repost::of(target.event_id.clone(), target.author.clone())
    }

    /// Start a NIP-57 zap-request targeting the author of `target`. Caller
    /// must still set amount + relays + (optionally) the zapped event id
    /// before `.build(...)`.
    pub fn zap_request(target: &NoteRecord) -> ZapRequestBuilder {
        ZapRequest::to_pubkey(target.author.clone()).zapped_event(target.event_id.clone())
    }

    /// Start a NIP-22 comment on `target` (top-level — root == parent).
    /// Forwards to [`Comment::on_event`]. For comments nested under a
    /// parent comment, chain [`CommentBuilder::reply_to_comment`].
    pub fn comment_on(target: &NoteRecord) -> CommentBuilder {
        // `NoteRecord` only ever represents a kind-1 short text note — the
        // root kind is a compile-time constant from `nmp-nip01`, never a
        // runtime field on the record.
        Comment::on_event(
            target.event_id.clone(),
            nmp_nip01::KIND_SHORT_NOTE,
            target.author.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::UnsignedEvent;
    use nmp_core::tags::Nip10Refs;

    const AUTHOR: &str = "deadbeef";

    fn note_record(id: &str, author: &str) -> NoteRecord {
        NoteRecord {
            event_id: id.into(),
            author: author.into(),
            created_at: 0,
            content: "x".into(),
            refs: Nip10Refs::default(),
        }
    }

    // ── for_event ──────────────────────────────────────────────────────────

    #[test]
    fn for_event_kind_1_populates_replies_and_thread() {
        let specs = Relations::for_event("EID", 1);
        assert!(specs.replies.is_some());
        assert!(specs.thread.is_some());
        assert_eq!(specs.replies.as_ref().unwrap().target, "EID");
        assert_eq!(specs.thread.as_ref().unwrap().root_event, "EID");
    }

    #[test]
    fn for_event_non_kind_1_omits_replies_and_thread() {
        let specs = Relations::for_event("EID", 30023);
        assert!(specs.replies.is_none());
        assert!(specs.thread.is_none());
    }

    #[test]
    fn for_event_wires_reactions_reposts_zaps_comments_to_same_target() {
        let specs = Relations::for_event("EID", 1);
        assert_eq!(specs.zaps.target, "EID");
        assert_eq!(specs.comments.target, "EID");
        match &specs.reactions.target {
            ReactionTarget::Event(id) => assert_eq!(id, "EID"),
            _ => panic!("expected Event variant"),
        }
        match &specs.reposts {
            RepostsSpec::OfTarget(ReactionTarget::Event(id)) => assert_eq!(id, "EID"),
            _ => panic!("expected OfTarget(Event(_))"),
        }
    }

    // ── builders forwarding ────────────────────────────────────────────────

    #[test]
    fn reply_to_builds_a_nip10_reply() {
        let parent = note_record("ROOT", "alice");
        let unsigned: UnsignedEvent = Relations::reply_to(&parent, "hi")
            .build(AUTHOR, 0)
            .unwrap();
        // Two e tags (root + reply markers) + one p tag — see nmp-nip01 tests.
        let keys: Vec<&str> = unsigned.tags.iter().filter_map(|t| t.first()).map(String::as_str).collect();
        assert_eq!(keys, vec!["e", "e", "p"]);
        assert_eq!(unsigned.tags[2][1], "alice");
        assert_eq!(unsigned.content, "hi");
    }

    #[test]
    fn react_to_builds_a_kind_7() {
        let target = note_record("NID", "alice");
        let unsigned: UnsignedEvent = Relations::react_to(&target).build(AUTHOR, 0).unwrap();
        assert_eq!(unsigned.kind, 7);
        // First tag is the e-tag pointing at the target.
        assert_eq!(unsigned.tags[0][0], "e");
        assert_eq!(unsigned.tags[0][1], "NID");
    }

    #[test]
    fn repost_builds_a_kind_6() {
        let target = note_record("NID", "alice");
        let unsigned: UnsignedEvent = Relations::repost(&target).build(AUTHOR, 0).unwrap();
        assert_eq!(unsigned.kind, 6);
        assert_eq!(unsigned.tags[0][1], "NID");
    }

    #[test]
    fn zap_request_pre_wires_recipient_and_zapped_event() {
        let target = note_record("NID", "alice");
        let unsigned: UnsignedEvent = Relations::zap_request(&target)
            .amount_msats(21_000)
            .relays(vec!["wss://r".into()])
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.kind, 9734);
        // p tag must point at the target author; e tag at the zapped event.
        let p_idx = unsigned.tags.iter().position(|t| t[0] == "p").unwrap();
        assert_eq!(unsigned.tags[p_idx][1], "alice");
        let e_idx = unsigned.tags.iter().position(|t| t[0] == "e").unwrap();
        assert_eq!(unsigned.tags[e_idx][1], "NID");
    }

    #[test]
    fn comment_on_builds_a_kind_1111_with_kind_1_root() {
        let target = note_record("NID", "alice");
        let unsigned: UnsignedEvent = Relations::comment_on(&target)
            .content("nice")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.kind, 1111);
        // Uppercase E + K=1 + P=alice; lowercase mirror for parent==root.
        let upper_e = unsigned.tags.iter().find(|t| t[0] == "E").unwrap();
        assert_eq!(upper_e[1], "NID");
        let upper_k = unsigned.tags.iter().find(|t| t[0] == "K").unwrap();
        assert_eq!(upper_k[1], "1");
        let upper_p = unsigned.tags.iter().find(|t| t[0] == "P").unwrap();
        assert_eq!(upper_p[1], "alice");
    }
}
