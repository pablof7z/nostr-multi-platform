use super::{MarmotProjection, hex_encode};
use mdk_core::prelude::group_types::GroupState;

impl MarmotProjection {
    /// Build the all-groups messages map for the `"nmp.marmot.messages"` push
    /// projection (ADR-0039, V-107 Rust leg).
    ///
    /// Returns a `serde_json::Value::Object` keyed by `group_id_hex` →
    /// newest-N [`crate::projection::payload::MarmotMessageRow`] JSON array for
    /// every joined group. Bounded by `page` rows per group (typically
    /// `DEFAULT_MESSAGE_PAGE` = 200).
    ///
    /// Reads from the MDK SQLite message store directly — already-decrypted rows,
    /// no re-decrypt per tick. D8 compliant: cheap, non-blocking.
    /// D6: poisoned mutex → empty JSON object.
    #[must_use]
    pub fn messages_all_groups_json(&self, page: usize) -> serde_json::Value {
        self.with_inner(|h| {
            let group_ids: Vec<String> = h
                .service()
                .get_groups()
                .map(|gs| {
                    gs.into_iter()
                        .filter(|g| g.state == GroupState::Active)
                        .map(|g| hex_encode(g.mls_group_id.as_slice()))
                        .collect()
                })
                .unwrap_or_default();
            let mut map = serde_json::Map::with_capacity(group_ids.len());
            for gid_hex in group_ids {
                let rows = crate::projection::ops::group_messages(h, &gid_hex, page);
                map.insert(
                    gid_hex,
                    serde_json::to_value(rows).unwrap_or(serde_json::Value::Array(vec![])),
                );
            }
            serde_json::Value::Object(map)
        })
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    }

    /// Structured sibling of [`Self::messages_all_groups_json`] for the typed
    /// FlatBuffers sidecar (ADR-0037, Wave A). Returns the SAME per-group data
    /// the JSON projection emits — `(group_id_hex, newest-N rows)` for every
    /// joined group — as native Rust structs instead of a `serde_json::Value`
    /// map, so [`crate::wire::messages_fb`] can encode them without re-parsing
    /// JSON.
    ///
    /// This is an additive read path: the authoritative JSON projection above is
    /// untouched and stays the source of truth. The two methods each issue an
    /// independent MDK read per tick (the typed sidecar is emitted alongside the
    /// JSON one); they are NOT merged so the JSON projection's wire behaviour is
    /// unchanged. The returned vector is in `get_groups()` order;
    /// [`crate::wire::messages_fb::encode_marmot_messages`] sorts by
    /// `group_id_hex` for a deterministic wire. D8 compliant: cheap,
    /// non-blocking. D6: poisoned mutex → empty vector.
    #[must_use]
    pub fn messages_all_groups(
        &self,
        page: usize,
    ) -> Vec<(String, Vec<crate::projection::payload::MarmotMessageRow>)> {
        self.with_inner(|h| {
            let group_ids: Vec<String> = h
                .service()
                .get_groups()
                .map(|gs| {
                    gs.into_iter()
                        .filter(|g| g.state == GroupState::Active)
                        .map(|g| hex_encode(g.mls_group_id.as_slice()))
                        .collect()
                })
                .unwrap_or_default();
            group_ids
                .into_iter()
                .map(|gid_hex| {
                    let rows = crate::projection::ops::group_messages(h, &gid_hex, page);
                    (gid_hex, rows)
                })
                .collect()
        })
        .unwrap_or_default()
    }
}
