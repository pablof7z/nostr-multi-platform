//! Browser-safe home-feed composition.
//!
//! The native `nmp-defaults::register_op_feed_defaults` currently names
//! `nmp-ffi::NmpApp`, so the browser runtime wires the same OP-feed primitives
//! directly through `BrowserAppBuilder`'s `AppHost` registrars. The browser owns
//! no Nostr protocol logic here: follow parsing, note/repost admission, card
//! construction, and FlatBuffers encoding stay in the NIP crates.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::slots;
use nmp_core::substrate::{
    EmptySuppressionLookup, IdentityChangeRegistrar, ObservedProjectionReconciler,
    ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_nip01::op_feed::{op_feed_observer, register_op_feed, OP_FEED_SNAPSHOT_KEY};
use nmp_planner::InterestShape;

use crate::builder::BrowserAppBuilder;

type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

pub(crate) fn register_browser_home_feed<S>(builder: &BrowserAppBuilder<S>) {
    let (active_account_slot, event_store, contacts_lookup) = {
        let Ok(g) = builder.inner.lock() else { return };
        let Some(contacts_lookup) = g.contacts_lookup.as_ref().cloned() else {
            return;
        };
        (
            g.reducer.active_account_handle(),
            g.reducer.event_store_handle(),
            contacts_lookup,
        )
    };

    let follow_set = nmp_nip02::ActiveFollowSet::new(active_account_slot.clone());
    let follow_list_projection =
        nmp_nip02::FollowListProjection::new(active_account_slot.clone(), contacts_lookup);
    let event_lookup: nmp_feed::EventLookup =
        Arc::new(move |id| slots::event_by_id_from_arc(&event_store, id));

    let engine = register_op_feed(
        String::new(),
        follow_set.predicate(),
        Arc::clone(&event_lookup),
    );
    let observer = op_feed_observer(
        Arc::clone(&engine),
        event_lookup,
        Arc::new(EmptySuppressionLookup),
    );

    let follow_observer = ObservedProjectionReconciler::new(
        builder.observed_projection_registrar_handle(),
        follow_set.clone() as Arc<dyn ObservedProjectionSink>,
        "nmp.feed.home.follow_set",
        1,
        64,
        active_contact_list_shape(active_account_slot.clone()),
    );
    let feed_observer = ObservedProjectionReconciler::new(
        builder.observed_projection_registrar_handle(),
        observer as Arc<dyn ObservedProjectionSink>,
        "nmp.feed.home.engine",
        1,
        512,
        active_follow_feed_shape(active_account_slot, Arc::clone(&follow_set)),
    );

    // Eager sync for cold-start: account may already be set.
    follow_observer.sync();
    feed_observer.sync();

    let engine_for_follow_change = Arc::clone(&engine);
    let feed_for_follow_change = feed_observer.clone();
    follow_set.on_change(Box::new(move || {
        engine_for_follow_change.reset_for_perspective_change();
        feed_for_follow_change.sync();
    }));

    let follow_for_identity = Arc::clone(&follow_set);
    let follow_observer_for_identity = follow_observer.clone();
    let feed_observer_for_identity = feed_observer.clone();
    builder.register_identity_change_observer(move |_| {
        follow_for_identity.notify_account_changed();
        follow_observer_for_identity.sync();
        feed_observer_for_identity.sync();
    });

    builder.register_typed_snapshot_projection(OP_FEED_SNAPSHOT_KEY, move || {
        Some(nmp_core::TypedProjectionData {
            key: OP_FEED_SNAPSHOT_KEY.to_string(),
            schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip01::op_feed::encode_op_feed_snapshot(&engine.snapshot_current_window()),
            ..Default::default()
        })
    });

    builder.register_typed_snapshot_projection("nmp.follow_list", move || {
        nmp_nip02::typed_projection_entry(&follow_list_projection)
    });
}

fn active_contact_list_shape(active_account_slot: nmp_core::slots::ActiveAccountSlot) -> LiveShape {
    Arc::new(move || {
        let active = active_account_slot.lock().ok()?.clone()?;
        Some(InterestShape {
            authors: [active].into_iter().collect(),
            kinds: [nmp_kinds::KIND_CONTACT_LIST].into_iter().collect(),
            ..Default::default()
        })
    })
}

fn active_follow_feed_shape(
    active_account_slot: nmp_core::slots::ActiveAccountSlot,
    follow_set: Arc<nmp_nip02::ActiveFollowSet>,
) -> LiveShape {
    Arc::new(move || {
        let active = active_account_slot
            .lock()
            .ok()
            .and_then(|g| g.clone());
        if active.is_none() {
            return Some(public_home_feed_shape());
        }
        let authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
        if authors.is_empty() {
            return Some(public_home_feed_shape());
        }
        Some(InterestShape::timeline_for(authors, home_feed_kinds()))
    })
}

fn public_home_feed_shape() -> InterestShape {
    let mut shape = InterestShape::timeline_for(BTreeSet::new(), home_feed_kinds());
    shape.limit = Some(128);
    shape
}

fn home_feed_kinds() -> BTreeSet<u32> {
    nmp_nip18::try_acquisition_kinds_for_primary([nmp_kinds::KIND_SHORT_TEXT_NOTE])
        .unwrap_or_else(|_| [nmp_kinds::KIND_SHORT_TEXT_NOTE].into_iter().collect())
}
