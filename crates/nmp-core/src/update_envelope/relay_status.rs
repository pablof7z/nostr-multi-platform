//! `RelayStatusEntry` / `WireSubscriptionEntry` — decoded rows from the
//! Tier-3 `relay_statuses` / `wire_subscriptions` vectors, plus their
//! decode helpers and the entry-level encoders used by
//! [`super::encode_snapshot_frame`].
//!
//! Split out of `update_envelope.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). PR-B (#991/#979) added this surface for the
//! chirp-desktop typed-first migration and extended it when the generic
//! `payload:Value` emission was zeroed (every former JSON reader of
//! `relay_status` / `wire_subscriptions` now reads these typed rows).

use crate::transport::wire as fb;
use flatbuffers::{FlatBufferBuilder, WIPOffset};

/// One relay-status row decoded from the Tier-3 `relay_statuses` vector (or
/// the singular `relay_status` aggregate).
///
/// A field-for-field mirror of the subset of `RelayStatus` fields that
/// Rust consumers read (role, relay_url, connection, auth, events_rx, denied).
/// Additional fields (`reconnect_count`, `bytes_rx`, etc.) are in the wire frame
/// but not decoded here — extend as needed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

/// One wire-subscription row decoded from the Tier-3 `wire_subscriptions`
/// vector. Subset mirror (id/relay/state) of `WireSubscriptionStatus`; the
/// timing/counter fields are in the wire frame but not decoded here — extend
/// as needed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WireSubscriptionEntry {
    /// Wire-level subscription id.
    pub wire_id: String,
    /// Relay the subscription is open against.
    pub relay_url: String,
    /// Lifecycle state label (e.g. `"open"`, `"eose"`, `"closed"`,
    /// `"closed_by_relay"`).
    pub state: String,
}

/// Decode one `RelayStatus` table into an owned [`RelayStatusEntry`].
fn relay_status_entry(rs: &fb::RelayStatus<'_>) -> RelayStatusEntry {
    RelayStatusEntry {
        role: rs.role().unwrap_or("").to_string(),
        relay_url: rs.relay_url().unwrap_or("").to_string(),
        connection: rs.connection().unwrap_or("").to_string(),
        auth: rs.auth().unwrap_or("").to_string(),
        events_rx: rs.events_rx(),
        denied: rs.denied(),
    }
}

/// Decode the `relay_statuses` vector off a Tier-3 `SnapshotFrame` into owned
/// [`RelayStatusEntry`] rows. Empty when the frame carries no relay statuses.
#[must_use]
pub(crate) fn decode_relay_statuses(snapshot: &fb::SnapshotFrame<'_>) -> Vec<RelayStatusEntry> {
    snapshot
        .relay_statuses()
        .map(|vec| (0..vec.len()).map(|i| relay_status_entry(&vec.get(i))).collect())
        .unwrap_or_default()
}

/// Decode the singular `relay_status` aggregate off a Tier-3 `SnapshotFrame`.
/// `None` when the frame carries no aggregate (e.g. non-kernel producers).
#[must_use]
pub(crate) fn decode_relay_status_aggregate(
    snapshot: &fb::SnapshotFrame<'_>,
) -> Option<RelayStatusEntry> {
    snapshot.relay_status().map(|rs| relay_status_entry(&rs))
}

/// Decode the `wire_subscriptions` vector off a Tier-3 `SnapshotFrame` into
/// owned [`WireSubscriptionEntry`] rows. Empty when absent.
#[must_use]
pub(crate) fn decode_wire_subscriptions(
    snapshot: &fb::SnapshotFrame<'_>,
) -> Vec<WireSubscriptionEntry> {
    snapshot
        .wire_subscriptions()
        .map(|vec| {
            (0..vec.len())
                .map(|i| {
                    let ws = vec.get(i);
                    WireSubscriptionEntry {
                        wire_id: ws.wire_id().unwrap_or("").to_string(),
                        relay_url: ws.relay_url().unwrap_or("").to_string(),
                        state: ws.state().unwrap_or("").to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Encode one [`RelayStatusEntry`] as a `RelayStatus` table (subset fields
/// only — the kernel's production encoder in `KernelSnapshot::encode_tier3`
/// writes the full table; this is the test/auxiliary-producer path).
pub(crate) fn encode_relay_status_entry<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    entry: &RelayStatusEntry,
) -> WIPOffset<fb::RelayStatus<'bldr>> {
    let role = builder.create_string(&entry.role);
    let relay_url = builder.create_string(&entry.relay_url);
    let connection = builder.create_string(&entry.connection);
    let auth = builder.create_string(&entry.auth);
    fb::RelayStatus::create(
        builder,
        &fb::RelayStatusArgs {
            role: Some(role),
            relay_url: Some(relay_url),
            connection: Some(connection),
            auth: Some(auth),
            events_rx: entry.events_rx,
            denied: entry.denied,
            ..Default::default()
        },
    )
}

/// Encode a [`WireSubscriptionEntry`] slice as a `wire_subscriptions` vector.
/// `None` when empty so the optional slot is omitted entirely.
pub(crate) fn encode_wire_subscriptions<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    entries: &[WireSubscriptionEntry],
) -> Option<
    WIPOffset<
        flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'bldr>>>,
    >,
> {
    if entries.is_empty() {
        return None;
    }
    let offsets: Vec<_> = entries
        .iter()
        .map(|entry| {
            let wire_id = builder.create_string(&entry.wire_id);
            let relay_url = builder.create_string(&entry.relay_url);
            let state = builder.create_string(&entry.state);
            fb::WireSubscriptionStatus::create(
                builder,
                &fb::WireSubscriptionStatusArgs {
                    wire_id: Some(wire_id),
                    relay_url: Some(relay_url),
                    state: Some(state),
                    ..Default::default()
                },
            )
        })
        .collect();
    Some(builder.create_vector(&offsets))
}

/// Encode a [`RelayStatusEntry`] slice as a `relay_statuses` vector. `None`
/// when empty so the optional slot is omitted entirely.
pub(crate) fn encode_relay_statuses<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    entries: &[RelayStatusEntry],
) -> Option<
    WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::RelayStatus<'bldr>>>>,
> {
    if entries.is_empty() {
        return None;
    }
    let offsets: Vec<_> =
        entries.iter().map(|entry| encode_relay_status_entry(builder, entry)).collect();
    Some(builder.create_vector(&offsets))
}
