use std::collections::BTreeSet;
use std::sync::Arc;

use crate::FeedSessionHost;
use nmp_core::ObservedProjectionSink;

use super::source::AcquisitionInterest;

pub(super) fn replay_fixed_event_ids(
    app: &impl FeedSessionHost,
    feed: &Arc<nmp_feed::FlatFeed<nmp_feed::FeedRow>>,
    interests: &[AcquisitionInterest],
) -> bool {
    let store = app.event_store_handle();
    let mut seen = BTreeSet::new();
    let mut changed = false;

    for interest in interests {
        for event_id in &interest.shape.event_ids {
            if !seen.insert(event_id.clone()) {
                continue;
            }
            let Some(event) = nmp_core::slots::event_by_id_from_store(&store, event_id) else {
                continue;
            };
            let before = flat_visible_ids(feed);
            feed.on_kernel_event(&event);
            changed |= flat_visible_ids(feed) != before;
        }
    }

    changed
}

fn flat_visible_ids(feed: &nmp_feed::FlatFeed<nmp_feed::FeedRow>) -> Vec<String> {
    feed.snapshot_current_window()
        .cards
        .into_iter()
        .map(|card| card.card.canonical_row_id)
        .collect()
}
