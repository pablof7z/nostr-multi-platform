//! Shared test fixtures: in-memory `MarmotService` construction, the
//! multi-actor `Actor` handle, and `bootstrap_pair`, the full
//! create → gift-wrap → unwrap → accept → post-join self-update dance every
//! scenario in the sibling test modules builds on.

use mdk_core::prelude::{MessageProcessingResult, NostrGroupConfigData};
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Keys, PublicKey, RelayUrl};

use crate::service::MarmotService;

pub(super) fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

pub(super) fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

/// One MLS actor: its identity keys + service. Returned by [`new_actor`] so
/// multi-actor lifecycle tests can keep a stable handle on each peer.
pub(super) struct Actor {
    pub(super) keys: Keys,
    pub(super) service: MarmotService,
}

impl Actor {
    pub(super) fn pubkey(&self) -> PublicKey {
        self.keys.public_key()
    }
}

/// Build a fresh actor with independent in-memory MLS storage. Each call to
/// `MdkSqliteStorage::new_in_memory()` yields a private SQLite handle, so
/// actors never share ratchet state (the round-trip test already relies on
/// this; multi-actor tests below exercise it harder).
pub(super) fn new_actor() -> Actor {
    let keys = Keys::generate();
    Actor {
        service: in_memory_service(keys.clone()),
        keys,
    }
}

/// A standard group config naming `admins` as the admin set.
pub(super) fn group_config(admins: Vec<PublicKey>) -> NostrGroupConfigData {
    NostrGroupConfigData::new(
        "Lifecycle Test".to_string(),
        "lifecycle".to_string(),
        None,
        None,
        None,
        test_relays(),
        admins,
    )
}

/// Have `admin` create a group with `joiner` invited. Performs the full
/// create → gift-wrap → unwrap → accept → post-join self-update dance and
/// converges both peers on the post-join epoch. Returns the group id.
pub(super) fn bootstrap_pair(admin: &Actor, joiner: &Actor) -> mdk_core::prelude::GroupId {
    let joiner_kp = joiner
        .service
        .publish_key_package(test_relays())
        .expect("joiner key package");
    let config = group_config(vec![admin.pubkey()]);
    let (group, pending) = admin
        .service
        .create_group(vec![joiner_kp.event_30443.clone()], config)
        .expect("admin creates group");
    let group_id = group.mls_group_id.clone();

    // Deliver the Welcome to the joiner via the real NIP-59 gift-wrap path.
    let rumor = pending.welcome_rumors[0].clone();
    let gift = admin
        .service
        .wrap_welcome(&joiner.pubkey(), rumor)
        .expect("admin gift-wraps welcome");
    pending.commit().expect("admin merges create commit");

    let (welcome, _) = joiner
        .service
        .unwrap_and_process_welcome(&gift)
        .expect("joiner processes welcome");
    joiner
        .service
        .accept_welcome(&welcome)
        .expect("joiner accepts welcome");

    // MIP-02 mandatory post-join self-update; admin processes the commit so
    // both converge.
    let su = joiner
        .service
        .self_update(&group_id)
        .expect("post-join self_update");
    let su_commit = su.evolution_event.clone();
    su.commit().expect("joiner merges self_update");
    match admin
        .service
        .process_message(&su_commit)
        .expect("admin processes joiner self_update")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from post-join self_update, got {other:?}"),
    }
    group_id
}
