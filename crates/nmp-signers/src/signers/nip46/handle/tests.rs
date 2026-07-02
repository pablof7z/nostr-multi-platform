//! `RemoteSignerHandle` test suite for `Nip46Signer`, split by behavior
//! area. Shared fixtures live in [`tests::fixtures`](fixtures).

#[path = "tests/fixtures.rs"]
mod fixtures;

/// Core inbound-response lifecycle: `pubkey_hex`, `signer_kind`,
/// `deliver_response`, `disconnect`.
#[path = "tests/response_delivery_tests.rs"]
mod response_delivery_tests;

/// `enqueue`'s transmit-failure branch: a failed `send_rpc` must surface as
/// an error and never leak a pending entry.
#[path = "tests/transport_failure_tests.rs"]
mod transport_failure_tests;

/// `Nip04`/`Nip44` RPC enqueue shape and round-trip via `resolve_response`.
#[path = "tests/encryption_rpc_tests.rs"]
mod encryption_rpc_tests;

/// `RemoteSignerHandle::nip44_*` seam (ADR-0072).
#[path = "tests/remote_handle_nip44_tests.rs"]
mod remote_handle_nip44_tests;

/// `Nip46Signer::from_payload` restore failure/success paths.
#[path = "tests/payload_restore_tests.rs"]
mod payload_restore_tests;

/// `Nip46SignerHandle` accessor behavior (`from_bunker_uri*`, `local_pubkey`).
#[path = "tests/accessor_tests.rs"]
mod accessor_tests;
