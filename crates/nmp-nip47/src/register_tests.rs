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

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use nmp_core::substrate::{
    ActionModule, ActionRegistrar, IncrementalApplyError, RegistrationError, RelayTextInterceptor,
    RelayTextInterceptorRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::TypedProjectionData;
use nmp_ownership::ProjectionRegistrationKey;

use crate::register::{register, wallet_typed_projection, Config};
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

// ── #2894: `Handles::status` shares state with the registered runtime ──────
//
// `NwcWalletBackend` (nmp-wallet, epic #2864) needs a `WalletStatusSlot` clone
// bound to the SAME runtime instance `register()` installed, to derive
// readiness in `snapshot()`. A minimal fake host — implementing only the
// narrow registrar traits `register()` requires — proves `Handles::status` is
// not a fresh, disconnected slot but a clone of the exact `Arc` the installed
// `WalletRuntime` holds (D4: the runtime is that slot's sole writer).

#[derive(Default)]
struct FakeApp {
    incremental_flag: Arc<AtomicBool>,
    session_id: Arc<AtomicU64>,
    snapshot_epoch: Arc<AtomicU64>,
}

impl ActionRegistrar for FakeApp {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        _module: M,
    ) -> Result<(), RegistrationError> {
        Ok(())
    }
}

impl RelayTextInterceptorRegistrar for FakeApp {
    fn add_relay_text_interceptor(&self, _interceptor: Arc<dyn RelayTextInterceptor>) {}
}

impl SnapshotProjectionRegistrar for FakeApp {
    fn register_typed_snapshot_projection<K, F>(&self, _key: K, _f: F)
    where
        K: Into<ProjectionRegistrationKey>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
    }

    fn register_typed_snapshot_projection_with_time<K, F>(&self, _key: K, _f: F)
    where
        K: Into<ProjectionRegistrationKey>,
        F: Fn(u64) -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
    }

    fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError> {
        Ok(())
    }

    fn incremental_apply_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.incremental_flag)
    }

    fn frame_identity_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (Arc::clone(&self.session_id), Arc::clone(&self.snapshot_epoch))
    }

    fn remove_snapshot_projection(&self, _key: &str) {}

    fn declare_consumed_projections<I, K>(&self, _keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
    }
}

#[test]
fn handles_status_is_the_same_slot_the_registered_runtime_writes() {
    let mut app = FakeApp::default();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    let guard = handles.wallet.lock().expect("runtime handle lock");
    let runtime = guard
        .as_ref()
        .expect("register() must install a runtime into the returned handle");
    assert!(
        Arc::ptr_eq(&handles.status, &runtime.status_slot),
        "Handles::status must be a clone of the SAME WalletStatusSlot the \
         installed WalletRuntime holds — not a freshly constructed, \
         disconnected slot"
    );
}
