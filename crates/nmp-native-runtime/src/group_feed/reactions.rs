//! Group-scoped NIP-25 reaction-aggregate typed read-session composition.
//!
//! Provides the `open_nip25_group_reactions_session[_with_reader]` /
//! `close_nip25_group_reactions_session` entry points; the shared open/teardown
//! plumbing lives in the sibling `feed` submodule.
//!
//! ## Composition boundary
//!
//! NIP-25 owns kind:7 reaction semantics (the
//! [`ReactionAggregateProjection`](nmp_nip25::ReactionAggregateProjection) fold)
//! and NIP-29 owns the `["h", local_id]` group routing; neither crate names the
//! other. The *group-scoped reaction view* is composed HERE, at the app layer,
//! by feeding the kind-agnostic reaction aggregator a relay-pinned `#h` +
//! `kinds:[7]` interest so it only ever folds the selected group's reactions.

use std::sync::Arc;

use nmp_nip25::{
    encode_reaction_aggregate_snapshot, ReactionAggregateProjection, KIND_REACTION,
    KIND_REACTION_DELETE, REACTION_AGGREGATE_FILE_IDENTIFIER, REACTION_AGGREGATE_SCHEMA_ID,
    REACTION_AGGREGATE_SCHEMA_VERSION,
};
use nmp_nip29::group_id::GroupId;

use crate::app_struct::NmpApp;

use super::{
    Nip25GroupReactionsHandle, Nip25GroupReactionsSession, GROUP_REACTIONS_CONSUMER,
    GROUP_REACTIONS_KEY, GROUP_REACTIONS_PROJECTION_TOKEN, SCOPE_GLOBAL,
};

impl NmpApp {
    /// Open the group-scoped NIP-25 reaction-aggregate typed read session for
    /// one group. Hydrating: a view opened after the group's kind:7 reactions
    /// were cached catches them up (#2088), then tails live. Pinned `Global`
    /// (the group host relay). Singleton: re-opening replaces the prior view.
    #[must_use]
    pub fn open_nip25_group_reactions_session(
        &self,
        descriptor: Nip25GroupReactionsSession,
    ) -> Nip25GroupReactionsHandle {
        let (handle, _) = self.open_nip25_group_reactions_session_with_reader(descriptor);
        handle
    }

    /// Open a group-scoped reaction-aggregate typed read session and return the
    /// canonical projection reader.
    ///
    /// The returned [`ReactionAggregateProjection`] is the same `Arc` registered
    /// as the observed projection and used by the `"nmp.nip25.reactions"` typed
    /// sidecar. Callers must not open a second reaction observer; use this
    /// reader and keep the sidecar, relay-pinned interest, and hydration
    /// single-owned by this door.
    #[must_use]
    pub fn open_nip25_group_reactions_session_with_reader(
        &self,
        descriptor: Nip25GroupReactionsSession,
    ) -> (Nip25GroupReactionsHandle, Arc<ReactionAggregateProjection>) {
        let Nip25GroupReactionsSession {
            group_id,
            active_pubkey,
        } = descriptor;
        let relay_pin = Some(group_id.host_relay_url.clone());
        let filter_json = group_reactions_filter_json(&group_id);
        let projection = Arc::new(ReactionAggregateProjection::new(Some(active_pubkey)));
        let projection_reader = Arc::clone(&projection);

        let projection_for_output = Arc::clone(&projection);
        let output_encoder: nmp_read_session::ReadOutputEncoder = Box::new(move || {
            let snapshot = projection_for_output.snapshot();
            Some(nmp_core::TypedProjectionData {
                key: GROUP_REACTIONS_KEY.to_string(),
                schema_id: REACTION_AGGREGATE_SCHEMA_ID.to_string(),
                schema_version: REACTION_AGGREGATE_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(REACTION_AGGREGATE_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_reaction_aggregate_snapshot(&snapshot),
                ..Default::default()
            })
        });

        let read_handle = self.open_group_feed(
            GROUP_REACTIONS_PROJECTION_TOKEN,
            GROUP_REACTIONS_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            filter_json,
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            output_encoder,
        );
        (Nip25GroupReactionsHandle { read_handle }, projection_reader)
    }

    /// Close the group-reactions typed read session represented by `handle`.
    /// Idempotent (D6).
    pub fn close_nip25_group_reactions_session(&self, handle: Nip25GroupReactionsHandle) {
        self.close_group_feed_handle(&handle.read_handle);
    }
}

/// NIP-01 `REQ` filter for one group's reactions:
/// `{"kinds":[5,7],"#h":["<id>"]}`.
///
/// This is the app-layer composition seam — it combines the NIP-25 reaction
/// kind (7) AND its NIP-09 deletion kind (5) with the NIP-29 `h`-tag group
/// routing. Observing kind:5 is what lets a relay-delivered retraction reach the
/// aggregate so the count decrements and the reactor's `mine` handle clears.
/// The host-relay pin is attached separately by `open_group_feed` (mirroring
/// `GroupEventsQuery::filter_json`), so this wire filter carries only `kinds` +
/// `#h`. Kinds are sorted ascending so the wire shape is stable.
fn group_reactions_filter_json(group_id: &GroupId) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "kinds".to_string(),
        serde_json::json!([KIND_REACTION_DELETE, KIND_REACTION]),
    );
    map.insert("#h".to_string(), serde_json::json!([group_id.local_id]));
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_carries_kinds_5_7_and_h_only() {
        let group = GroupId::new("wss://groups.example.com", "room-a");
        let filter = group_reactions_filter_json(&group);
        let v: serde_json::Value = serde_json::from_str(&filter).unwrap();
        // Reactions (7) AND their NIP-09 deletions (5) so relay-delivered
        // retractions decrement the aggregate.
        assert_eq!(v["kinds"], serde_json::json!([5, 7]));
        assert_eq!(v["#h"], serde_json::json!(["room-a"]));
        assert!(v.get("relay_pin").is_none());
        // The interest planner must accept the composed filter.
        assert!(nmp_planner::InterestShape::from_filter_json(&filter).is_some());
    }
}
