#![cfg(test)]
//! Shared test helpers for `remote_signer_tests`.
//!
//! `fresh()` and `stub_signer()` live here so that doctrine-lint's
//! `file_is_test_only` check exempts them via the `_tests.rs` suffix.
//! `mod.rs` re-exports both so sibling test modules can still reach them
//! via the normal `use super::{fresh, stub_signer}` path.

use std::sync::{atomic::AtomicU32, Arc};

use nostr::{Keys, SecretKey};
use nostr::nips::nip19::FromBech32;

use crate::actor::commands::identity::IdentityRuntime;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{StubRemoteSigner, TEST_NSEC};

pub(crate) fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            super::new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

pub(crate) fn stub_signer() -> (Box<StubRemoteSigner>, Arc<AtomicU32>) {
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let stub = StubRemoteSigner::new(keys);
    let count = stub.sign_count_handle();
    (Box::new(stub), count)
}
