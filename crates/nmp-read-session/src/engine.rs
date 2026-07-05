//! The ONE read-lifecycle driver: [`open_read`] / [`close_read`].
//!
//! This is the single implementation of the mechanics a concept-owned active
//! read must NOT re-author: replay-before-live ordering, live activation, exact
//! per-demand withdrawal, reverse teardown, and typed-output tombstone. A
//! concept crate supplies only a declarative [`ReadSpec`] and drives it through
//! here; if a concept read grows its own registry, close map, replay
//! implementation, or teardown recipe, the engine boundary is wrong and the
//! engine is what gets fixed (#2777).

use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_planner::InterestShape;

use crate::dependent::{close_dependent_reconcilers, prepare_dependent_demand_observer};
use crate::host::{ReadDemand, ReadHandle, ReadHost, ReadReplayPolicy, ReadSpec};
use crate::registry::{ReadSessionBuild, ReadSessionId, TeardownAction};

/// Derive the read-cache replay shapes for a demand from its own `REQ` filter
/// (the default structural seed strategy). The replay matches cached events by
/// the SAME wire shape the live filter uses, so the muted observer is hydrated
/// to exactly its future-delivery scope before it is activated (ADR-0070). A
/// malformed filter yields no shapes (the host then no-ops the interest open);
/// a concept never hand-authors replay.
#[must_use]
pub fn replay_shapes_for(filter_json: &str, relay_pin: Option<&str>) -> Vec<InterestShape> {
    InterestShape::from_filter_json(filter_json)
        .map(|mut shape| {
            shape.relay_pin = relay_pin.map(str::to_string);
            shape
        })
        .into_iter()
        .collect()
}

/// Open one concept-owned active read on the engine and return its typed close
/// handle.
///
/// The concept supplied the demand(s), the admission-applying reducer, and the
/// typed output in `spec`. The engine here — and only here — installs the
/// output, opens each demand with replay-before-live, records the exact reverse
/// teardown (withdraw every interest, tombstone the output, flag a tick), and
/// registers the session in the ONE shared registry so it lands in one leak
/// audit. All demands share the single reducer, so composing conventions (e.g.
/// NIP-10 + NIP-22 replies) is many `REQ`s folding into one read model.
#[must_use]
pub fn open_read(host: &dyn ReadHost, spec: ReadSpec) -> ReadHandle {
    let ReadSpec {
        projection_key,
        demands,
        observer,
        output_encoder,
        dependent_demands,
        keep_open_without_live_demand,
    } = spec;
    let key_str = projection_key.as_str().to_string();

    let (observer, dependent_reconcilers) =
        prepare_dependent_demand_observer(host, &key_str, observer, dependent_demands);

    // 1. Install the typed output (coalesced emission + tombstone are host-owned).
    host.install_read_output(projection_key, output_encoder);

    // 2. Replay-before-live per demand, all folding into the single reducer.
    let mut interest_ids = Vec::with_capacity(demands.len());
    for demand in demands {
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
            observer: Arc::clone(&observer),
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
            interest_ids.push(id);
        }
    }

    // 3. If nothing stayed live, do not track a dead read: tombstone the output
    //    we installed and flag a tick, then hand back a closed sentinel handle.
    if interest_ids.is_empty() && !keep_open_without_live_demand {
        close_dependent_reconcilers(&dependent_reconcilers);
        (host.teardown_remove_output(key_str.clone()))();
        (host.teardown_mark_changed())();
        return ReadHandle {
            projection_key: key_str,
            session_id: ReadSessionId(0),
        };
    }

    // 4. Reverse-teardown recipe. Registration order below is the reverse of the
    //    execution order the registry applies on close, so execution is:
    //    withdraw derived + primary interests → tombstone output → flag tick.
    let mut teardown: Vec<TeardownAction> =
        Vec::with_capacity(interest_ids.len() + dependent_reconcilers.len() + 2);
    teardown.push(host.teardown_mark_changed()); // exec last
    teardown.push(host.teardown_remove_output(key_str.clone())); // exec middle
    for id in interest_ids {
        teardown.push(host.teardown_close_interest(id)); // exec first (reversed)
    }
    for reconciler in dependent_reconcilers {
        teardown.push(Box::new(move || reconciler.close_current()));
    }

    // 5. Record in the ONE shared registry; pair the id with the key as the handle.
    let session_id = host.store_read_session(ReadSessionBuild {
        projection_key: key_str.clone(),
        teardown,
        demand_set: None,
    });
    ReadHandle {
        projection_key: key_str,
        session_id,
    }
}

/// Close a read opened by [`open_read`], using the HANDLE (never a re-derived
/// filter or raw key). The registry is the authority for whether the session is
/// live and which key it owns; a stale/forged handle is a safe `false` (D6).
#[must_use]
pub fn close_read(host: &dyn ReadHost, handle: &ReadHandle) -> bool {
    match host.read_session_projection_key(&handle.session_id) {
        Some(key) if key == handle.projection_key => host.close_read_session(&handle.session_id),
        _ => false,
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
