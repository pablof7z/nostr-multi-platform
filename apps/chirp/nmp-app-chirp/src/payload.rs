//! Wire-shape carried across the Chirp FFI to Swift.
//!
//! `TimelineBlock` is re-exported from `nmp-threading` so Swift's mirror has
//! the same serde shape as the Rust grouper produces. The per-event card
//! (`ChirpEventCard`) is a flat decoder-free struct: Swift renders it
//! directly without consulting any other projection.

use nmp_core::substrate::KernelEvent;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

/// One renderable card per event id. Built from a `KernelEvent` on every
/// `on_kernel_event` fan-out. Fields are deliberately minimal — author
/// display name / picture URL come from the existing kernel `TimelineItem`
/// snapshot on the Swift side (D1 placeholders already in place there). The
/// projection layer doesn't duplicate profile lookup.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChirpEventCard {
    pub id: String,
    pub author_pubkey: String,
    pub kind: u32,
    pub created_at: u64,
    /// Raw kind:1 content. Swift trims / formats as needed; the projection
    /// keeps the full payload so a future reflow doesn't re-fetch.
    pub content: String,
}

impl From<&KernelEvent> for ChirpEventCard {
    fn from(event: &KernelEvent) -> Self {
        Self {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            content: event.content.clone(),
        }
    }
}

/// Complete snapshot Swift consumes via `nmp_app_chirp_snapshot`. Carries
/// the grouper's blocks (ids only) plus the per-id cards so the renderer
/// has everything it needs in one payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChirpTimelineSnapshot {
    pub blocks: Vec<TimelineBlock>,
    pub cards: Vec<ChirpEventCard>,
}

impl ChirpTimelineSnapshot {
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            cards: Vec::new(),
        }
    }
}
