//! Facade-composable constructors for [`nmp_read_session::KeyedReadCollection`]
//! (#3115) against a live [`NmpApp`].
//!
//! `KeyedReadCollection<K, C>` itself is generic over host open/close by
//! design (it never names a read-session or an observed projection — see its
//! module docs in `nmp-read-session`), so it cannot be exposed as ONE opaque
//! UniFFI type: a facade's `K`/`C` are its own concept types (e.g. 29er's
//! group-tree descriptors), which this crate never sees. What every such
//! facade DOES repeat verbatim is the open/close plumbing against `NmpApp`'s
//! two host seams — [`nmp_read_session::open_read`]/[`close_read`] for a
//! full independent read-session per key, and
//! `ObservedProjectionRegistrar::open_observed_projection`/
//! `close_observed_projection` for a raw observed projection per key (29er's
//! group-tree uses BOTH, across two separate collections). These two
//! constructors are that shared plumbing, factored once so a facade crate
//! supplies only its own concept-specific `spec_for`/`projection_for`
//! closure and gets a working [`KeyedReadCollection`] back.
//!
//! # Building `K`/`C`
//!
//! `K` is the facade's own stable per-member identity (e.g. a group id); `C`
//! is the facade's own `Clone + PartialEq` descriptor — see
//! [`nmp_read_session::KeyedReadCollection`]'s module docs for the
//! `MemberKey`-injectivity rule and the exogenous-scalar-via-`Replace`
//! pattern (encode any value the descriptor depends on that is NOT part of
//! the key-set directly into `C`, e.g. an active account pubkey, so a value
//! change on a live key diffs to `Replace` instead of a hand-rolled
//! force-close+reopen of the whole collection).
//!
//! # No raw Trellis vocabulary here (#2858 Phase A)
//!
//! Both constructors, and every closure a caller writes against them, are
//! typed entirely in [`nmp_read_session::MemberKey`] — the NMP-owned wrapper
//! around `trellis_core::ResourceKey` — never the raw Trellis type. This
//! crate is a scanned public app/native/web-facing Rust surface; it does not
//! (and must not) depend on `trellis-core` directly.
//!
//! # Deferred UniFFI ergonomics
//!
//! Neither constructor is itself `#[uniffi::export]`ed here — `nmp-uniffi-
//! support` never calls `uniffi::setup_scaffolding!()` (crate-level doc), so
//! every export lives in the OWNING facade crate's namespace. A facade
//! exposes its own `open_<concept>_collection(app, members_json) -> Handle`
//! door (mirroring `sessions::open_feed`) that decodes its own JSON member
//! shape, builds `spec_for`/`projection_for` from its own reducer, and calls
//! one of these two constructors. A generic JSON-in door is NOT provided
//! here because the reducer/output a `ReadSpec` needs is inherently
//! concept-owned (D0) — baking one shape here would bake one app's surface.

use std::sync::Arc;

use nmp_core::substrate::{ObservedProjection, ObservedProjectionRegistrar};
use nmp_native_runtime::NmpApp;
use nmp_read_session::{
    close_read, open_read, KeyedReadCollection, MemberKey, ReadSpec, TeardownAction,
};

/// Builds a [`KeyedReadCollection`] whose members are each a full,
/// independent `nmp_read_session` read-session against `app` — its own
/// reducer, its own typed output, its own close handle (#3115 shape (b)).
///
/// `spec_for` builds one key's full [`ReadSpec`] from the derived
/// [`MemberKey`] (diagnostic identity — see [`KeyedReadCollection`]'s
/// module docs on why `C` must be self-describing) and the committed
/// descriptor `C`. This is the facade's own concern (its reducer, its
/// output encoder, its projection key derivation); this constructor owns
/// only the repeated `open_read`/`close_read` teardown wiring.
///
/// # Panics
///
/// Never in practice: a fresh [`KeyedReadCollection`] construction over an
/// empty graph cannot fail (same invariant `nmp_read_session::demand_set`
/// documents at its own construction site).
#[must_use]
pub fn keyed_read_session_collection<K, C>(
    app: Arc<NmpApp>,
    scope_debug_name: impl Into<String>,
    key_fn: impl Fn(&K) -> MemberKey + Send + Sync + 'static,
    spec_for: impl Fn(&MemberKey, &C) -> ReadSpec + Send + Sync + 'static,
) -> KeyedReadCollection<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    KeyedReadCollection::new(scope_debug_name, key_fn, move |member_key, command: C| {
        let spec = spec_for(member_key, &command);
        let handle = open_read(app.as_ref(), spec);
        let app_for_close = Arc::clone(&app);
        Box::new(move || {
            let _ = close_read(app_for_close.as_ref(), &handle);
        }) as TeardownAction
    })
    .expect("fresh KeyedReadCollection construction over an empty graph cannot fail")
}

