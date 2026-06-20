//! `nmp-app-chirp-web` — wasm32 composition root for the Chirp web client.
//!
//! This crate wires the OP-centric home feed (`nmp.feed.home`) into
//! [`nmp_wasm::WasmRuntime`]. It is the web twin of
//! `apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs`: both composition roots
//! register the feed engine as a kernel event observer and expose the typed
//! `nmp.feed.home` projection.
//!
//! # Crate layout
//!
//! * [`composition`] — `setup_chirp_web_feeds` wires everything together:
//!   creates the `ActiveFollowSet`, builds the engine via `register_op_feed`,
//!   registers the engine as a `KernelEventObserver`, registers the typed
//!   `nmp.feed.home` snapshot projection, and resets the feed when the active
//!   follow-set perspective changes.

pub mod composition;
// wasm32 composition-root entry point. Compiled only for the wasm32 target so
// `wasm-bindgen` glue is never emitted for native builds or test binaries.
#[cfg(target_arch = "wasm32")]
pub mod wasm_binding;

pub use composition::{setup_chirp_web_feeds, ChirpWebFeedSetup};
