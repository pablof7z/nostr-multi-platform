//! Shared helpers across `tests/*.rs` integration tests in `nmp-testing`.
//!
//! Today this is just the NIP-46 mock-bunker relay used by
//! `nip46_bunker_signing.rs`. Future shared fixtures (mock content relay,
//! etc.) belong here too — cargo treats `tests/common/mod.rs` as a non-test
//! source file even when sibling files are integration tests.

#![allow(dead_code)]

pub mod mock_bunker_relay;
