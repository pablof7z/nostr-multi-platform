//! [`Nip10ReplyAttribution`] — the NIP-10 instance of the engine's
//! [`AttributionPayload`] trait.
//!
//! This is the concrete payload the generic `RootIndexedFeed` engine attaches
//! to a thread root when a *followed* author posts a NIP-10 reply that
//! references it. The engine stays protocol-agnostic; this type supplies the
//! NIP-10 qualification rules (`from_reply`), the keying accessors, and the
//! in-place profile refresh.
//!
//! # Display separation (aim.md §2)
//!
//! The payload carries **raw protocol data only** — a raw hex pubkey, the raw
//! reply event id, the raw `created_at` (Unix seconds), and the kind:0
//! display-name / picture-url via the nested [`AuthorDisplay`] (None until a
//! kind:0 arrives). No `display::` formatting helper is invoked here: the
//! render surface formats the missing-name case itself (typically by
//! formatting the raw pubkey). The flat `author_display_name` /
//! `author_picture_url` mirrors that previously duplicated `author_display`
//! fields have been removed (ADR-0032 / #1493 P1).

use nmp_core::substrate::KernelEvent;
use nmp_core::tags::parse_nip10;
use nmp_feed::AttributionPayload;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_TEXT_NOTE;
use crate::profile_display::{AuthorDisplay, ProfileDisplay};

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
    /// Raw mirror of the author's kind:0 display fields (None until a kind:0
    /// arrives). Carries the optional name/picture. Shells read name/picture
    /// from this nested table directly.
    pub author_display: AuthorDisplay,
    /// Raw event id of the reply this attribution was built from.
    pub reply_event_id: String,
    /// Raw signed `created_at` of the reply (Unix seconds). Eviction ordering.
    pub reply_created_at: u64,
}

impl AttributionPayload for Nip10ReplyAttribution {
    type Profile = ProfileDisplay;

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
    /// The profile mirrors are filled best-effort from `profile_for`; a `None`
    /// profile yields the fallback `AuthorDisplay` (npub only), refreshed later
    /// via [`Self::refresh_for_profile`] when the kind:0 lands.
    fn from_reply(
        reply: &KernelEvent,
        follow: &dyn Fn(&str) -> bool,
        profile_for: &dyn Fn(&str) -> Option<Self::Profile>,
    ) -> Option<Self> {
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
        let profile = profile_for(&reply.author);
        let author_display = AuthorDisplay::from_profile(&reply.author, profile.as_ref());
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

    fn author_pubkey(&self) -> &str {
        &self.author_pubkey
    }

    fn reply_created_at(&self) -> u64 {
        self.reply_created_at
    }

    /// Refresh `author_display` in place when a newer kind:0 for this author
    /// arrives. Never mutates the keying fields (`reply_event_id`,
    /// `author_pubkey`).
    fn refresh_for_profile(&mut self, profile: &Self::Profile) {
        self.author_display = AuthorDisplay::from_profile(&self.author_pubkey, Some(profile));
    }
}
