//! [`Nip10ReplyAttribution`] — the NIP-10 instance of the engine's
//! [`AttributionPayload`] trait.
//!
//! This is the concrete payload the generic `RootIndexedFeed` engine attaches
//! to a thread root when a *followed* author posts a NIP-10 reply that
//! references it. The engine stays protocol-agnostic; this type supplies the
//! NIP-10 qualification rules (`from_reply`), the keying accessors, and the
//! raw attribution metadata.
//!
//! # Display separation
//!
//! The payload carries the replying author's raw pubkey and reply metadata.
//! kind:0 profile display is intentionally absent from this feed payload:
//! mounted profile/avatar components claim and render profile data through
//! their own dependency path.

use nmp_core::substrate::KernelEvent;
use nmp_core::tags::parse_nip10;
use nmp_feed::AttributionPayload;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_TEXT_NOTE;
use crate::profile_display::AuthorDisplay;

/// Per-root attribution for a followed author's NIP-10 reply.
///
/// Built by [`AttributionPayload::from_reply`] only when the referencing event
/// is a kind:1 reply authored by a followed pubkey. The engine de-dupes on
/// [`Self::reply_event_id`] (the per-root sub-map key) and evicts the oldest by
/// [`Self::reply_created_at`] under D5 pressure.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Nip10ReplyAttribution {
    /// Raw hex pubkey of the replying (followed) author.
    pub author_pubkey: String,
    /// Always the fallback raw-author display. Profile components render
    /// kind:0-derived name/picture independently when mounted.
    pub author_display: AuthorDisplay,
    /// Raw event id of the reply this attribution was built from.
    pub reply_event_id: String,
    /// Raw signed `created_at` of the reply (Unix seconds). Eviction ordering.
    pub reply_created_at: u64,
}

impl AttributionPayload for Nip10ReplyAttribution {
    /// Build attribution from a referencing event, or `None` when it does not
    /// qualify as a NIP-10 reply from a followed author.
    ///
    /// Qualification chain (all must hold):
    /// 1. `event.kind == 1` (short text note — reposts/reactions are not
    ///    attribution: a kind:6 is handled by the engine's repost arm, never
    ///    this path);
    /// 2. `follow(author)` is true (the engine also gates on follow before
    ///    calling, so this is a fail-closed re-check per the trait contract);
    /// 3. the event carries a NIP-10 reply marker (`Nip10Refs::is_reply`).
    ///
    fn from_reply(reply: &KernelEvent, follow: &dyn Fn(&str) -> bool) -> Option<Self> {
        if reply.kind != KIND_SHORT_TEXT_NOTE {
            return None;
        }
        if !follow(&reply.author) {
            return None;
        }
        let refs = parse_nip10(&reply.tags);
        if !refs.is_reply() {
            return None;
        }
        let author_display = AuthorDisplay::fallback(&reply.author);
        Some(Self {
            author_pubkey: reply.author.clone(),
            author_display,
            reply_event_id: reply.id.clone(),
            reply_created_at: reply.created_at,
        })
    }

    fn reply_event_id(&self) -> &str {
        &self.reply_event_id
    }
}

impl nmp_feed::AttributionAuthors for Nip10ReplyAttribution {
    /// ADR-0063 D7 — the replying (followed) author this attribution RENDERS.
    /// A mounted avatar shows this pubkey, so the kernel must auto-resolve it.
    fn rendered_author_key(&self) -> String {
        self.author_pubkey.clone()
    }
}
