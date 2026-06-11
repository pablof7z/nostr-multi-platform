//! `RelayStatusEntry` — one decoded relay-status row from the Tier-3
//! `relay_statuses` vector, plus its decode helper.
//!
//! Split out of `update_envelope.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). PR-B (#991/#979) added this surface for the
//! chirp-desktop typed-first migration.

use crate::transport::wire as fb;

/// One relay-status row decoded from the Tier-3 `relay_statuses` vector.
///
/// A field-for-field mirror of the subset of `RelayStatus` fields that
/// chirp-desktop renders (role, relay_url, connection, auth, events_rx, denied).
/// Additional fields (`reconnect_count`, `bytes_rx`, etc.) are in the wire frame
/// but not decoded here — extend as needed.
#[derive(Clone, Debug, Default)]
pub struct RelayStatusEntry {
    /// Role label (e.g. `"read"`, `"write"`, `"both"`).
    pub role: String,
    /// Relay WebSocket URL.
    pub relay_url: String,
    /// Connection state label (e.g. `"connected"`, `"ready"`, `"disconnected"`).
    pub connection: String,
    /// Auth status label (e.g. `""`, `"accepted"`, `"waiting"`).
    pub auth: String,
    /// Total relay events received on this connection.
    pub events_rx: u64,
    /// `true` when the relay rejected authentication with the `restricted` code.
    pub denied: bool,
}

/// Decode the `relay_statuses` vector off a Tier-3 `SnapshotFrame` into owned
/// [`RelayStatusEntry`] rows. Empty when the frame carries no relay statuses.
#[must_use]
pub(crate) fn decode_relay_statuses(snapshot: &fb::SnapshotFrame<'_>) -> Vec<RelayStatusEntry> {
    snapshot
        .relay_statuses()
        .map(|vec| {
            (0..vec.len())
                .map(|i| {
                    let rs = vec.get(i);
                    RelayStatusEntry {
                        role: rs.role().unwrap_or("").to_string(),
                        relay_url: rs.relay_url().unwrap_or("").to_string(),
                        connection: rs.connection().unwrap_or("").to_string(),
                        auth: rs.auth().unwrap_or("").to_string(),
                        events_rx: rs.events_rx(),
                        denied: rs.denied(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
