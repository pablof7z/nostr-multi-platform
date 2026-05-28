//! [`Nip10ReplyAttribution`] — the NIP-10 instance of the engine's
//! [`AttributionPayload`] trait.
//!
//! This is the concrete payload the generic `RootIndexedFeed` engine attaches
//! to a thread root when a *followed* author posts a NIP-10 reply that
//! references it. The engine stays protocol-agnostic; this type supplies the
//! NIP-10 qualification rules (`from_reply`), the keying accessors, and the
//! in-place profile refresh.
//!
//! # Display separation (2026-05-25 doctrine)
//!
//! The payload carries **raw protocol data only** — a raw hex pubkey, the raw
//! reply event id, the raw `created_at` (Unix seconds), and the kind:0
//! display-name / picture-url *mirrors* as `Option<String>` (None until a
//! kind:0 arrives). No `display::` formatting helper is invoked here: the
//! render surface formats the missing-name case itself (typically by
//! formatting the raw pubkey). The nested [`AuthorDisplay`] is the same raw
//! mirror the sibling `TimelineEventCard` exposes — it carries `npub`
//! (pubkey-deterministic) plus the optional kind:0 fields, never a
//! presentation decision.

use nmp_core::substrate::KernelEvent;
use nmp_core::tags::parse_nip10;
use nmp_feed::AttributionPayload;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;
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
    /// arrives). Carries the deterministic `npub` plus optional name/picture.
    pub author_display: AuthorDisplay,
    /// Flat mirror of `author_display.name` for shells that want the display
    /// name without decoding the nested `AuthorDisplay`. `None` until a kind:0
    /// arrives — the render surface formats the raw pubkey itself.
    pub author_display_name: Option<String>,
    /// Author's kind:0 picture URL. `None` until a kind:0 arrives, or when the
    /// kind:0 omits `picture`.
    pub author_picture_url: Option<String>,
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
        if reply.kind != KIND_SHORT_NOTE {
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
            author_display_name: author_display.name.clone(),
            author_picture_url: author_display.picture_url.clone(),
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

    /// Refresh the display mirrors in place when a newer kind:0 for this
    /// author arrives. Mirrors `ModularTimelineProjection::refresh_author_cards`
    /// (V-27 / V-32 thin-shell): rebuild `author_display` from the profile and
    /// keep the flat `author_display_name` / `author_picture_url` mirrors in
    /// sync. Never mutates the keying fields (`reply_event_id`,
    /// `author_pubkey`).
    fn refresh_for_profile(&mut self, profile: &Self::Profile) {
        let refreshed = AuthorDisplay::from_profile(&self.author_pubkey, Some(profile));
        self.author_display_name = refreshed.name.clone();
        self.author_picture_url = refreshed.picture_url.clone();
        self.author_display = refreshed;
    }
}
