//! Flat `TimelineItem` schema for the remaining JSON timeline row.
//!
//! The production modular timeline path is [`crate::TimelineEventCard`]. This
//! struct owns the still-live Swift `TimelineItem` Decodable schema outside
//! `nmp-core`, so the kernel no longer carries a social feed row type just to
//! feed codegen. Issue #920 tracks the staged native-call-site migration off
//! this flat row shape.

use serde::{Deserialize, Serialize};

/// A single item in the current flat timeline/thread JSON view.
///
/// Carries raw protocol data only: pubkeys as hex, timestamps as Unix seconds,
/// and profile/zap facts as optional raw fields. Presentation layers own
/// formatting decisions such as bech32, relative-time labels, and avatar
/// placeholders.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub struct TimelineItem {
    pub id: String,
    /// Author Nostr pubkey, hex (64 chars).
    pub author_pubkey: String,
    /// Author picture URL from kind:0. `None` when no kind:0 has arrived or the
    /// metadata carries no `picture` field.
    pub author_picture_url: Option<String>,
    /// NIP-57 lightning address (`lud16`) or LNURL (`lud06`) from the author's
    /// kind:0 metadata.
    pub author_lnurl: Option<String>,
    /// Author display name from kind:0, if cached.
    pub author_display_name: Option<String>,
    /// Nostr event kind carried as an uninterpreted integer on this row.
    pub kind: u32,
    pub content: String,
    pub content_preview: String,
    /// Event `created_at` (Unix seconds).
    pub created_at: u64,
    pub relay_count: u32,
    pub relay_provenance: Vec<String>,
    /// `true` when this row represents a NIP-18 repost.
    pub is_repost: bool,
    /// Event id the shell should route to when the row is tapped.
    pub nav_target_id: String,
    /// Inner-note text rendered inside a kind:6 repost cell.
    pub repost_inner_content: String,
}
