//! Desktop egui widgets — mirrors the TUI component registry.
//!
//! Each component is a builder-pattern struct rendered into an [`egui::Ui`].
//! Data is sourced from `nmp_gallery_tui` wire types (ContentTreeWire,
//! ProfileWire, ContentRenderData) so the gallery showcases the exact same
//! examples as the TUI and Swift surfaces.

pub mod content_core;
pub mod content_minimal;
pub mod content_view;
pub mod user_avatar;
pub mod user_card;
pub mod user_name;
pub mod user_nip05;
pub mod user_npub;
