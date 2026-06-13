//! PR-B (#991/#979) typed-first snapshot decode for the egui shell.
//!
//! Split out of `app.rs` to keep that file within the 500-LOC ceiling
//! (AGENTS.md). Decodes one transport payload into a fully-populated
//! [`Snapshot`] from the typed Tier-3 `SnapshotEnvelope` + per-projection
//! typed sidecars — the generic `payload:Value` tree is never read. Projection
//! payloads are re-materialised as `serde_json::Value` via `serde_json::json!`
//! (the `snapshot::*` payload structs derive `Deserialize` only) so the shell's
//! existing `snap.projection::<T>(key)` read sites keep working unchanged.

use crate::snapshot::Snapshot;

// ---------------------------------------------------------------------------
// Helper functions — typed OP-feed decode (mirrors chirp-tui approach)
// ---------------------------------------------------------------------------

/// Extract the typed OP-feed `nmp.feed.home` sidecar and re-serialize it as a
/// generic `Value` for insertion into the snapshot projections map.
///
/// Returns `None` when the projection is absent, the schema id does not match
/// [`nmp_nip01::OP_FEED_SCHEMA_ID`], or the FlatBuffers payload is corrupt.
/// Both of these cases fall back to the generic `Value` projection that the
/// snapshot already carries.
fn extract_home_feed_from_typed(
    projections: &[nmp_core::TypedProjectionData],
) -> Option<serde_json::Value> {
    let proj = projections
        .iter()
        .find(|p| p.key == "nmp.feed.home" && p.schema_id == nmp_nip01::OP_FEED_SCHEMA_ID)?;
    nmp_nip01::decode_op_feed_snapshot(&proj.payload)
        .ok()
        .and_then(|snapshot| serde_json::to_value(&snapshot).ok())
}

// ---------------------------------------------------------------------------
// PR-B typed-first snapshot decode (#991/#979)
//
// Replaces the former `payload:Value` decode. Every field comes from the typed
// Tier-3 `SnapshotEnvelope` or a per-projection typed sidecar. The shell still
// reads view payloads via `snap.projection::<T>(key)`, so we re-materialise the
// decoded models as `serde_json::Value` with the `json!` macro (the
// `snapshot::*` payload structs derive `Deserialize` only — never `Serialize`).
// ---------------------------------------------------------------------------

/// Map a kernel `ProfileCardModel` to the wire-shape `Value` the desktop
/// `snapshot::ProfileCard` deserialises from.
fn profile_card_value(card: &nmp_core::typed_projections::ProfileCardModel) -> serde_json::Value {
    serde_json::json!({
        "pubkey": card.pubkey,
        "npub": card.npub,
        "display_name": card.display_name,
        "picture_url": card.picture_url,
        "nip05": card.nip05,
        "about": card.about,
        "lnurl": card.lnurl,
    })
}

