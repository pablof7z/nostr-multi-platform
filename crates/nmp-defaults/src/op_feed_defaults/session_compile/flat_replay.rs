use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::KernelEventObserver;
use nmp_ffi::NmpApp;
use nmp_planner::InterestShape;

pub(super) fn replay_fixed_event_ids(
    app: &NmpApp,
    feed: &Arc<nmp_nip01::FlatFeed>,
    interests: &[(String, u32)],
) -> bool {
    let store = app.event_store_handle();
    let mut seen = BTreeSet::new();
    let mut changed = false;

    for (filter_json, _) in interests {
        let Some(shape) = InterestShape::from_filter_json(filter_json) else {
            continue;
        };
        for event_id in shape.event_ids {
            if !seen.insert(event_id.clone()) {
                continue;
            }
            let Some(event) = nmp_core::slots::event_by_id_from_store(&store, &event_id) else {
                continue;
            };
            let before = flat_visible_ids(feed);
            feed.on_kernel_event(&event);
            changed |= flat_visible_ids(feed) != before;
        }
    }

    changed
}

fn flat_visible_ids(feed: &nmp_nip01::FlatFeed) -> Vec<String> {
    feed.snapshot_current_window()
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}
