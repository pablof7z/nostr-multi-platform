//! KindRenderer trait for TUI kind-dispatched content rendering (F-CR-06).
//!
//! See ADR-0034 for the cross-platform projection contract.

use ratatui::{buffer::Buffer, layout::Rect};
use std::sync::Arc;

use nmp_content::context::RenderContext;
use nmp_content::embed_projection::EmbedKindProjection;

use super::NostrKindRegistry;

/// Trait for a renderer of one specific `EmbedKindProjection` variant (or
/// a group of unknown kinds).
pub trait KindRenderer: Send + Sync {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        ctx: &RenderContext,
        registry: &NostrKindRegistry,
        area: Rect,
        buf: &mut Buffer,
    );

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16;
}

pub type KindRendererRef = Arc<dyn KindRenderer>;
