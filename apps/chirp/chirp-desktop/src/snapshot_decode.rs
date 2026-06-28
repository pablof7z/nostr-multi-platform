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

/// Extract typed OP-feed sidecars and re-serialize each as a generic `Value`
/// for insertion into the snapshot projections map under its original key.
///
/// Returns only valid `nmp.feed.*` projections with the NOFS schema. Absent,
/// wrong-schema, or corrupt sidecars are ignored; desktop has no generic
/// `payload:Value` fallback.
fn extract_op_feeds_from_typed(
    projections: &[nmp_core::TypedProjectionData],
) -> Vec<(String, serde_json::Value)> {
    projections
        .iter()
        .filter(|p| p.key.starts_with("nmp.feed.") && p.schema_id == nmp_nip01::OP_FEED_SCHEMA_ID)
        .filter_map(|proj| {
            nmp_nip01::decode_op_feed_snapshot(&proj.payload)
                .ok()
                .and_then(|snapshot| serde_json::to_value(&snapshot).ok())
                .map(|value| (proj.key.clone(), value))
        })
        .collect()
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

/// Decode one transport payload into a fully-populated [`Snapshot`] from typed
/// sources only. Returns `None` when the Tier-3 envelope itself fails to decode
/// (a malformed frame the shell should skip).
pub(crate) fn decode_snapshot_typed(
    payload: &[u8],
    ref_profiles: &mut nmp_core::refs::RefProfileStore,
) -> Option<Snapshot> {
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
            npub: nmp_core::display::to_npub(&m.pubkey),
            pubkey: m.pubkey,
            display_name: m.display_name,
            name: m.name,
            raw_display_name: m.raw_display_name,
            display_name_camel: m.display_name_camel,
            picture_url: m.picture_url,
            banner: m.banner,
            website: m.website,
            nip05: m.nip05,
            about: m.about,
            lud16: m.lud16,
            lud06: m.lud06,
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
    // Author and thread screens now read from handle-opened flat-feed projections under
    // "nmp.feed.author.<pubkey>" / "nmp.feed.thread.<event_id>" keys. These
    // are present in the typed sidecar list with schema_id "nmp.nip01.opfeed"
    // and are decoded/inserted below alongside the home feed.

    // nmp.feed.* — typed OP-feed sidecars. Home, author, and thread feeds all
    // use the same NOFS schema and differ only by their projection key.
    for (key, feed) in extract_op_feeds_from_typed(&typed) {
        projections.insert(key, feed);
    }

    // nmp.follow_list — active account's NIP-02 follow set. Desktop consumes
    // the Rust-owned projection for button state and owns no local follow cache.
    if let Some(m) = find("nmp.follow_list").and_then(|b| nmp_nip02::decode_follow_list(b).ok()) {
        if let Ok(value) = serde_json::to_value(&m) {
            projections.insert("nmp.follow_list".to_string(), value);
        }
    }

    // ADR-0063 (#1671 Lane F): `refs.profile` replaces the `resolved_profiles`
    // projection. The sidecar is a per-KEY row-delta batch (only changed/cleared
    // rows, or a baseline on identity change), so it MUST be merged into the
    // persistent RefRowCache the reader thread holds — not decoded per-frame.
    // The merged full set is materialised into `refs_profiles` below.
    if let Some(entry) = typed
        .iter()
        .find(|p| p.key == nmp_core::refs::REFS_PROFILE_KEY)
    {
        ref_profiles.apply_sidecar(&entry.payload, envelope.session_id, envelope.snapshot_epoch);
    }
    let refs_profiles: std::collections::HashMap<String, crate::snapshot::ProfileCard> =
        ref_profiles
            .profiles()
            .into_iter()
            .map(|(k, card)| (k, crate::snapshot::ProfileCard::from_model(card)))
            .collect();

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

    // signer_state — unified remote-signer health (KSST typed sidecar, ADR-0048).
    // Absent when no remote signer is active (the JSON closure also emits null
    // then — so no entry is added in that case, which is the correct absence
    // signal for the Settings pane).
    if let Some(m) = find(nmp_core::typed_projections::SIGNER_STATE_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_signer_state(b).ok())
    {
        projections.insert(
            nmp_core::typed_projections::SIGNER_STATE_SCHEMA_ID.to_string(),
            serde_json::json!({
                "signer_kind": m.signer_kind,
                "state": m.state,
                "is_ready": m.is_ready,
                "is_failed": m.is_failed,
                "reason": m.reason,
            }),
        );
    }

    // bunker_handshake — NIP-46 connect-QR progress (KBHS typed sidecar).
    // Absent when no handshake is in flight (mirrors the JSON null semantics).
    if let Some(m) = find(nmp_core::typed_projections::BUNKER_HANDSHAKE_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_bunker_handshake(b).ok())
    {
        projections.insert(
            nmp_core::typed_projections::BUNKER_HANDSHAKE_SCHEMA_ID.to_string(),
            serde_json::json!({
                "stage": m.stage,
                "is_in_flight": m.is_in_flight,
                "is_terminal_success": m.is_terminal_success,
                "is_failed": m.is_failed,
                "can_cancel": m.can_cancel,
                "message": m.message,
                // #1711 — the stable progress code (shells localize it).
                "progress_code": m.progress_code,
            }),
        );
    }

    // nip46_onboarding — static signer-app probe table + handshake state (KN46
    // typed sidecar).  Always emitted by the kernel (never null) so always
    // decoded and inserted.
    if let Some(m) = find(nmp_core::typed_projections::NIP46_ONBOARDING_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_nip46_onboarding(b).ok())
    {
        let apps: Vec<serde_json::Value> = m
            .signer_apps
            .iter()
            .map(|a| {
                // `display_label` was removed from the wire (#1712, D7/D27); the
                // raw `scheme` token is enough for the desktop shell, which has
                // no onboarding-label surface that consumes a brand name.
                serde_json::json!({
                    "scheme": a.scheme,
                    "signer_kind": a.signer_kind,
                })
            })
            .collect();
        projections.insert(
            nmp_core::typed_projections::NIP46_ONBOARDING_SCHEMA_ID.to_string(),
            serde_json::json!({
                "signer_apps": apps,
                "stage_kind": m.stage_kind,
                // #1711 — the stable progress code (shells localize it).
                "progress_code": m.progress_code,
                "progress_message": m.progress_message,
                "is_in_flight": m.is_in_flight,
                "is_terminal_success": m.is_terminal_success,
                "is_failed": m.is_failed,
                "can_cancel": m.can_cancel,
            }),
        );
    }

    // nmp.nip17.dm_inbox — DM conversations (host-registered sidecar).
    if let Some(m) =
        find("nmp.nip17.dm_inbox").and_then(|b| nmp_nip17::decode_dm_inbox_snapshot(b).ok())
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

    // Issue #1283 Phase 1: the typed `refs.event.envelopes` (`NEMB`) sidecar —
    // the SAME pre-resolved embed map Chirp iOS consumes. Desktop is a
    // typed-frame shell (no JSON `payload`), so this typed sidecar is the only
    // way it can render embeds; `render::note_body` looks an `EventRef` up by
    // `primary_id` here instead of drawing a `↗ note` placeholder. Best-effort
    // (D6): a malformed buffer degrades to an empty map.
    let embeds: std::collections::HashMap<String, nmp_content::EmbeddedEventEnvelope> =
        find(nmp_content::wire::EMBED_SIDECAR_SCHEMA_ID)
            .and_then(|b| nmp_content::wire::decode_ref_event_envelopes(b).ok())
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();

    Some(Snapshot {
        rev: envelope.rev,
        running: envelope.running,
        last_error_toast: envelope.last_error_toast,
        relay_statuses,
        metrics: crate::snapshot::Metrics {
            note_events: envelope.note_events,
            events_rx: envelope.events_rx,
            visible_items: envelope.visible_items.min(usize::MAX as u64) as usize,
            events_since_last_update: envelope.events_since_last_update,
        },
        profile,
        active_account,
        accounts,
        projections,
        embeds,
        refs_profiles,
    })
}

#[cfg(test)]
#[path = "snapshot_decode_roundtrip_tests.rs"]
mod roundtrip_tests;

#[cfg(test)]
#[path = "snapshot_decode_feed_tests.rs"]
mod feed_tests;

#[cfg(test)]
mod signer_projection_decode_tests {
    //! Unit tests that verify the three actor-owned signer projections
    //! (signer_state / bunker_handshake / nip46_onboarding) round-trip through
    //! the projections-map → Snapshot::projection::<T> path used by the
    //! desktop Settings pane.
    //!
    //! These tests exercise the *JSON-materialisation* step (the
    //! `serde_json::json!{...}` insertion that `decode_snapshot_typed` performs
    //! after decoding the FlatBuffers sidecar) and the subsequent
    //! `Snapshot::projection::<T>` deserialisation.  The FlatBuffers encode →
    //! decode round-trips are already covered by the in-crate
    //! `*_fb_tests.rs` files in nmp-core.

    use crate::snapshot::{BunkerHandshakeStatus, SignerStatus};

    /// Verifies that the JSON shape `decode_snapshot_typed` inserts under
    /// `"signer_state"` can be deserialised into `SignerStatus`.
    #[test]
    fn signer_state_json_materialises_into_snapshot_type() {
        let v = serde_json::json!({
            "signer_kind": "nip46",
            "state": "ready",
            "is_ready": true,
            "is_failed": false,
            "reason": null,
        });
        let status: SignerStatus = serde_json::from_value(v)
            .expect("SignerStatus must deserialize from the signer_state projection JSON");
        assert_eq!(status.signer_kind, "nip46");
        assert_eq!(status.state, "ready");
        assert!(status.is_ready);
        assert!(!status.is_failed);
        assert!(status.reason.is_none());
    }

    /// Verifies that the JSON shape `decode_snapshot_typed` inserts under
    /// `"bunker_handshake"` can be deserialised into `BunkerHandshakeStatus`.
    #[test]
    fn bunker_handshake_json_materialises_into_snapshot_type() {
        let v = serde_json::json!({
            "stage": "waiting_for_approval",
            "is_in_flight": true,
            "is_terminal_success": false,
            "is_failed": false,
            "can_cancel": true,
            "message": "Approve in your signer app",
        });
        let status: BunkerHandshakeStatus = serde_json::from_value(v).expect(
            "BunkerHandshakeStatus must deserialize from the bunker_handshake projection JSON",
        );
        assert_eq!(status.stage, "waiting_for_approval");
        assert!(status.is_in_flight);
        assert!(!status.is_terminal_success);
        assert!(!status.is_failed);
        assert!(status.can_cancel);
        assert_eq!(
            status.message.as_deref(),
            Some("Approve in your signer app")
        );
    }

    /// Verifies that the JSON shape `decode_snapshot_typed` inserts under
    /// `"bunker_handshake"` for a terminal-success state surfaces the correct
    /// flags (the Settings pane uses `is_terminal_success` to clear the QR).
    #[test]
    fn bunker_handshake_terminal_success_surfaces_correctly() {
        let v = serde_json::json!({
            "stage": "complete",
            "is_in_flight": false,
            "is_terminal_success": true,
            "is_failed": false,
            "can_cancel": false,
            "message": null,
        });
        let status: BunkerHandshakeStatus = serde_json::from_value(v)
            .expect("terminal-success BunkerHandshakeStatus must deserialize");
        assert!(status.is_terminal_success);
        assert!(!status.is_in_flight);
        assert!(!status.is_failed);
    }

    /// Verifies the JSON shape produced from `nmp_nip02::decode_follow_list`
    /// deserialises into the desktop snapshot mirror.
    #[test]
    fn follow_list_json_materialises_into_snapshot_type() {
        let v = serde_json::json!({
            "follows": [
                { "pubkey": "aa" },
                { "pubkey": "bb" }
            ]
        });
        let snapshot: crate::snapshot::FollowListSnapshot = serde_json::from_value(v)
            .expect("FollowListSnapshot must deserialize from nmp.follow_list JSON");
        assert_eq!(snapshot.follows.len(), 2);
        assert_eq!(snapshot.follows[0].pubkey, "aa");
        assert_eq!(snapshot.follows[1].pubkey, "bb");
    }
}
