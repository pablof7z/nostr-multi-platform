//! Wave A proof: the `"wallet"` typed projection produces a typed-sidecar
//! entry (`TypedProjectionData`) whose `payload` decodes back to the same
//! `WalletStatus` via the generated `NWST` bindings.
//!
//! `wallet_typed_projection` returns exactly the `TypedProjectionData` the
//! kernel's `SnapshotRegistry::run_typed` collects into a frame's
//! `typed_projections` sidecar; driving it directly is the in-crate proof that
//! the `"wallet"` closure wires the right schema identity and payload — without
//! spinning the actor.
//!
//! Moved here from `nmp-app-chirp::wallet_runtime_tests` (V-95 / issue #619):
//! the wallet composition is now app-neutral and lives in this crate.

use crate::register::wallet_typed_projection;
use crate::{
    decode_wallet_status, new_wallet_status_slot, NwcConnectionState, WalletStatus,
    WALLET_STATUS_SCHEMA_ID, WALLET_STATUS_SCHEMA_VERSION,
};

fn sample_status() -> WalletStatus {
    WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://relay.example/nwc".to_string(),
        wallet_pubkey_hex: "ab".repeat(32),
        balance_msats: Some(7_000_000),
        balance_sats: Some(7_000),
        // `wallet_npub_short` removed (#1678, D7); `wallet_npub` itself
        // removed (#2762, D27) — shells derive `npub` from `wallet_pubkey_hex`.
        is_ready: true,
        is_connected: true,
        connection_state: Some(NwcConnectionState::Connected),
    }
}

#[test]
fn empty_slot_contributes_no_typed_sidecar_entry() {
    let slot = new_wallet_status_slot();
    assert!(
        wallet_typed_projection(&slot).is_none(),
        "a disconnected (None) slot must contribute no typed sidecar entry"
    );
}

#[test]
fn populated_slot_lands_typed_wallet_in_the_sidecar_and_round_trips() {
    let slot = new_wallet_status_slot();
    let status = sample_status();
    *slot.lock().unwrap() = Some(status.clone());

    let entry =
        wallet_typed_projection(&slot).expect("a connected wallet must produce a typed entry");

    // Schema identity the host's NWST decoder keys off.
    assert_eq!(entry.key, "wallet");
    assert_eq!(entry.schema_id, WALLET_STATUS_SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.nip47.wallet");
    assert_eq!(entry.schema_version, WALLET_STATUS_SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NWST");
    assert!(
        !entry.payload.is_empty(),
        "the typed sidecar payload must carry the encoded WalletStatus bytes"
    );

    // The bytes in the sidecar decode back to the original struct via the
    // generated NWST bindings — not only the generic `payload:Value` tree.
    let decoded =
        decode_wallet_status(&entry.payload).expect("sidecar payload must decode as NWST");
    assert_eq!(decoded, status);
}
