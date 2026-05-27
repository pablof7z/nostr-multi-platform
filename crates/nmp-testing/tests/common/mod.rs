//! Shared helpers across `tests/*.rs` integration tests in `nmp-testing`.
//!
//! - `mock_bunker_relay` — NIP-46 mock-bunker relay (bunker:// direction).
//! - `mock_nostrconnect_signer` — NIP-46 mock signer-app (nostrconnect://
//!   direction; Phase 2).
//! - `broker_adapter` — test-only translation from app-neutral broker events
//!   into actor commands.
//! - `wire_log` — stderr FD-pipe capture for `NMP_CLAIM_LOG` structured JSON
//!   lines (W9 relay-search-radius acceptance tests).
//! - `stub_relay` — TCP stub relay that drops connections after a configurable
//!   delay (A5 mid-claim unreachable test).
//!
//! cargo treats `tests/common/mod.rs` as a non-test source file even when
//! sibling files are integration tests.

#![allow(dead_code)]

pub mod broker_adapter;
pub mod mock_bunker_relay;
pub mod mock_nostrconnect_signer;
pub mod stub_relay;
pub mod wire_log;
