//! Shared helpers across `tests/*.rs` integration tests in `nmp-testing`.
//!
//! - `mock_bunker_relay` — NIP-46 mock-bunker relay (bunker:// direction).
//! - `mock_nostrconnect_signer` — NIP-46 mock signer-app (nostrconnect://
//!   direction; Phase 2).
//!
//! cargo treats `tests/common/mod.rs` as a non-test source file even when
//! sibling files are integration tests.

#![allow(dead_code)]

pub mod mock_bunker_relay;
pub mod mock_nostrconnect_signer;
