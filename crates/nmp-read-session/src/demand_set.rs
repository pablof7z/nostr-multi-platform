//! The dynamic-membership read engine: [`open_read_demand_set`] /
//! [`reconcile_read_demand_set`] (#93).
//!
//! A fixed [`crate::ReadSpec`] declares its demand(s) once, at open time — the
//! right shape for "one group's events" or "one relay's roster", where the
//! demand never changes for the life of the read. Some reads instead compose
//! an unbounded, CALLER-CONTROLLED number of like-shaped demands sharing one
//! reducer + one output, where the live member set changes over the
//! session's lifetime — e.g. NIP-29 multi-relay group discovery: one member
//! per relay, and the relay set grows/shrinks as the user browses. That is
//! what [`ReadDemandSetSpec`] models.
//!
//! The mechanics a concept must NOT re-author still apply: replay-before-live
//! per member, exact per-member withdrawal, and reverse teardown on close.
//! What's new here is that the withdrawal recipe isn't fixed at open time —
//! [`open_read_demand_set`] stores a **live, shared** member map in the
//! session's registry entry (keyed by the session's stable projection key,
//! not by a handle the caller must thread through), so a LATER call —
//! addressed only by that projection key — can look up the same map (and the
//! same reducer instance, type-erased) and reconcile it via
//! [`reconcile_read_demand_set`]: open members newly desired, close members no
//! longer desired, leave everything else untouched. No REQ for an unaffected
//! relay is ever closed and reopened.
//!
//! On close (via the ordinary [`crate::close_read`]), a SINGLE teardown action
//! drains whatever the member map holds AT THAT MOMENT and runs every
//! remaining member's withdrawal — so members added/removed via intervening
//! reconcile calls are still torn down exactly once, in the same reverse
//! order as every other read (interests first, then output, then the change
//! flag).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::ObservedProjection;
use nmp_core::ObservedProjectionSink;

use crate::engine::replay_shapes_for;
use crate::host::{KeyedReadDemand, ReadDemand, ReadDemandSetSpec, ReadHost, ReadReplayPolicy};
use crate::registry::{DemandSetMembers, DemandSetState, ReadSessionBuild};
use crate::ReadHandle;

/// Open a dynamic read-demand-set session and return its typed close handle.
///
/// Installs the typed output, opens every initial member with
/// replay-before-live (same ordering guarantee as [`crate::open_read`]), and
/// registers ONE session whose close teardown drains and withdraws whatever
/// members are live at close time — so later `reconcile_read_demand_set`
/// calls need no engine-side bookkeeping beyond the shared member map already
/// recorded in the registry.
///
/// Unlike [`crate::open_read`], a demand set with zero live members is not
/// torn down immediately: a demand set may legitimately start (or end up,
/// after reconciling away every member) empty and grow later.
#[must_use]
pub fn open_read_demand_set(host: &dyn ReadHost, spec: ReadDemandSetSpec) -> ReadHandle {
    let ReadDemandSetSpec {
        projection_key,
        members: initial_members,
        observer,
        reducer,
        output_encoder,
    } = spec;
    let key_str = projection_key.as_str().to_string();

    host.install_read_output(projection_key, output_encoder);

    let members: DemandSetMembers = Arc::new(Mutex::new(HashMap::new()));
    for KeyedReadDemand { key, demand } in initial_members {
        open_one_member(host, &observer, &members, key, demand);
    }

    let members_for_close = Arc::clone(&members);
    let close_all_members: crate::registry::TeardownAction = Box::new(move || {
        let Ok(mut map) = members_for_close.lock() else {
            return;
        };
        for (_, action) in map.drain() {
            action();
        }
    });

    let teardown = vec![
        host.teardown_mark_changed(),                 // exec last
        host.teardown_remove_output(key_str.clone()), // exec middle
        close_all_members,                            // exec first (reversed)
    ];

    let session_id = host.store_read_session(ReadSessionBuild {
        projection_key: key_str.clone(),
        teardown,
        demand_set: Some(DemandSetState { members, reducer }),
    });
    ReadHandle {
        projection_key: key_str,
        session_id,
    }
}

/// Reconcile a LIVE demand-set session (registered under `projection_key`) to
/// exactly `desired`: members present in `desired` but not yet open are
/// opened (replay-before-live, sharing `observer` — normally the SAME
/// instance [`crate::registry::ReadSessionRegistry::demand_set_reducer`]
/// handed back to the caller); members open but absent from `desired` are
/// withdrawn. Members whose key appears in both are left untouched — no REQ
/// for an already-live, still-desired member is ever closed and reopened.
///
/// Returns `false` when no live demand-set session is registered under
/// `projection_key` (the caller should [`open_read_demand_set`] instead).
#[must_use]
pub fn reconcile_read_demand_set(
    host: &dyn ReadHost,
    projection_key: &str,
    observer: &Arc<dyn ObservedProjectionSink>,
    desired: Vec<KeyedReadDemand>,
) -> bool {
    let Some(members) = host.read_demand_set_members(projection_key) else {
        return false;
    };

    let desired_keys: HashSet<&str> = desired.iter().map(|m| m.key.as_str()).collect();
    let stale_actions: Vec<crate::registry::TeardownAction> = {
        let Ok(mut map) = members.lock() else {
            return false;
        };
        let stale_keys: Vec<String> = map
            .keys()
            .filter(|k| !desired_keys.contains(k.as_str()))
            .cloned()
            .collect();
        stale_keys
            .into_iter()
            .filter_map(|k| map.remove(&k))
            .collect()
    };
    for action in stale_actions {
        action();
    }

    let already_open: HashSet<String> = members
        .lock()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    for KeyedReadDemand { key, demand } in desired {
        if already_open.contains(&key) {
            continue;
        }
        open_one_member(host, observer, &members, key, demand);
    }

    (host.teardown_mark_changed())();
    true
}

fn open_one_member(
    host: &dyn ReadHost,
    observer: &Arc<dyn ObservedProjectionSink>,
    members: &DemandSetMembers,
    key: String,
    demand: ReadDemand,
) {
    let ReadDemand {
        filter_json,
        consumer_id,
        scope,
        relay_pin,
        is_indexer_discovery,
        lifecycle,
        replay_limit,
        replay,
    } = demand;
    let replay_shapes = match replay {
        ReadReplayPolicy::Structural => replay_shapes_for(&filter_json, relay_pin.as_deref()),
        ReadReplayPolicy::LiveOnly => Vec::new(),
    };
    let decl = ObservedProjection {
        observer: Arc::clone(observer),
        filter_json,
        consumer_id,
        scope,
        relay_pin,
        is_indexer_discovery,
        lifecycle,
        replay_shapes,
        replay_limit,
    };
    let id = match replay {
        ReadReplayPolicy::Structural => host.open_read_interest(decl),
        ReadReplayPolicy::LiveOnly => host.open_live_only_read_interest(decl),
    };
    if id.0 != 0 {
        let action = host.teardown_close_interest(id);
        if let Ok(mut map) = members.lock() {
            map.insert(key, action);
        }
    }
}

#[cfg(test)]
#[path = "demand_set_tests.rs"]
mod tests;
