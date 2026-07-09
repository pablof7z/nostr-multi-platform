//! Boundary proofs for [`super::KeyedReadCollection`]: membership add/remove
//! reconcile, `Replace` on a descriptor change (the exogenous-scalar path),
//! the leak-audit oracle, lock-freedom around host closures (the #60
//! deadlock-class guard), and both flavors of "generic over host open/close"
//! — a bare closure standing in for `open_observed_projection`, and a real
//! [`crate::ReadHost`]-backed independent read-session per key (shape (b)'s
//! defining difference from `demand_set`'s shape (a): each key gets its OWN
//! reducer/output, not one shared across the whole collection).

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::MemberKey;

use super::KeyedReadCollection;

fn collection_of_u32() -> KeyedReadCollection<String, u32> {
    KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        |_key, _command| Box::new(|| {}) as crate::registry::TeardownAction,
    )
    .expect("fresh collection")
}

fn desired(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), *value))
        .collect()
}

// ── Shape (b): membership + Replace + leak oracle ──────────────────────────

#[test]
fn reconcile_mounts_every_desired_member() {
    let collection = collection_of_u32();
    collection.reconcile(desired(&[("a", 1), ("b", 2)]));
    assert_eq!(collection.live_count(), 2);
    assert!(collection.full_recompute_matches());
}

#[test]
fn reconcile_adds_a_member_without_touching_the_existing_one() {
    let mounted = Arc::new(Mutex::new(Vec::<String>::new()));
    let mounted_in_closure = Arc::clone(&mounted);
    let collection: KeyedReadCollection<String, u32> = KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        move |key, _command| {
            mounted_in_closure
                .lock()
                .unwrap()
                .push(key.as_str().to_string());
            Box::new(|| {}) as crate::registry::TeardownAction
        },
    )
    .expect("fresh collection");

    collection.reconcile(desired(&[("a", 1), ("b", 2)]));
    mounted.lock().unwrap().clear();

    collection.reconcile(desired(&[("a", 1), ("b", 2), ("c", 3)]));
    assert_eq!(
        *mounted.lock().unwrap(),
        vec!["c".to_string()],
        "adding a member must not remount any existing member"
    );
    assert_eq!(collection.live_count(), 3);
    assert!(collection.full_recompute_matches());
}

#[test]
fn reconcile_withdraws_a_member_no_longer_desired() {
    let collection = collection_of_u32();
    collection.reconcile(desired(&[("a", 1), ("b", 2)]));

    collection.reconcile(desired(&[("b", 2)]));
    assert_eq!(collection.live_count(), 1);
    assert!(collection.full_recompute_matches());
}

#[test]
fn reconcile_replaces_a_live_member_whose_descriptor_changed() {
    // The exogenous-scalar path: a value NOT in the key-set (e.g. 29er's
    // `active_pubkey`) changes, the caller embeds it in the payload and
    // re-supplies the SAME key with the new payload — this must withdraw +
    // remount exactly that member, never force-close+reopen the whole set.
    let withdrawn = Arc::new(Mutex::new(0u32));
    let mounted = Arc::new(Mutex::new(Vec::<u32>::new()));
    let (withdrawn_for_open, mounted_for_open) = (Arc::clone(&withdrawn), Arc::clone(&mounted));
    let collection: KeyedReadCollection<String, u32> = KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        move |_key, command| {
            mounted_for_open.lock().unwrap().push(command);
            let withdrawn = Arc::clone(&withdrawn_for_open);
            Box::new(move || {
                *withdrawn.lock().unwrap() += 1;
            }) as crate::registry::TeardownAction
        },
    )
    .expect("fresh collection");

    collection.reconcile(desired(&[("a", 1)]));
    assert_eq!(*mounted.lock().unwrap(), vec![1]);

    collection.reconcile(desired(&[("a", 2)]));
    assert_eq!(
        *withdrawn.lock().unwrap(),
        1,
        "the stale descriptor's mount must be torn down exactly once"
    );
    assert_eq!(
        *mounted.lock().unwrap(),
        vec![1, 2],
        "the new descriptor must be remounted under the SAME key"
    );
    assert_eq!(collection.live_count(), 1);
    assert!(collection.full_recompute_matches());
}

#[test]
fn reconcile_leaves_an_unchanged_member_untouched() {
    let mounted = Arc::new(Mutex::new(0u32));
    let mounted_in_closure = Arc::clone(&mounted);
    let collection: KeyedReadCollection<String, u32> = KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        move |_key, _command| {
            *mounted_in_closure.lock().unwrap() += 1;
            Box::new(|| {}) as crate::registry::TeardownAction
        },
    )
    .expect("fresh collection");

    collection.reconcile(desired(&[("a", 1)]));
    collection.reconcile(desired(&[("a", 1)]));
    assert_eq!(
        *mounted.lock().unwrap(),
        1,
        "an unchanged key/payload pair must never remount"
    );
}

