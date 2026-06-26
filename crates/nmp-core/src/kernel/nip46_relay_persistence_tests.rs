//! NIP-46 relay-persistence contract tests.
//!
//! Verifies that `RelayRole::Signer` is classified as persistent by
//! `relay_socket_is_persistent` — the make-or-break relay-lifetime invariant:
//! the bunker relay socket MUST never be reaped by the idle sweeper between
//! NIP-46 RPC calls.  This mirrors the `RelayRole::Wallet` contract for NWC.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const BUNKER_URL: &str = "wss://bunker.relay.example";

/// `RelayRole::Signer` must always be classified as persistent, regardless of
/// whether the bunker URL appears in bootstrap or configured relays.
#[test]
fn signer_role_is_always_persistent() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let canonical = crate::kernel::CanonicalRelayUrl::parse_or_raw(BUNKER_URL);
    assert!(
        kernel.relay_socket_is_persistent(&canonical, RelayRole::Signer),
        "RelayRole::Signer must be classified as persistent so the idle sweeper \
         never reaps the bunker socket between RPC calls"
    );
}

/// `RelayRole::Wallet` must remain persistent (regression guard — adding
/// `Signer` must not break the existing NWC contract).
#[test]
fn wallet_role_remains_persistent_after_signer_addition() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let canonical = crate::kernel::CanonicalRelayUrl::parse_or_raw(BUNKER_URL);
    assert!(
        kernel.relay_socket_is_persistent(&canonical, RelayRole::Wallet),
        "RelayRole::Wallet must still be classified as persistent \
         (regression: adding Signer role must not break NWC)"
    );
}

/// `RelayRole::Content` must NOT be classified as persistent for an arbitrary
/// URL not in bootstrap / configured relays (regression guard — the default
/// path must still work correctly).
#[test]
fn content_role_is_not_persistent_for_unknown_url() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let canonical = crate::kernel::CanonicalRelayUrl::parse_or_raw(BUNKER_URL);
    assert!(
        !kernel.relay_socket_is_persistent(&canonical, RelayRole::Content),
        "RelayRole::Content must NOT be persistent for an arbitrary URL \
         that is not in bootstrap / configured relays"
    );
}

/// `RelayRole::Signer` is excluded from `all()` — it must not gate startup or
/// appear in the standard relay-statuses projection.
#[test]
fn signer_role_excluded_from_all() {
    let all = RelayRole::all();
    assert!(
        !all.contains(&RelayRole::Signer),
        "RelayRole::Signer must NOT be in all() — it spawns on demand, \
         not at startup (same as RelayRole::Wallet)"
    );
}
