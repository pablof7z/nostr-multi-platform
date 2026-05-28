//! Desktop component gallery — reusable iced widgets for Nostr UI surfaces.
//!
//! This crate is the egui analogue of `nmp-gallery-tui`: it renders sample
//! profiles and content with the actual components a production desktop app
//! would use. Data is static (no kernel in-process) so the gallery loads
//! instantly and every component state is deterministic.

pub mod components;
pub mod gallery;
