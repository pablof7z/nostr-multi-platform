//! `nmp-nip42-types` — dependency-free NIP-42 wire/type substrate.
//!
//! NIP-42 (relay AUTH) has two implementations in this workspace for a
//! deliberate reason: the kernel inlines the handshake FSM
//! (`nmp_core::kernel::auth`) because the AUTH path is tightly coupled to
//! the relay socket, while `nmp-nip42` is the standalone protocol module
//! downstream consumers and isolated tests use. `nmp-core` cannot depend on
//! `nmp-nip42` (it would form a Cargo cycle), which historically forced the
//! shared vocabulary — the [`RelayAuthState`] lifecycle enum, the
//! `["AUTH", _]` / `["OK", …]` frame shapes, and their parsers — to be
//! duplicated on both sides with hand-maintained "keep these in sync"
//! comments.
//!
//! This crate is that shared vocabulary, extracted once. It depends on
//! **nothing in the workspace** (serde / serde_json only), so both
//! `nmp-core` and `nmp-nip42` depend on it with no cycle. The FSM drivers
//! and the kind:22242 builder stay where they are — they need
//! `nmp_core::substrate::{SignedEvent, UnsignedEvent}`, which this crate
//! must not see.
//!
//! ## Doctrine
//!
//! - **D0** — protocol primitives only (lifecycle state + frame shapes), no
//!   app nouns.
//! - **D6** — parsers return `Option`, never panic; no `Result` surface.

mod frame;
mod state;

pub use frame::{parse_auth_frame, parse_ok_frame, AuthChallenge, AuthOk};
pub use state::RelayAuthState;
