//! Tests for [`super::register`]: the single composition-root entry point
//! wires every trait it declares without error, shares the same `nmp-nip47`
//! runtime handle between `Handles::nwc_wallet` and the installed
//! `NwcWalletBackend`, and registers exactly this wave's new action
//! namespaces (`select_backend` + the `cashu.*`/`nutzap.*` families) — never
//! `nmp.wallet.{connect,disconnect,pay_invoice}`, which stay `nmp-nip47`'s
//! own registration (see module docs).

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use nmp_core::slots::{new_active_account_slot, ActiveAccountSlot};
use nmp_core::substrate::{
    ActionModule, IncrementalApplyError, ObservedProjection, RelayTextInterceptor,
};
use nmp_core::ObservedProjectionId;
use nmp_ownership::ProjectionRegistrationKey;

use super::*;
use crate::backend::WalletBackendId;

/// Always-no-op `ObservedProjectionRegistrar` handle — sufficient for these
/// tests, which only prove `register()` wires without error and returns the
/// right `Handles`, not the identity-reactive open/close behavior itself
/// (that is `runtime_tests.rs`'s job, exercised directly against
/// `WalletRuntime`).
struct NoopObservedRegistrar;

impl ObservedProjectionRegistrar for NoopObservedRegistrar {
    fn open_observed_projection(&self, _decl: ObservedProjection) -> ObservedProjectionId {
        ObservedProjectionId(0)
    }

    fn close_observed_projection(&self, _id: ObservedProjectionId) {}

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::new(NoopObservedRegistrar)
    }
}

struct FakeApp {
    actions: Vec<&'static str>,
    active_pubkey: ActiveAccountSlot,
    actor_sender: nmp_core::CommandSender,
    configured_relays: nmp_core::AppRelaySlot,
    incremental: Arc<AtomicBool>,
    session_id: Arc<AtomicU64>,
    snapshot_epoch: Arc<AtomicU64>,
}

impl FakeApp {
    fn new() -> Self {
        Self {
            actions: Vec::new(),
            active_pubkey: new_active_account_slot(),
            actor_sender: nmp_core::CommandSender::bounded_channel().0,
            configured_relays: Arc::new(Mutex::new(nmp_core::AppRelayList::default())),
            incremental: Arc::new(AtomicBool::new(false)),
            session_id: Arc::new(AtomicU64::new(0)),
            snapshot_epoch: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ActionRegistrar for FakeApp {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        _module: M,
    ) -> Result<(), RegistrationError> {
        self.actions.push(M::NAMESPACE.as_str());
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
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
    }

    fn register_typed_snapshot_projection_with_time<K, F>(&self, _key: K, _f: F)
    where
        K: Into<ProjectionRegistrationKey>,
        F: Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
    }

    fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError> {
        Ok(())
    }

    fn incremental_apply_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.incremental)
    }

    fn frame_identity_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (
            Arc::clone(&self.session_id),
            Arc::clone(&self.snapshot_epoch),
        )
    }

    fn remove_snapshot_projection(&self, _key: &str) {}

    fn declare_consumed_projections<I, K>(&self, _keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
    }
}

impl ObservedProjectionRegistrar for FakeApp {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        NoopObservedRegistrar.open_observed_projection(decl)
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        NoopObservedRegistrar.close_observed_projection(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::new(NoopObservedRegistrar)
    }
}

impl IdentityChangeRegistrar for FakeApp {
    fn register_identity_change_observer<F>(&self, _f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
    }
}

impl HostCapabilities for FakeApp {
    fn active_pubkey(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_pubkey)
    }

    fn actor_sender(&self) -> nmp_core::CommandSender {
        self.actor_sender.clone()
    }

    fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        Arc::clone(&self.configured_relays)
    }
}