/// Builds a [`KeyedReadCollection`] whose members are each a raw
/// `open_observed_projection`/`close_observed_projection` call against
/// `app` — no reducer, no typed output, just the admitted-event fan-out a
/// facade's own below-the-typed-boundary read model consumes directly
/// (#3115's "collection A" precedent: a group's live feed observed straight
/// off the kernel, no `nmp-read-session` machinery involved).
///
/// `projection_for` builds one key's [`ObservedProjection`] declaration
/// (including its own `observer` sink) from the derived [`MemberKey`] and
/// the committed descriptor `C`.
///
/// # Panics
///
/// Never in practice — see [`keyed_read_session_collection`]'s panics note.
#[must_use]
pub fn keyed_observed_projection_collection<K, C>(
    app: Arc<NmpApp>,
    scope_debug_name: impl Into<String>,
    key_fn: impl Fn(&K) -> MemberKey + Send + Sync + 'static,
    projection_for: impl Fn(&MemberKey, &C) -> ObservedProjection + Send + Sync + 'static,
) -> KeyedReadCollection<K, C>
where
    K: Clone + Ord + Send + Sync + 'static,
    C: Clone + PartialEq + Send + Sync + 'static,
{
    KeyedReadCollection::new(scope_debug_name, key_fn, move |member_key, command: C| {
        let decl = projection_for(member_key, &command);
        let id = app.open_observed_projection(decl);
        let app_for_close = Arc::clone(&app);
        Box::new(move || {
            app_for_close.close_observed_projection(id);
        }) as TeardownAction
    })
    .expect("fresh KeyedReadCollection construction over an empty graph cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;
    use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};
    use nmp_read_session::{InterestLifecycle, ReadDemand, ReadOutputEncoder, ReadReplayPolicy};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct NoopSink;
    impl ObservedProjectionSink for NoopSink {
        fn on_kernel_event(&self, _event: &KernelEvent) {}
    }

    #[test]
    fn keyed_read_session_collection_mounts_one_independent_read_per_key() {
        let app = Arc::new(nmp_native_runtime::new_app());
        let collection = keyed_read_session_collection::<String, String>(
            Arc::clone(&app),
            "test.keyed-read-session-collection",
            |group_id| MemberKey::new(group_id.clone()),
            |member_key, group_id| ReadSpec {
                projection_key: ProjectionRegistrationKey::Dynamic(
                    DynamicProjectionKey::app_owned(format!(
                        "test.group-feed.{}",
                        member_key.as_str()
                    ))
                    .unwrap(),
                ),
                demands: vec![ReadDemand {
                    filter_json: format!(r##"{{"kinds":[9],"#h":["{group_id}"]}}"##),
                    consumer_id: format!("group-feed::{group_id}"),
                    scope: 1,
                    relay_pin: None,
                    is_indexer_discovery: false,
                    lifecycle: InterestLifecycle::Tailing,
                    replay_limit: 32,
                    replay: ReadReplayPolicy::Structural,
                }],
                observer: Arc::new(NoopSink),
                output_encoder: Box::new(|| None) as ReadOutputEncoder,
                dependent_demands: Vec::new(),
                keep_open_without_live_demand: false,
            },
        );

        let mut desired = BTreeMap::new();
        desired.insert("group-1".to_string(), "group-1".to_string());
        desired.insert("group-2".to_string(), "group-2".to_string());
        collection.reconcile(desired);
        assert_eq!(collection.live_count(), 2);
        assert!(collection.full_recompute_matches());

        collection.close();
        assert_eq!(collection.live_count(), 0);
    }

    #[test]
    fn keyed_observed_projection_collection_mounts_one_raw_projection_per_key() {
        let app = Arc::new(nmp_native_runtime::new_app());
        let collection = keyed_observed_projection_collection::<String, String>(
            Arc::clone(&app),
            "test.keyed-observed-projection-collection",
            |group_id| MemberKey::new(group_id.clone()),
            |_member_key, group_id| ObservedProjection {
                observer: Arc::new(NoopSink),
                filter_json: format!(r##"{{"kinds":[9],"#h":["{group_id}"]}}"##),
                consumer_id: format!("group-feed::{group_id}"),
                scope: 1,
                relay_pin: None,
                is_indexer_discovery: false,
                lifecycle: InterestLifecycle::Tailing,
                replay_shapes: Vec::new(),
                replay_limit: 32,
            },
        );

        let mut desired = BTreeMap::new();
        desired.insert("group-1".to_string(), "group-1".to_string());
        collection.reconcile(desired);
        assert_eq!(collection.live_count(), 1);
        assert!(collection.full_recompute_matches());

        collection.close();
        assert_eq!(collection.live_count(), 0);
    }
}
