//! `nmp-nip92-types` - dependency-light NIP-92 `imeta` wire/type substrate.
//!
//! NIP-92 `imeta` tags are media metadata carried by multiple media-oriented
//! NIPs. Keeping the shared tag vocabulary here lets protocol crates such as
//! `nmp-nip68` and future video/file wrappers reuse one parser/builder without
//! depending on each other.
//!
//! This crate depends on nothing in the workspace. It owns only the NIP-92 tag
//! shape and generic media metadata. Event kinds, image-only MIME policy,
//! publishing, relay routing, upload, and app behavior stay in the consuming
//! protocol/app crates; `nmp-core` gains zero NIP-92 nouns.
//!
//! ## Doctrine
//!
//! - **D0** - protocol wire primitives only, no app nouns and no kernel nouns.
//! - **D4** - one parser/builder owns the `imeta` tag representation.
//! - **D6** - parsers return `Option`, never panic and never expose FFI errors.

mod imeta;

pub use imeta::{parse_imeta_tag, ImetaField, MediaDimensions, MediaMeta};

#[cfg(test)]
mod tests;
