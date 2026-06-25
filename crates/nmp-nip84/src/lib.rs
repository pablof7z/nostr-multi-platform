//! `nmp-nip84` - NIP-84 kind:9802 highlight publishing.
//!
//! This crate owns the app-neutral write path for public NIP-84 highlights.
//! It builds unsigned kind:9802 events from typed protocol inputs and routes
//! them through the normal action/publish engine. Group-scoped `h`-tagged
//! highlights remain owned by `nmp-nip29`; app-specific clip selection,
//! playback state, and Highlighter policy stay in app crates.

mod action;
mod wire;

pub use action::{
    build_highlight_event, HighlightAttribution, HighlightSource, Nip84Descriptor,
    PublishHighlightCommand, PublishHighlightInput, PublishHighlightModule,
    PUBLISH_HIGHLIGHT_NAMESPACE,
};
pub use nmp_kinds::KIND_HIGHLIGHT;
