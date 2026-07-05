//! Tests for [`super::register`]: the composition-root entry point wires the
//! identity-reactive [`MintDiscoveryRuntime`] plus the typed
//! `"mint_discovery"` snapshot projection, and a discovery-only event
//! re-emits a changed projection on the next producer call (the dirty-flag +
//! `make_update` path, #2880).

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use nmp_core::slots::{new_active_account_slot, ActiveAccountSlot};
use nmp_core::substrate::{IncrementalApplyError, ObservedProjection};
use nmp_core::ObservedProjectionId;
use nmp_ownership::ProjectionRegistrationKey;

use super::*;

/// One opened observed-projection interest, recorded so a test can drive its
/// sink directly.
#[derive(Clone)]
struct RecordedObservedProjection {
    consumer_id: String,
    observer: Arc<dyn nmp_core::ObservedProjectionSink>,
}

/// Records every opened observed-projection interest instead of discarding
/// it, so a test can drive `register()`'s wired sink directly with
/// relay-shaped `KernelEvent`s.
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
    active_pubkey: ActiveAccountSlot,
    actor_sender: nmp_core::CommandSender,
    configured_relays: nmp_core::AppRelaySlot,
    incremental: Arc<AtomicBool>,
    session_id: Arc<AtomicU64>,
    snapshot_epoch: Arc<AtomicU64>,
    identity_observer: CapturedIdentityObserver,
    observed: Arc<RecordingObservedRegistrar>,
}

impl FakeApp {
    fn new() -> Self {
        Self {
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
fn register_installs_a_runtime_and_a_decodable_nmds_sidecar() {
    let mut app = FakeApp::new();
    let handles = register(&mut app, Config::default()).expect("register must succeed");

    let entry = mint_discovery_typed_projection(&handles.runtime);
    assert_eq!(entry.key, crate::projection_wire::PROJECTION_KEY);
    assert_eq!(entry.schema_id, crate::projection_wire::SCHEMA_ID);
    assert_eq!(entry.schema_version, crate::projection_wire::SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NMDS");

    let decoded = crate::projection_wire::decode_mint_discovery_projection(&entry.payload)
        .expect("registered sidecar payload must decode as NMDS");
    assert_eq!(decoded, handles.runtime.snapshot());
    assert!(decoded.mints.is_empty(), "no discovery events fed yet");
}

/// The whole point of this crate: once the runtime has observed a
/// capability-qualifying announcement and a web-of-trust-scoped
/// recommendation, calling the registered typed-projection producer AGAIN
/// must reflect the change — proving the dirty-flag + `make_update`
/// reactivity path actually re-emits rather than serving a stale empty
/// snapshot (#2880).
#[test]
fn a_discovery_only_event_re_emits_a_changed_typed_projection() {
    let viewer = "aa".repeat(32);
    let recommender = "bb".repeat(32);
    let mint_url = "https://mint.example".to_string();

    let mut app = FakeApp::new();
    // Eager cold-start: an account already active before `register()` runs
    // sets the store's scoring viewer immediately.
    *app.active_pubkey.lock().unwrap() = Some(viewer.clone());

    let handles = register(&mut app, Config::default()).expect("register must succeed");

    // Baseline: no events observed yet, so the producer emits an empty row.
    let baseline = mint_discovery_typed_projection(&handles.runtime);
    let baseline_decoded =
        crate::projection_wire::decode_mint_discovery_projection(&baseline.payload)
            .expect("baseline payload must decode");
    assert!(baseline_decoded.mints.is_empty());

    let opened = app.observed.opened();
    let sink = opened
        .iter()
        .find(|o| o.consumer_id == "nmp.mint_discovery.announcements")
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

    let entry = mint_discovery_typed_projection(&handles.runtime);
    assert_ne!(
        entry.payload, baseline.payload,
        "the producer must re-emit a changed payload once the store is dirtied"
    );
    let decoded = crate::projection_wire::decode_mint_discovery_projection(&entry.payload)
        .expect("registered sidecar payload must decode as NMDS");

    assert_eq!(decoded.mints.len(), 1);
    let mint = &decoded.mints[0];
    assert_eq!(mint.url, mint_url);
    assert_eq!(mint.name.as_deref(), Some("Test Mint"));
    assert_eq!(mint.nuts, vec![1, 2, 4, 7, 11, 12]);
    assert!(mint.supports_nutzap);
    assert_eq!(mint.recommendation_count, 1);
    assert_eq!(mint.trust_score, nmp_wot::score::DIRECT_FOLLOW_SCORE);

    // `Handles::runtime` (the Rust-side runtime-holds-projection access) must
    // agree with the encoded sidecar bit for bit.
    let direct = handles.runtime.snapshot();
    assert_eq!(direct.mints.len(), 1);
    assert_eq!(direct.mints[0].url, mint_url);
}
