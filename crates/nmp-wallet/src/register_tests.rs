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
    ActionModule, IncrementalApplyError, IngestParser, IngestParserRegistrar, ObservedProjection,
    RelayTextInterceptor,
};
use nmp_core::ObservedProjectionId;
use nmp_ownership::ProjectionRegistrationKey;

use super::*;
use crate::backend::WalletBackendId;

/// One opened observed-projection interest, recorded so a test can drive its
/// sink directly — mirrors `runtime_tests.rs`'s `RecordingRegistrar` (that
/// file exercises `WalletRuntime`'s own two interests directly; this one lets
/// `discovered_mints_populates_from_the_registered_mint_discovery_runtime`
/// below drive the sibling `MintDiscoveryRuntime`'s interests the same way).
#[derive(Clone)]
struct RecordedObservedProjection {
    consumer_id: String,
    observer: Arc<dyn nmp_core::ObservedProjectionSink>,
}

/// Records every opened observed-projection interest instead of discarding it
/// — `register()` wires two runtimes' worth of identity-reactive interests
/// (`WalletRuntime` + `MintDiscoveryRuntime`), and most tests here only care
/// that registration didn't error, so recording is a strict superset of the
/// old no-op behavior.
#[derive(Default)]
struct RecordingObservedRegistrar {
    next_id: Mutex<u64>,
    opened: Mutex<Vec<RecordedObservedProjection>>,
}

impl RecordingObservedRegistrar {
    fn opened(&self) -> Vec<RecordedObservedProjection> {
        self.opened.lock().unwrap().clone()
    }
}

impl ObservedProjectionRegistrar for RecordingObservedRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = ObservedProjectionId(*next);
        self.opened.lock().unwrap().push(RecordedObservedProjection {
            consumer_id: decl.consumer_id,
            observer: decl.observer,
        });
        id
    }

    fn close_observed_projection(&self, _id: ObservedProjectionId) {}

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        panic!("test does not request nested registrar handles")
    }
}

type CapturedIdentityObserver = Arc<Mutex<Option<Box<dyn Fn(Option<String>) + Send + Sync>>>>;

struct FakeApp {
    actions: Vec<&'static str>,
    active_pubkey: ActiveAccountSlot,
    actor_sender: nmp_core::CommandSender,
    configured_relays: nmp_core::AppRelaySlot,
    incremental: Arc<AtomicBool>,
    session_id: Arc<AtomicU64>,
    snapshot_epoch: Arc<AtomicU64>,
    /// Captures the identity-change observer `register` installs so a test can
    /// fire it and prove the account-switch reset wiring exists (#2916).
    identity_observer: CapturedIdentityObserver,
    observed: Arc<RecordingObservedRegistrar>,
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
            identity_observer: Arc::new(Mutex::new(None)),
            observed: Arc::new(RecordingObservedRegistrar::default()),
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

/// #3010 — `register()` now installs the kind:10019-arrival ingest parser
/// (`backend::cashu::nutzap_await`), so this fake host needs a (no-op, like
/// its `RelayTextInterceptorRegistrar` sibling above) implementation to
/// satisfy `register`'s trait bound. Nothing here needs to observe what was
/// registered — `send_nutzap_await_tests.rs` exercises the parser's real
/// behavior directly against a `CashuWalletBackend`.
impl IngestParserRegistrar for FakeApp {
    fn register_ingest_parser(&self, _kind: u32, _parser: Arc<dyn IngestParser>) {}

    fn replace_ingest_parser(
        &self,
        _kind: u32,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        None
    }

