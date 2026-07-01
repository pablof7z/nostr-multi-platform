use crate::planner::InterestShape;
use crate::store::{EventStore, RawEvent, StoreLogEntry, StoreQuery};

use super::predicate::raw_matches_shape;
use super::PullError;

pub(super) fn entry_matches_any_shape(
    store: &dyn EventStore,
    entry: &StoreLogEntry,
    raw: &RawEvent,
    compiled_shapes: &[(InterestShape, Vec<StoreQuery>)],
) -> Result<bool, PullError> {
    let mut provenance: Option<Vec<String>> = None;
    for (shape, queries) in compiled_shapes {
        if !raw_matches_shape(raw, queries, shape) {
            continue;
        }
        if relay_pin_matches(
            store,
            shape,
            entry.source_relay.as_deref(),
            raw,
            &mut provenance,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn relay_pin_matches(
    store: &dyn EventStore,
    shape: &InterestShape,
    source_relay: Option<&str>,
    raw: &RawEvent,
    current_provenance: &mut Option<Vec<String>>,
) -> Result<bool, PullError> {
    let Some(pin) = shape.relay_pin.as_deref() else {
        return Ok(true);
    };
    if source_relay == Some(pin) || source_relay == Some("local://publish") {
        return Ok(true);
    }
    if current_provenance.is_none() {
        let urls = raw
            .id_bytes()
            .map(|id| {
                store
                    .provenance_for(&id)
                    .map(|entries| entries.into_iter().map(|entry| entry.relay_url).collect())
            })
            .transpose()?
            .unwrap_or_default();
        *current_provenance = Some(urls);
    }
    Ok(current_provenance.as_ref().is_some_and(|urls| {
        urls.iter()
            .any(|url| url == pin || url == "local://publish")
    }))
}