#[test]
fn close_withdraws_every_member_exactly_once_and_is_idempotent() {
    let withdrawn = Arc::new(Mutex::new(Vec::<String>::new()));
    let withdrawn_in_closure = Arc::clone(&withdrawn);
    let collection: KeyedReadCollection<String, u32> = KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        move |key, _command| {
            let withdrawn = Arc::clone(&withdrawn_in_closure);
            let key = key.as_str().to_string();
            Box::new(move || withdrawn.lock().unwrap().push(key)) as crate::registry::TeardownAction
        },
    )
    .expect("fresh collection");

    collection.reconcile(desired(&[("a", 1), ("b", 2)]));
    collection.close();
    assert_eq!(collection.live_count(), 0);
    let first = withdrawn.lock().unwrap().clone();
    assert_eq!(
        first.len(),
        2,
        "every live member must be withdrawn on close"
    );

    collection.close();
    assert_eq!(
        withdrawn.lock().unwrap().len(),
        2,
        "a second close must not re-run any teardown"
    );
}

// ── Generic over host open/close: two flavors, same primitive ──────────────

/// Flavor 1: a bare closure, standing in for a raw
/// `open_observed_projection`/`close_observed_projection` host call — no
/// `ReadHost`, no `nmp-read-session` machinery involved at all. Proves the
/// primitive does not hardcode read-session-shaped mounting.
#[test]
fn members_can_mount_over_a_bare_host_closure_not_a_read_session() {
    let next_id = Arc::new(AtomicU64::new(1));
    let live_projection_ids = Arc::new(Mutex::new(Vec::<u64>::new()));
    let (ids_for_open, ids_for_close) = (
        Arc::clone(&live_projection_ids),
        Arc::clone(&live_projection_ids),
    );
    let next_id_for_open = Arc::clone(&next_id);
    let collection: KeyedReadCollection<String, u32> = KeyedReadCollection::new(
        "test-scope",
        |key: &String| MemberKey::new(key.clone()),
        move |_key, _command| {
            let id = next_id_for_open.fetch_add(1, Ordering::Relaxed);
            ids_for_open.lock().unwrap().push(id);
            let ids_for_close = Arc::clone(&ids_for_close);
            Box::new(move || ids_for_close.lock().unwrap().retain(|&x| x != id))
                as crate::registry::TeardownAction
        },
    )
    .expect("fresh collection");

    collection.reconcile(desired(&[("group-1", 0)]));
    assert_eq!(*live_projection_ids.lock().unwrap(), vec![1]);
    collection.close();
    assert!(live_projection_ids.lock().unwrap().is_empty());
}

