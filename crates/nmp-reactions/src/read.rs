//! The reaction-read demand: one NIP-01 `REQ` filter per target (#2758).
//!
//! Reuses the same kind:7 filter shape as the NIP-29-group-scoped reaction
//! read, swapping the group lane's `#h` routing tag for a direct `#e` target
//! tag. Kind:5 retractions are routed by the
//! read-session engine's dependent-demand stage once concrete reaction ids are
//! known. This module only builds the primary filter string; admission and
//! retraction folding are `nmp_nip25::ReactionAggregateProjection`'s job
//! (driven from `summary.rs`).

use nmp_nip25::KIND_REACTION;

use crate::target::ReactionTarget;

/// NIP-01 `REQ` filter selecting kind:7 reactions for `target`:
/// `{"kinds":[7],"#e":["<target>"]}`.
#[must_use]
pub fn reaction_filter_json(target: &ReactionTarget) -> String {
    let mut map = serde_json::Map::new();
    map.insert("kinds".to_string(), serde_json::json!([KIND_REACTION]));
    map.insert("#e".to_string(), serde_json::json!([target.as_str()]));
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
