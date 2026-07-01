//! TUI content-kind-registry component (F-CR-06).

pub mod embed_chrome_container;
pub mod embedded_event;
pub mod kind_renderer;
pub mod nostr_kind_registry;

pub use embedded_event::EmbeddedEvent;
pub use kind_renderer::{KindRenderer, KindRendererRef};
pub use nostr_kind_registry::{
    DefaultArticleRenderer, DefaultHighlightRenderer, DefaultProfileRenderer,
    DefaultShortNoteRenderer, DefaultUnknownRenderer, NostrKindRegistry,
};
