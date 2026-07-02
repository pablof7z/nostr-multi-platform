//! # nmp-content-fixtures
//!
//! Offline signed-event + pre-tokenized DTO bundle generator for the
//! **NmpGallery** content showcase (the NMP analogue of NDKSwift's
//! content-component demo app).
//!
//! ## Why a pre-tokenized bundle
//!
//! `nmp_content::Segment` / `ContentTree` deliberately do **not** derive
//! serde, and there is no live `ContentTree` FFI projection (T93
//! "ContentTree FFI ADR" is in flight, not landed). This crate **consumes**
//! `nmp-content` / `nmp-nip23` unchanged: it runs the real
//! tokenizer + NIP-23 decode + recursion guard on each fixture and projects
//! the result to a serde-derivable `ContentTreeDto` (see [`dto`] +
//! [`project`]). The Rust side does all real content work; only the
//! cross-language transport changes. The DTO schema is the candidate shape
//! for T93 to canonicalize (documented in
//! `docs/design/content-gallery-scenarios.md` §6).
//!
//! ## Output
//!
//! [`build_bundle`] returns a [`dto::Bundle`]. The original iOS-facing
//! `build-bundle` binary (which serialized it to
//! `ios/NmpGallery/Resources/content-gallery-bundle.json`) was deleted once
//! the NmpGallery iOS showcase moved to hand-authored `ContentTreeWire`
//! fixtures (see `apps/nmp-gallery/ios/NmpGallery/Gallery/ContentComponentPages.swift`)
//! and no longer consumes a generated bundle. [`build_bundle`] itself is
//! retained as the spec-as-code fixture generator exercised directly by
//! `tests/bundle.rs`, and the sibling `build-android-gallery-bundle` binary
//! still emits the analogous bundle for the Android gallery module.
//!
//! Every event is signed in-process with deterministic test keys via BIP340
//! schnorr **without auxiliary randomness** (so signatures are byte-stable
//! across runs) and is **never** published to a relay.

pub mod dto;
pub mod embed_store;
pub mod identities;
pub mod project;
pub mod scenarios;
pub mod wire_fixtures;

use dto::Bundle;
use identities::Identities;

/// Bundle schema version.
pub const BUNDLE_VERSION: u32 = 1;

/// Build the full showcase bundle: every scenario tokenized through the
/// real `nmp-content` / `nmp-nip23` path, every embed resolved through the
/// real `RenderContext` recursion guard, every event really signed.
pub fn build_bundle() -> Bundle {
    let ids = Identities::new();
    let mut scenarios = Vec::new();
    scenarios.extend(scenarios::text::build(&ids));
    scenarios.extend(scenarios::mentions::build(&ids));
    scenarios.extend(scenarios::quotes::build(&ids));
    scenarios.extend(scenarios::highlights::build(&ids));
    scenarios.extend(scenarios::lists::build(&ids));
    scenarios.extend(scenarios::media::build(&ids));
    scenarios.extend(scenarios::links::build(&ids));
    scenarios.extend(scenarios::hashtags::build(&ids));
    scenarios.extend(scenarios::edge::build(&ids));
    Bundle {
        version: BUNDLE_VERSION,
        scenarios,
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