/// Flavor 2: each key mounts a FULL, independent [`crate::open_read`]
/// read-session — its own reducer, its own typed output, its own
/// [`crate::ReadHandle`] — via a real [`crate::ReadHost`]. This is shape
/// (b)'s defining difference from `demand_set`'s shape (a): N members, N
/// reducers/outputs, not one shared across the set.
mod read_session_flavor {
    use super::*;
    use nmp_core::substrate::{KernelEvent, ObservedProjection};
    use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
    use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};

    use crate::host::{DemandSetReconciler, ReadDemand};
    use crate::registry::{DemandSetMembers, ReadSessionBuild, ReadSessionId, ReadSessionRegistry};
    use crate::{
        close_read, open_read, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy, ReadSpec,
    };

    #[derive(Default)]
    struct RecordingSink {
        seen: Mutex<Vec<String>>,
    }

    impl ObservedProjectionSink for RecordingSink {
        fn on_kernel_event(&self, event: &KernelEvent) {
            self.seen.lock().unwrap().push(event.id.clone());
        }
    }

    struct FakeHost {
        registry: ReadSessionRegistry,
        next_interest: AtomicU64,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                registry: ReadSessionRegistry::default(),
                next_interest: AtomicU64::new(1),
            }
        }
    }

    impl ReadHost for FakeHost {
        fn install_read_output(
            &self,
            _key: ProjectionRegistrationKey,
            _encoder: ReadOutputEncoder,
        ) {
        }
        fn open_read_interest(&self, _decl: ObservedProjection) -> ObservedProjectionId {
            ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed))
        }
        fn teardown_close_interest(&self, _id: ObservedProjectionId) -> crate::TeardownAction {
            Box::new(|| {})
        }
        fn teardown_remove_output(&self, _key: String) -> crate::TeardownAction {
            Box::new(|| {})
        }
        fn teardown_mark_changed(&self) -> crate::TeardownAction {
            Box::new(|| {})
        }
        fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
            self.registry.open(build)
        }
        fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
            self.registry.projection_key(id)
        }
        fn close_read_session(&self, id: &ReadSessionId) -> bool {
            self.registry.close(id)
        }
        fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
            self.registry.close_by_projection_key(projection_key)
        }
        fn read_session_id_for_projection_key(
            &self,
            projection_key: &str,
        ) -> Option<ReadSessionId> {
            self.registry.session_id_for_projection_key(projection_key)
        }
        fn read_demand_set_members(&self, projection_key: &str) -> Option<DemandSetMembers> {
            self.registry.demand_set_members(projection_key)
        }
        fn read_demand_set_reducer(
            &self,
            projection_key: &str,
        ) -> Option<Arc<dyn Any + Send + Sync>> {
            self.registry.demand_set_reducer(projection_key)
        }
        fn read_demand_set_reconciler(
            &self,
            projection_key: &str,
        ) -> Option<Arc<DemandSetReconciler>> {
            self.registry.demand_set_reconciler(projection_key)
        }
    }

    fn projection_key(group_id: &str) -> ProjectionRegistrationKey {
        ProjectionRegistrationKey::Dynamic(
            DynamicProjectionKey::app_owned(format!("group-feed.{group_id}")).unwrap(),
        )
    }

    fn group_demand(group_id: &str) -> ReadDemand {
        ReadDemand {
            filter_json: format!(r##"{{"kinds":[9],"#h":["{group_id}"]}}"##),
            consumer_id: format!("group-feed::{group_id}"),
            scope: 1,
            relay_pin: None,
            is_indexer_discovery: false,
            lifecycle: nmp_planner::InterestLifecycle::Tailing,
            replay_limit: 64,
            replay: ReadReplayPolicy::Structural,
        }
    }

    #[test]
    fn each_key_mounts_its_own_independent_read_session() {
        let host = Arc::new(FakeHost::new());
        let host_for_open = Arc::clone(&host);
        let collection: KeyedReadCollection<String, String> = KeyedReadCollection::new(
            "group-feeds",
            |group_id: &String| MemberKey::new(group_id.clone()),
            move |resource_key, group_id: String| {
                let host_for_close = Arc::clone(&host_for_open);
                // Each key builds its OWN reducer + output — shape (b)'s
                // defining property, unlike demand_set's one-reducer-for-all.
                let spec = ReadSpec {
                    projection_key: projection_key(&group_id),
                    demands: vec![group_demand(&group_id)],
                    observer: Arc::new(RecordingSink::default()),
                    output_encoder: Box::new(|| None),
                    dependent_demands: Vec::new(),
                    keep_open_without_live_demand: false,
                };
                let handle: ReadHandle = open_read(host_for_open.as_ref(), spec);
                let _ = resource_key;
                Box::new(move || {
                    let _ = close_read(host_for_close.as_ref(), &handle);
                }) as crate::registry::TeardownAction
            },
        )
        .expect("fresh collection");

        let mut desired = BTreeMap::new();
        desired.insert("group-1".to_string(), "group-1".to_string());
        desired.insert("group-2".to_string(), "group-2".to_string());
        collection.reconcile(desired);

        assert_eq!(collection.live_count(), 2);
        assert_eq!(
            host.registry.live_count(),
            2,
            "two independent read-sessions, one per key"
        );

        collection.close();
        assert_eq!(collection.live_count(), 0);
        assert_eq!(host.registry.live_count(), 0);
    }
}

// ── #60 deadlock-class guard ────────────────────────────────────────────────

#[test]
fn host_open_closure_can_call_back_into_the_collection_without_deadlocking() {
    // The #60 deadlock class (#3078-#3081) was a closure re-locking a
    // registry the calling frame already held. Proves the inverse holds
    // here: `reconcile`'s `apply` step never holds a lock of its own — not
    // `KeyedReconciler`'s (released before `reconcile` returns the plan,
    // strictly before `apply` runs) nor `live`'s (locked only to record the
    // teardown AFTER the open closure returns) — while running a host
    // `open` closure. A closure that calls straight back into the SAME
    // collection cannot hang; if `apply`/`open_one` held either lock across
    // the call, this test would deadlock and time out rather than assert.
    #[allow(clippy::type_complexity)]
    let self_ref: Arc<Mutex<Option<std::sync::Weak<KeyedReadCollection<String, u32>>>>> =
        Arc::new(Mutex::new(None));
    let self_ref_in_closure = Arc::clone(&self_ref);
    let collection = Arc::new(
        KeyedReadCollection::new(
            "test-scope",
            |key: &String| MemberKey::new(key.clone()),
            move |_key, _command: u32| {
                if let Some(collection) = self_ref_in_closure
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade)
                {
                    // Reentrant calls from inside the host's own open
                    // closure — proves no lock this type owns is held here.
                    assert_eq!(
                        collection.live_count(),
                        0,
                        "no member has been recorded yet"
                    );
                    assert!(collection.full_recompute_matches());
                }
                Box::new(|| {}) as crate::registry::TeardownAction
            },
        )
        .expect("fresh collection"),
    );
    *self_ref.lock().unwrap() = Some(Arc::downgrade(&collection));

    collection.reconcile(desired(&[("a", 1)]));
    assert_eq!(collection.live_count(), 1);
}
