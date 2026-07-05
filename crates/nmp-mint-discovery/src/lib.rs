//! `nmp-mint-discovery` — WoT-scoped NIP-87 mint discovery, extracted from
//! `nmp-wallet` into a standalone installable crate (issue #2880, epic
//! #2864).
//!
//! This crate composes `nmp-nip87` (kind:38172 announcement / kind:38000
//! recommendation codecs) with `nmp-wot` (`WotGraph::score_rooted` trust
//! scoring) into a capability-fail-closed, web-of-trust-scoped
//! discovered-mints view, and owns its own read interests and typed
//! `"mint_discovery"` FlatBuffers snapshot projection. It deliberately holds
//! no wallet, backend-selection, or operation-journal state — any app can
//! compose mint discovery on its own, or alongside a wallet, at its own
//! composition root (see `docs/architecture/nip60-nip61-wallet-design.md` in
//! the workspace root for the fuller design rationale).
//!
//! # Composing this crate
//!
//! ```ignore
//! let _mint_discovery = nmp_mint_discovery::register(
//!     app,
//!     nmp_mint_discovery::Config::default(),
//! ).expect("nmp-mint-discovery registration must not collide");
//! ```
//!
//! # Optional `audit` feature
//!
//! Enables `apply_audit`'s companion async `enrich_with_audit` helper, which
//! composes the external `cashu-mint-audit` crate for Cashu-mint-auditor
//! reliability enrichment. See `audit` module docs for the hard D8 boundary:
//! that helper performs real HTTP and must never run inside a registered
//! projection-producer closure.
//!
//! # Optional `mint-info` feature
//!
//! Enables `fetch_mint_info`, this crate's own NUT-06 `/v1/info` pull fetch
//! for a discovered mint's canonical identity (name/icon/description/units/
//! nuts) — see `mint_info_fetch` module docs for why this crate rolls its
//! own instead of depending on `nmp-nip60`/`nmp-wallet`, and the same D8
//! hot-path boundary as `enrich_with_audit`.
// `deny` (not `forbid`) so the generated FlatBuffers bindings module
// `wire/generated/mint_discovery_generated.rs` (`#[path]`-included by
// `projection_wire.rs`) may opt back in via `#[allow(unsafe_code)]` —
// FlatBuffers accessors are intrinsically `unsafe`; `forbid` cannot be
// locally overridden. All hand-written code in this crate remains
// unsafe-free, mirroring `nmp-wallet`'s / `nmp-content`'s own posture.
#![deny(unsafe_code)]

pub mod audit;
pub mod discovery;
pub mod interests;
#[cfg(feature = "mint-info")]
pub mod mint_info_fetch;
pub mod ownership;
pub mod projection_wire;
pub mod register;
pub mod runtime;

pub use audit::{apply_audit, MintAuditRating, MintAuditSummary};
pub use discovery::{
    aggregate, DiscoveredMint, DiscoveryPolicy, MintDiscoveryProjection, MintDiscoveryStore,
    Pubkey, MAX_DISCOVERED_MINTS,
};
pub use interests::{mint_discovery_shape, mint_discovery_trust_graph_shape};
pub use projection_wire::{
    decode_mint_discovery_projection, encode_mint_discovery_projection,
    FILE_IDENTIFIER as MINT_DISCOVERY_FILE_IDENTIFIER, PROJECTION_KEY,
    SCHEMA_ID as MINT_DISCOVERY_SCHEMA_ID, SCHEMA_VERSION as MINT_DISCOVERY_SCHEMA_VERSION,
};
pub use register::{register, Config, Handles};
pub use runtime::MintDiscoveryRuntime;

#[cfg(feature = "audit")]
pub use audit::enrich_with_audit;

#[cfg(feature = "mint-info")]
pub use mint_info_fetch::{fetch_mint_info, MintInfoError, MintNut06Info};