    fn unregister_ingest_parser(&self, _kind: u32, _slot_key: &'static str) {}

    fn replace_ingest_parser_range(
        &self,
        _range: std::ops::Range<u32>,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        None
    }

    fn unregister_ingest_parser_range(&self, _slot_key: &'static str) {}
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
        self.observed.open_observed_projection(decl)
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.observed.close_observed_projection(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::clone(&self.observed) as Arc<dyn ObservedProjectionRegistrar + Send + Sync>
    }
}

impl IdentityChangeRegistrar for FakeApp {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        *self.identity_observer.lock().unwrap() = Some(Box::new(f));
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
        crate::ACTION_CASHU_CROSS_MINT_TRANSFER,
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
fn register_wires_an_identity_change_reset_observer_for_both_backends() {
    // #2916: the composition root must install an identity-change observer that
    // resets BOTH the Cashu and NWC backends on a Nostr account switch (NWC is
    // Nostr-account-scoped per the owner's settled decision). Prove the observer
    // is registered and that firing it — for an account switch and a sign-out —
    // is safe and leaves the merged projection coherent.
    let mut app = FakeApp::new();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    let observer =
        app.identity_observer.lock().unwrap().take().expect(
            "register must wire an identity-change observer that resets the wallet backends",
        );

    // Switch to a new account, then sign out — neither may panic.
    observer(Some("b".repeat(64)));
    observer(None);

    // With nothing connected under any account, the merged NWC contribution is
    // NotConfigured — the projection stays coherent after the reset fires.
    let projection = handles.runtime.snapshot();
    assert_eq!(
        projection.readiness,
        crate::projection::WalletReadiness::NotConfigured,
        "no wallet connected under the switched-to account"
    );
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
    // back to exactly the runtime's merged snapshot (no mints discovered in
    // this test, so `discovered_mints` is empty on both sides — see
    // `discovered_mints_populates_from_the_registered_mint_discovery_runtime`
    // below for the populated case).
    let entry = wallet_merged_typed_projection(&handles.runtime, &handles.mint_discovery);
    assert_eq!(entry.key, crate::projection_wire::PROJECTION_KEY);
    assert_eq!(entry.schema_id, crate::projection_wire::SCHEMA_ID);
    assert_eq!(entry.schema_version, crate::projection_wire::SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NWMP");

    let decoded = crate::projection_wire::decode_wallet_projection(&entry.payload)
        .expect("registered sidecar payload must decode as NWMP");
    assert_eq!(decoded, handles.runtime.snapshot());
}

/// #2880 (epic #2864) — the whole point of this wave: once `MintDiscoveryRuntime`
/// (registered by `register()` alongside `WalletRuntime`) has observed a
/// capability-qualifying announcement and a web-of-trust-scoped recommendation,
/// `wallet_merged_typed_projection`'s encoded `NWMP` sidecar must carry them in
/// `discovered_mints` — proving the NIP-87 discovery policy
/// (`crate::mint_discovery::aggregate_discovered_mints`, already unit-tested in
/// isolation by `mint_discovery_tests.rs`) actually reaches the projection that
/// crosses FFI, not just `Handles::mint_discovery.snapshot()` in Rust.
///
/// Mirrors `runtime_tests.rs`'s
/// `on_kernel_event_forwards_backend_commands_onto_the_command_channel`: fetch
/// the sink `register()` wired onto the shared `RecordingObservedRegistrar`
/// and drive it directly with the exact `KernelEvent` shapes a relay delivery
/// would produce (kind:3 follow, kind:38172 announcement, kind:38000
/// recommendation) — both of `MintDiscoveryRuntime`'s reconcilers share the
/// same underlying sink (see `discovery_runtime.rs`), so opening any one of
/// its recorded interests is sufficient to reach the store.
#[test]
fn discovered_mints_populates_from_the_registered_mint_discovery_runtime() {
    let viewer = "aa".repeat(32);
    let recommender = "bb".repeat(32);
    let mint_url = "https://mint.example".to_string();

    let mut app = FakeApp::new();
    // Eager cold-start: an account already active before `register()` runs
    // (mirrors `MintDiscoveryRuntime::new`'s doc comment) sets the store's
    // scoring viewer immediately, without needing to fire an identity-change
    // observer first.
    *app.active_pubkey.lock().unwrap() = Some(viewer.clone());

    let handles = register(&mut app, Config::default()).expect("register must succeed");

    let opened = app.observed.opened();
    let sink = opened
        .iter()
        .find(|o| o.consumer_id == "nmp.wallet.mint_discovery")
        .expect("register() must open the NIP-87 mint-discovery interest")
        .observer
        .clone();

    // Viewer directly follows the recommender (kind:3 `p` tag) — a
    // `DIRECT_FOLLOW_SCORE` (100) vouch, well above the default minimum (1).
    sink.on_kernel_event(&nmp_core::substrate::KernelEvent {
        id: "3".repeat(64),
        author: viewer.clone(),
        kind: nmp_wot::KIND_CONTACT_LIST,
        created_at: 1_700_000_000,
        tags: vec![vec!["p".to_string(), recommender.clone()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    // A kind:38172 announcement advertising the nutzap-required NUTs.
    sink.on_kernel_event(&nmp_core::substrate::KernelEvent {
        id: "1".repeat(64),
        author: recommender.clone(),
        kind: nmp_nip87::KIND_MINT_ANNOUNCE,
        created_at: 1_700_000_001,
        tags: vec![
            vec!["d".to_string(), "mint-d".to_string()],
            vec!["u".to_string(), mint_url.clone()],
            vec!["nuts".to_string(), "1,2,4,7,11,12".to_string()],
            vec!["name".to_string(), "Test Mint".to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    // A kind:38000 Cashu recommendation for the same mint URL.
    sink.on_kernel_event(&nmp_core::substrate::KernelEvent {
        id: "2".repeat(64),
        author: recommender.clone(),
        kind: nmp_nip87::KIND_MINT_RECOMMEND,
        created_at: 1_700_000_002,
        tags: vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), mint_url.clone()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    let entry = wallet_merged_typed_projection(&handles.runtime, &handles.mint_discovery);
    let decoded = crate::projection_wire::decode_wallet_projection(&entry.payload)
        .expect("registered sidecar payload must decode as NWMP");

    assert_eq!(decoded.discovered_mints.len(), 1);
    let mint = &decoded.discovered_mints[0];
    assert_eq!(mint.url, mint_url);
    assert_eq!(mint.name.as_deref(), Some("Test Mint"));
    assert_eq!(mint.nuts, vec![1, 2, 4, 7, 11, 12]);
    assert!(mint.supports_nutzap);
    assert_eq!(mint.recommendation_count, 1);
    assert_eq!(mint.trust_score, nmp_wot::score::DIRECT_FOLLOW_SCORE);

    // `Handles::mint_discovery` (the Rust-side runtime-holds-projection
    // access) must agree with the encoded FFI sidecar bit for bit.
    let direct = handles.mint_discovery.snapshot();
    assert_eq!(direct.mints.len(), 1);
    assert_eq!(direct.mints[0].url, mint_url);
}
