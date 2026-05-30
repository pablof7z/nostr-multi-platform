//! KindRenderer trait for TUI kind-dispatched content rendering (F-CR-06).
//!
//! See ADR-0034 for the cross-platform projection contract.

use ratatui::{buffer::Buffer, layout::Rect};
use std::sync::Arc;

use nmp_content::context::RenderContext;
use nmp_content::embed_projection::EmbedKindProjection;
use nmp_core::display::short_npub;

use super::NostrKindRegistry;
use crate::nostr_mention_chip::NostrMentionProfileHost;

/// Resolve the author byline for an embed, component-owned (mirrors iOS #833).
///
/// Component-owned claiming: the renderer that *displays* an author's name is
/// the component that claims that author's kind:0. NO event triggers a kernel
/// kind:0 fetch of the author — fetching kind:0 is always the presentation
/// layer's concern. The displaying component issues the claim itself (no
/// separate hidden trigger) and reads the live-resolved name.
///
/// Reuses [`NostrMentionProfileHost`] — the same presentation-owned profile
/// host the mention chip and `NostrContentView` already thread through render,
/// rather than a parallel byline-only abstraction. With no host (preview-only
/// callers) it falls back to a Rust-formatted `npub_short`. In neither case
/// does the byline depend on the static `author_display_name` projection field;
/// the kernel still emits that field for now, but it is no longer the source.
pub(crate) fn author_byline(
    host: Option<&dyn NostrMentionProfileHost>,
    consumer_id: Option<&str>,
    author_pubkey: &str,
) -> String {
    if let (Some(host), Some(consumer_id)) = (host, consumer_id) {
        // The displaying component owns the claim — no separate hidden trigger.
        host.claim_profile(author_pubkey, consumer_id);
        if let Some(name) = host
            .profile_for_pubkey(author_pubkey)
            .and_then(|profile| profile.display_name)
        {
            return name;
        }
    }
    // Rust-formatted npub_short fallback (never hex, never a non-Rust
    // abbreviation), matching the user-* components' identity rule.
    short_npub(author_pubkey)
}

/// Trait for a renderer of one specific `EmbedKindProjection` variant (or
/// a group of unknown kinds).
pub trait KindRenderer: Send + Sync {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        ctx: &RenderContext,
        registry: &NostrKindRegistry,
        author_host: Option<&dyn NostrMentionProfileHost>,
        consumer_id: Option<&str>,
        area: Rect,
        buf: &mut Buffer,
    );

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16;
}

pub type KindRendererRef = Arc<dyn KindRenderer>;
