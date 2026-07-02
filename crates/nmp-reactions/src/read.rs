//! The reaction-read demand: one NIP-01 `REQ` filter per target (#2758).
//!
//! Reuses the exact filter shape already proven by the NIP-29-group-scoped
//! reaction read (`nmp-native-runtime::group_feed::reactions::
//! group_reactions_filter_json`) — `kinds:[5,7]` so relay-delivered NIP-09
//! deletions decrement the fold, same as the group lane — swapping that
//! lane's `#h` group-routing tag for a direct `#e` target tag. This module
//! only builds the filter string; admission and retraction folding are
//! `nmp_nip25::ReactionAggregateProjection`'s job (driven from `summary.rs`).

use nmp_nip25::{KIND_REACTION, KIND_REACTION_DELETE};

use crate::target::ReactionTarget;

/// NIP-01 `REQ` filter selecting kind:7 reactions AND their kind:5 NIP-09
/// deletions for `target`: `{"kinds":[5,7],"#e":["<target>"]}`. Kinds sorted
/// ascending for a stable wire shape.
#[must_use]
pub fn reaction_filter_json(target: &ReactionTarget) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "kinds".to_string(),
        serde_json::json!([KIND_REACTION_DELETE, KIND_REACTION]),
    );
    map.insert("#e".to_string(), serde_json::json!([target.as_str()]));
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
