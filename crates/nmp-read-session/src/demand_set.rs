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
//!
//! # Trellis-backed diff (#3116)
//!
//! The desired-vs-live diff itself is NOT hand-rolled here: every session
//! owns a persistent [`nmp_core::trellis_reconciler::KeyedReconciler`] (the
//! reusable core factored for #3115/#3116) that `reconcile_read_demand_set`
//! feeds the full desired member map on every call. Trellis returns an
//! ORDERED resource plan (open added / replace a changed payload / close
//! removed); [`apply_demand_set_commands`] is the only place that plan turns
//! into real `ReadHost` calls. The hand-rolled `HashSet` diff this module
//! used before #3116 is gone — Trellis IS the diff.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::ObservedProjection;
use nmp_core::trellis_reconciler::KeyedReconciler;
use nmp_core::ObservedProjectionSink;
use trellis_core::{ResourceCommand, ResourceKey};

use crate::engine::replay_shapes_for;
use crate::host::{
    DemandSetReconciler, KeyedReadDemand, ReadDemand, ReadDemandSetSpec, ReadHost,
    ReadReplayPolicy,
};
use crate::registry::{DemandSetMembers, DemandSetState, ReadSessionBuild};
use crate::ReadHandle;

/// Trellis-internal diagnostic scope label — never surfaced to a concept.
const DEMAND_SET_SCOPE: &str = "nmp.read-session.demand-set.v1";

/// Open a dynamic read-demand-set session and return its typed close handle.
///
/// Installs the typed output, opens every initial member with
/// replay-before-live (same ordering guarantee as [`crate::open_read`]), and
/// registers ONE session whose close teardown drains and withdraws whatever
/// members are live at close time — so later `reconcile_read_demand_set`
/// calls need no engine-side bookkeeping beyond the shared member map and
/// [`nmp_core::trellis_reconciler::KeyedReconciler`] already recorded in the
/// registry.
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
    let reconciler = Arc::new(
        KeyedReconciler::<String, ReadDemand>::new(DEMAND_SET_SCOPE, demand_set_resource_key)
            .expect("fresh KeyedReconciler construction over an empty graph cannot fail"),
    );

    let commands = reconciler.reconcile(desired_map(initial_members));
    apply_demand_set_commands(host, &observer, &members, commands);

    let members_for_close = Arc::clone(&members);
    let reconciler_for_close = Arc::clone(&reconciler);
    let close_all_members: crate::registry::TeardownAction = Box::new(move || {
        for command in reconciler_for_close.close() {
            if let ResourceCommand::Close { key, .. } = command {
                withdraw_member(&members_for_close, key.as_str());
            }
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
        demand_set: Some(DemandSetState {
            members,
            reducer,
            reconciler,
        }),
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
/// withdrawn. Members whose key appears in both, with an unchanged demand,
/// are left untouched — no REQ for an already-live, still-desired member is
/// ever closed and reopened.
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
    // `read_demand_set_reconciler` hands back the reconciler type-erased
    // (#3130 — a host impl lives on a scanned public-surface crate, and even
    // the `DemandSetReconciler` alias must not name a raw Trellis type
    // there); this module is the ONE place that downcasts it back, since it
    // is the sole owner of the concrete type.
    let Some(reconciler) = host
        .read_demand_set_reconciler(projection_key)
        .and_then(|erased| erased.downcast::<DemandSetReconciler>().ok())
    else {
        return false;
    };

    let commands = reconciler.reconcile(desired_map(desired));
    apply_demand_set_commands(host, observer, &members, commands);

    (host.teardown_mark_changed())();
    true
}

/// The single-segment `ResourceKey` a demand-set member's `String` identity
/// encodes to. Single-segment means [`ResourceKey::as_str`] recovers the
/// ORIGINAL member key unchanged, so [`apply_demand_set_commands`] never
/// needs a separate `ResourceKey → String` translation table — the
/// member-teardown map stays keyed by the concept's own identity exactly as
/// it was before #3116.
fn demand_set_resource_key(key: &String) -> ResourceKey {
    ResourceKey::new(key.clone())
}

fn desired_map(members: Vec<KeyedReadDemand>) -> BTreeMap<String, ReadDemand> {
    members
        .into_iter()
        .map(|KeyedReadDemand { key, demand }| (key, demand))
        .collect()
}

/// Applies a Trellis resource plan **in `Vec` order** — never sort or
/// parallelize; LIFO close correctness on scope teardown lives in this order
/// (#3116 VERIFY-FIRST note; trellis-core guarantees Close-vs-Close
/// ordering, the host must preserve it by applying in order).
fn apply_demand_set_commands(
    host: &dyn ReadHost,
    observer: &Arc<dyn ObservedProjectionSink>,
    members: &DemandSetMembers,
    commands: Vec<ResourceCommand<ReadDemand>>,
) {
    for command in commands {
        match command {
            ResourceCommand::Open { key, command, .. } => {
                open_one_member(host, observer, members, key.as_str().to_string(), command);
            }
            ResourceCommand::Replace { key, command, .. } => {
                // A live member's demand payload changed under an unchanged
                // key. `ReadHost` has no in-place "replace a REQ" primitive,
                // so this withdraws then reopens under the SAME member key —
                // functionally what a correct hand-rolled diff would have
                // done had it detected the change (the pre-#3116 diff
                // silently ignored a same-key payload change; production
                // callers never trigger this in practice because a member's
                // key deterministically derives its demand, e.g.
                // `discovery_member_for_relay` in `nmp-nip29`).
                withdraw_member(members, key.as_str());
                open_one_member(host, observer, members, key.as_str().to_string(), command);
            }
            ResourceCommand::Close { key, .. } => {
                withdraw_member(members, key.as_str());
            }
            ResourceCommand::Refresh { .. } => {
                // Never emitted by this reconciler's planner
                // (`KeyedReconciler::new`'s `map_resource_planner` only
                // opens added / replaces updated / closes removed) —
                // exhaustive match, not a reachable branch.
            }
        }
    }
}

/// Removes and runs `key`'s recorded teardown action, if the member is
/// currently live. A poisoned lock or an already-withdrawn key is a safe
/// no-op (D6).
fn withdraw_member(members: &DemandSetMembers, key: &str) {
    let action = members.lock().ok().and_then(|mut map| map.remove(key));
    if let Some(action) = action {
        action();
    }
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
