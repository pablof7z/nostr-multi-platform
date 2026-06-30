use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Mutex as StdMutex;

use mdk_core::MdkConfig;
use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_core::actor::ActorMail;
use nmp_core::slots::{new_active_account_slot, ActiveAccountSlot};
use nmp_core::substrate::{ActionModule, IncrementalApplyError};
use nostr::RelayUrl;

use super::*;

struct FakeHost {
    actions: Vec<&'static str>,
    ingest: StdMutex<Vec<(u32, &'static str)>>,
    typed: StdMutex<Vec<String>>,
    typed_time: StdMutex<Vec<String>>,
    identity_observers: AtomicUsize,
    active_pubkey: ActiveAccountSlot,
    actor_sender: nmp_core::CommandSender,
    configured_relays: nmp_core::AppRelaySlot,
    incremental: Arc<AtomicBool>,
    session_id: Arc<AtomicU64>,
    snapshot_epoch: Arc<AtomicU64>,
}

impl FakeHost {
    fn new(active_pubkey: Option<String>) -> Self {
        let active = new_active_account_slot();
        *active.lock().expect("active slot") = active_pubkey;
        Self {
            actions: Vec::new(),
            ingest: StdMutex::new(Vec::new()),
            typed: StdMutex::new(Vec::new()),
            typed_time: StdMutex::new(Vec::new()),
            identity_observers: AtomicUsize::new(0),
            active_pubkey: active,
            actor_sender: nmp_core::CommandSender::bounded_channel().0,
            configured_relays: Arc::new(Mutex::new(nmp_core::AppRelayList::default())),
            incremental: Arc::new(AtomicBool::new(false)),
            session_id: Arc::new(AtomicU64::new(0)),
            snapshot_epoch: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ActionRegistrar for FakeHost {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        _module: M,
    ) -> Result<(), RegistrationError> {
        self.actions.push(M::NAMESPACE);
        Ok(())
    }
}

impl IngestParserRegistrar for FakeHost {
    fn register_ingest_parser(&self, kind: u32, _parser: Arc<dyn IngestParser>) {
        self.ingest
            .lock()
            .expect("ingest registrations")
            .push((kind, "<anonymous>"));
    }

    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        self.ingest
            .lock()
            .expect("ingest registrations")
            .push((kind, slot_key));
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

impl SnapshotProjectionRegistrar for FakeHost {
    fn register_typed_snapshot_projection<K, F>(&self, key: K, _f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        self.typed
            .lock()
            .expect("typed registrations")
            .push(key.into());
    }

    fn register_typed_snapshot_projection_with_time<K, F>(&self, key: K, _f: F)
    where
        K: Into<String>,
        F: Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        self.typed_time
            .lock()
            .expect("typed registrations")
            .push(key.into());
    }

    fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError> {
        self.incremental
            .store(true, std::sync::atomic::Ordering::Release);
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

impl IdentityChangeRegistrar for FakeHost {
    fn register_identity_change_observer<F>(&self, _f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        self.identity_observers
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl HostCapabilities for FakeHost {
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

fn in_memory_projection() -> Arc<MarmotProjection> {
    let storage =
        MdkSqliteStorage::new_in_memory().expect("in-memory MDK storage should construct");
    let service = MarmotService::from_storage(storage, Keys::generate(), MdkConfig::default());
    Arc::new(MarmotProjection::new(service, None))
}

#[test]
fn install_registers_marmot_owned_runtime_surfaces() {
    let mut host = FakeHost::new(None);
    let config = MarmotConfig::new(
        ".",
        MarmotLocalCredentialSlot::new(Arc::new(Mutex::new(None))),
    );

    install(&mut host, config).expect("install should register");

    assert_eq!(host.actions, vec!["nmp.marmot"]);
    let ingest = host.ingest.lock().expect("ingest registrations");
    assert_eq!(
        ingest.as_slice(),
        &[
            (KIND_GIFT_WRAP, MARMOT_INGEST_SLOT),
            (KIND_MARMOT_GROUP_MESSAGE, MARMOT_INGEST_SLOT),
            (KIND_MARMOT_KEY_PACKAGE, MARMOT_INGEST_SLOT),
            (KIND_MARMOT_WELCOME, MARMOT_INGEST_SLOT),
        ]
    );
    assert_eq!(
        host.typed_time.lock().expect("typed time").as_slice(),
        &[snapshot_fb::PROJECTION_KEY.to_string()]
    );
    assert_eq!(
        host.typed.lock().expect("typed").as_slice(),
        &[messages_fb::PROJECTION_KEY.to_string()]
    );
    assert_eq!(
        host.identity_observers
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[test]
fn clearing_active_projection_withdraws_marmot_owned_interests() {
    let (sender, rx) = nmp_core::CommandSender::bounded_channel();
    let projection = in_memory_projection();
    let relay: RelayUrl = "wss://relay.example".parse().expect("valid relay url");
    let _ = projection.with_inner(|h| h.cache_group_relays("group-1".to_string(), vec![relay]));

    let runtime = MarmotRuntime {
        config: MarmotConfig::new(
            ".",
            MarmotLocalCredentialSlot::new(Arc::new(Mutex::new(None))),
        ),
        actor_sender: sender,
        active: Mutex::new(Some(ActiveMarmotProjection {
            pubkey_hex: "alice".to_string(),
            projection,
        })),
    };

    runtime.rebind_for_identity(None);

    let commands = rx.try_iter().collect::<Vec<_>>();
    let drop_count = commands
        .iter()
        .filter(|mail| {
            matches!(
                mail,
                ActorMail::Command(ActorCommand::Interests(
                    InterestsCommand::DropInterestOwner(_)
                ))
            )
        })
        .count();
    assert_eq!(
        drop_count, 2,
        "logout must drop the gift-wrap inbox and cached group-message interest owners"
    );
}