/// Decode one transport payload into a fully-populated [`Snapshot`] from typed
/// sources only. Returns `None` when the Tier-3 envelope itself fails to decode
/// (a malformed frame the shell should skip).
pub(crate) fn decode_snapshot_typed(payload: &[u8]) -> Option<Snapshot> {
    use nmp_core::typed_projections as tp;

    let envelope = nmp_core::decode_snapshot_envelope(payload).ok()?;
    let typed = nmp_core::decode_snapshot_typed_projections(payload).ok()?;

    let find = |key: &str| -> Option<&[u8]> {
        typed
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.payload.as_slice())
    };

    // --- Top-level kernel fields (envelope + identity/profile sidecars) ---
    let profile = find(tp::PROFILE_SCHEMA_ID)
        .and_then(|b| tp::decode_profile(b).ok())
        .map(|m| crate::snapshot::ProfileCard {
            pubkey: m.pubkey,
            npub: m.npub,
            display_name: m.display_name,
            picture_url: m.picture_url,
            nip05: m.nip05,
            about: m.about,
            lnurl: m.lnurl,
        })
        .unwrap_or_default();

    let active_account = find(tp::ACTIVE_ACCOUNT_SCHEMA_ID)
        .and_then(|b| tp::decode_active_account(b).ok())
        .and_then(|m| m.pubkey);

    let accounts = find(tp::ACCOUNTS_SCHEMA_ID)
        .and_then(|b| tp::decode_accounts(b).ok())
        .map(|m| {
            m.accounts
                .into_iter()
                .map(|row| crate::snapshot::AccountSummary {
                    pubkey: row.id,
                    display_name: row.display_name,
                    picture_url: row.picture_url,
                    is_active: row.is_active,
                })
                .collect()
        })
        .unwrap_or_default();

    let relay_statuses = envelope
        .relay_statuses
        .iter()
        .map(|rs| crate::snapshot::RelayStatus {
            role: rs.role.clone(),
            relay_url: rs.relay_url.clone(),
            connection: rs.connection.clone(),
            auth: rs.auth.clone(),
            events_rx: rs.events_rx,
            denied: rs.denied,
        })
        .collect();

    // --- Projection map (every key the shell reads via `snap.projection()`) ---
    let mut projections: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    // V-112 (ADR-0042): author_view / thread_view projections deleted.
    // Author and thread screens now read from dynamic flat-feed projections
    // registered by nmp_app_chirp_open_author_feed / _open_thread_feed under
    // "nmp.feed.author.<pubkey>" / "nmp.feed.thread.<event_id>" keys. These
    // are present in the typed sidecar list with schema_id "nmp.nip01.opfeed"
    // and are decoded/inserted below alongside the home feed.

    // nmp.feed.home — typed OP-feed sidecar (same path as chirp-tui).
    if let Some(feed) = extract_home_feed_from_typed(&typed) {
        projections.insert("nmp.feed.home".to_string(), feed);
    }

    // resolved_profiles — pubkey -> ProfileCard map (mention/display resolution).
    if let Some(m) =
        find(tp::RESOLVED_PROFILES_SCHEMA_ID).and_then(|b| tp::decode_resolved_profiles(b).ok())
    {
        let map: serde_json::Map<String, serde_json::Value> = m
            .entries
            .iter()
            .map(|(k, card)| (k.clone(), profile_card_value(card)))
            .collect();
        projections.insert("resolved_profiles".to_string(), serde_json::Value::Object(map));
    }

    // configured_relays — relay-edit rows for the Settings pane.
    if let Some(m) =
        find(tp::CONFIGURED_RELAYS_SCHEMA_ID).and_then(|b| tp::decode_configured_relays(b).ok())
    {
        let rows: Vec<serde_json::Value> = m
            .relays
            .iter()
            .map(|r| serde_json::json!({ "url": r.url, "role": r.role }))
            .collect();
        projections.insert(
            "configured_relays".to_string(),
            serde_json::Value::Array(rows),
        );
    }

    // action_stages — publish lifecycle rows (latest stage per correlation id).
    if let Some(m) =
        find(tp::ACTION_STAGES_SCHEMA_ID).and_then(|b| tp::decode_action_stages(b).ok())
    {
        let rows: Vec<serde_json::Value> = m
            .entries
            .into_iter()
            .filter_map(|(cid, history)| {
                let last = history.into_iter().last()?;
                Some(serde_json::json!({
                    "correlation_id": cid,
                    "stage": last.stage,
                    "reason": last.reason,
                }))
            })
            .collect();
        projections.insert("action_stages".to_string(), serde_json::Value::Array(rows));
    }

    // nmp.nip17.dm_inbox — DM conversations (host-registered sidecar).
    if let Some(m) = find("nmp.nip17.dm_inbox").and_then(|b| nmp_nip17::decode_dm_inbox_snapshot(b).ok())
    {
        let conversations: Vec<serde_json::Value> = m
            .conversations
            .into_iter()
            .map(|conv| {
                let peer_pubkey = conv.peer_pubkey.clone();
                let peer_display = if peer_pubkey.is_empty() {
                    String::new()
                } else {
                    nmp_core::display::short_npub(&peer_pubkey)
                };
                let messages: Vec<serde_json::Value> = conv
                    .messages
                    .into_iter()
                    .map(|msg| {
                        serde_json::json!({
                            "id": msg.id,
                            "author": msg.sender_pubkey,
                            "content": msg.content,
                            "outgoing": msg.is_outgoing,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "peer_pubkey": peer_pubkey,
                    "peer_display": peer_display,
                    "messages": messages,
                })
            })
            .collect();
        projections.insert(
            "nmp.nip17.dm_inbox".to_string(),
            serde_json::json!({ "conversations": conversations }),
        );
    }

    Some(Snapshot {
        rev: envelope.rev,
        running: envelope.running,
        last_error_toast: envelope.last_error_toast,
        relay_statuses,
        metrics: crate::snapshot::Metrics {
            note_events: 0,
            events_rx: envelope.events_rx,
            visible_items: 0,
            events_since_last_update: 0,
        },
        profile,
        active_account,
        accounts,
        projections,
    })
}

#[cfg(test)]
#[path = "snapshot_decode_roundtrip_tests.rs"]
mod roundtrip_tests;