#[test]
fn register_installs_both_backends_and_a_live_nwc_runtime_handle() {
    let mut app = FakeApp::new();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    let selector = handles.runtime.selector();
    let nwc = selector
        .backend_by_id(&WalletBackendId::new(crate::NWC_BACKEND_ID))
        .expect("NWC backend must be registered");
    assert_eq!(nwc.id().as_str(), crate::NWC_BACKEND_ID);
    let cashu = selector
        .backend_by_id(&WalletBackendId::new(crate::CASHU_BACKEND_ID))
        .expect("Cashu backend must be registered");
    assert_eq!(cashu.id().as_str(), crate::CASHU_BACKEND_ID);

    // `Handles::nwc_wallet` must be a LIVE handle `nmp_nip47::register`
    // installed a runtime into — not an empty, never-initialized one — so
    // the composition-root caller can wire it into the NIP-57 zap
    // auto-chain exactly as it did before this crate's `register` existed.
    let guard = handles.nwc_wallet.lock().expect("wallet handle lock");
    assert!(
        guard.is_some(),
        "nmp_nip47::register must install a live WalletRuntime into the returned handle"
    );
}

#[test]
fn register_installs_exactly_this_waves_new_action_namespaces() {
    let mut app = FakeApp::new();
    register(&mut app, Config::default()).expect("register must succeed");

    // This wave's new selecting dispatch.
    for expected in [
        crate::ACTION_SELECT_BACKEND,
        crate::ACTION_CASHU_CREATE,
        crate::ACTION_CASHU_RECOVER,
        crate::ACTION_CASHU_SET_MINTS,
        crate::ACTION_CASHU_DEPOSIT_QUOTE,
        crate::ACTION_CASHU_COMPLETE_DEPOSIT,
        crate::ACTION_NUTZAP_PUBLISH_INFO,
        crate::ACTION_NUTZAP_SEND,
        crate::ACTION_NUTZAP_REDEEM,
    ] {
        assert!(
            app.actions.contains(&expected),
            "register() must install {expected}"
        );
    }

    // `nmp-nip47` registers connect/disconnect/pay_invoice itself (see
    // module docs) — `register()` must NOT install a second, competing
    // registration under those same names.
    for nip47_owned in [
        crate::ACTION_NWC_CONNECT,
        crate::ACTION_NWC_DISCONNECT,
        crate::ACTION_PAY_INVOICE,
    ] {
        let count = app.actions.iter().filter(|a| **a == nip47_owned).count();
        assert_eq!(
            count, 1,
            "{nip47_owned} must be registered exactly once (by nmp-nip47), got {count}"
        );
    }
}

#[test]
fn register_builds_a_working_merged_projection_snapshot() {
    let mut app = FakeApp::new();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    // Both backends advertise their capabilities regardless of connection
    // state (NWC is not connected in this test — no NWC URI was ever
    // dispatched — and the Cashu wallet has not been created), so the
    // merged projection's capability union must still surface both.
    let projection = handles.runtime.snapshot();
    assert!(
        projection.capabilities.pay_bolt11,
        "NWC's pay_bolt11 capability"
    );
    assert!(
        projection.capabilities.create_cashu_wallet,
        "Cashu's create_cashu_wallet capability"
    );
    assert!(
        projection.capabilities.deposit_cashu,
        "Cashu's deposit_cashu capability"
    );
}

#[test]
fn merged_typed_projection_encodes_a_decodable_nwmp_sidecar() {
    let mut app = FakeApp::new();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    // The typed sidecar builder the registered `"wallet.merged"` closure calls:
    // its envelope identity is the `NWMP` schema, and its payload round-trips
    // back to exactly the runtime's merged snapshot.
    let entry = wallet_merged_typed_projection(&handles.runtime);
    assert_eq!(entry.key, crate::projection_wire::PROJECTION_KEY);
    assert_eq!(entry.schema_id, crate::projection_wire::SCHEMA_ID);
    assert_eq!(entry.schema_version, crate::projection_wire::SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NWMP");

    let decoded = crate::projection_wire::decode_wallet_projection(&entry.payload)
        .expect("registered sidecar payload must decode as NWMP");
    assert_eq!(decoded, handles.runtime.snapshot());
}
