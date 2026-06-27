//! PR-B (#991/#979) typed-first decode for [`FeatureSnapshot`].
//!
//! Split out of `feature_snapshot.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). This module owns the FlatBuffers typed-sidecar path:
//! it reads kernel built-in projections via `nmp_core::typed_projections::*`
//! and host-registered projections via the protocol crates' public decoders.
//! The generic `payload:Value` tree is never read.

use crate::feature_snapshot::{
    AccountLine, DmConversationLine, FeatureSnapshot, GroupLine, HistoryRelayLine, MessageLine,
    OutboxLine, OutboxRelayLine, PublishHistoryLine, RelayEditLine, SummaryLine, WalletLine,
};
use crate::ui::nostr_user::profile_wire::ProfileWire;

pub(crate) fn feature_snapshot_from_flatbuffer(bytes: &[u8]) -> FeatureSnapshot {
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).unwrap_or_default();

    // Helper closure: find a sidecar entry by its projection KEY.
    let find = |key: &str| -> Option<&[u8]> {
        typed
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.payload.as_slice())
    };

    // accounts (key == schema_id == "accounts")
    let accounts = find(nmp_core::typed_projections::ACCOUNTS_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_accounts(b).ok())
        .map(|m| {
            m.accounts
                .into_iter()
                .map(|row| AccountLine {
                    id: row.id.clone(),
                    display: row
                        .display_name
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| row.npub.clone()),
                    npub: row.npub,
                    signer: crate::feature_snapshot::signer_label_for_kind(&row.signer_kind),
                    active: row.is_active,
                })
                .collect()
        })
        .unwrap_or_default();

    // active_account (key == schema_id == "active_account")
    let active_account = find(nmp_core::typed_projections::ACTIVE_ACCOUNT_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_active_account(b).ok())
        .and_then(|m| m.pubkey)
        .unwrap_or_default();

    // configured_relays (key == schema_id == "configured_relays")
    let configured_relays = find(nmp_core::typed_projections::CONFIGURED_RELAYS_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_configured_relays(b).ok())
        .map(|m| {
            m.relays
                .into_iter()
                .map(|row| RelayEditLine {
                    url: row.url,
                    role: row.role,
                })
                .collect()
        })
        .unwrap_or_default();

    // settings_hub (key == schema_id == "settings_hub")
    let settings_hub = find(nmp_core::typed_projections::SETTINGS_HUB_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_settings_hub(b).ok())
        .map(|m| SummaryLine {
            title: "Settings".to_string(),
            subtitle: crate::feature_snapshot::relay_count_subtitle(m.relay_count as u64),
        })
        .unwrap_or_else(|| SummaryLine {
            title: "Settings".to_string(),
            subtitle: String::new(),
        });

    // publish_outbox (key == schema_id == "publish_outbox")
    // aim.md §2 #4: title/preview/status_label removed from wire. TUI shell
    // computes them from raw kind/content/status (same as iOS/Android shells).
    let outbox = find(nmp_core::typed_projections::PUBLISH_OUTBOX_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_publish_outbox(b).ok())
        .map(|m| {
            m.items
                .into_iter()
                .map(|row| OutboxLine {
                    handle: row.handle,
                    title: outbox_kind_title(row.kind),
                    status_label: outbox_status_label(&row.status),
                    preview: outbox_preview(row.kind, &row.content),
                    can_retry: row.can_retry,
                    relays: row
                        .relays
                        .into_iter()
                        .map(|r| OutboxRelayLine {
                            relay_url: r.relay_url,
                            status_label: outbox_relay_status_label(&r.status),
                            reason: format_relay_reason_token(&r.relay_reason),
                            message: format_relay_message_token(&r.message),
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    // outbox_summary (key == schema_id == "outbox_summary")
    // aim.md §2 #4: title/subtitle removed from wire. TUI shell computes
    // them from raw per-status counters.
    let outbox_summary = find(nmp_core::typed_projections::OUTBOX_SUMMARY_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_outbox_summary(b).ok())
        .map(|m| SummaryLine {
            title: outbox_summary_title(m.total),
            subtitle: outbox_summary_subtitle(m.total, m.sending, m.retrying, m.queued, m.failed),
        })
        .unwrap_or_default();

    // publish_queue (key == schema_id == "publish_queue")
    let history = find(nmp_core::typed_projections::PUBLISH_QUEUE_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_publish_queue(b).ok())
        .map(publish_history_from_queue)
        .unwrap_or_default();

    // V-112 (ADR-0042): author_view / thread_view projections deleted.
    // SharedSnapshot decodes dynamic nmp.feed.author.* / nmp.feed.thread.*
    // FlatFeed registrations for the profile and detail panes.

    // Host-registered: nmp.nip17.dm_inbox (key == "nmp.nip17.dm_inbox")
    let dm_conversations = find("nmp.nip17.dm_inbox")
        .and_then(|b| nmp_nip17::decode_dm_inbox_snapshot(b).ok())
        .map(|m| {
            m.conversations
                .into_iter()
                .map(|conv| {
                    let peer_pubkey = conv.peer_pubkey.clone();
                    let peer_display = if peer_pubkey.is_empty() {
                        String::new()
                    } else {
                        nmp_core::display::short_npub(&peer_pubkey)
                    };
                    let messages = conv
                        .messages
                        .into_iter()
                        .map(|msg| MessageLine {
                            id: msg.id,
                            author: msg.sender_pubkey,
                            content: msg.content,
                            outgoing: msg.is_outgoing,
                        })
                        .collect::<Vec<_>>();
                    let latest = messages
                        .last()
                        .map(|m| m.content.clone())
                        .unwrap_or_default();
                    DmConversationLine {
                        peer_pubkey,
                        peer_display,
                        latest,
                        messages,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Host-registered: nmp.nip29.group_events (key == "nmp.nip29.group_events")
    let group_messages = find("nmp.nip29.group_events")
        .and_then(|b| nmp_nip29::decode_group_events_snapshot(b).ok())
        .map(|m| {
            m.events
                .into_iter()
                .map(|msg| MessageLine {
                    id: msg.id,
                    author: msg.pubkey,
                    content: msg.content,
                    // Group chat messages don't carry an is_outgoing flag;
                    // the TUI never renders group messages as outgoing.
                    outgoing: false,
                })
                .collect()
        })
        .unwrap_or_default();

    // Host-registered: nmp.nip29.discovered_groups (key == "nmp.nip29.discovered_groups")
    let discovered_groups = find("nmp.nip29.discovered_groups")
        .and_then(|b| nmp_nip29::decode_discovered_groups_snapshot(b).ok())
        .map(|m| {
            m.groups
                .into_iter()
                .map(|row| GroupLine {
                    host_relay_url: row.host_relay_url,
                    group_id: row.group_id.clone(),
                    name: row
                        .name
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| row.group_id.clone()),
                    about: row.about.unwrap_or_default(),
                    member_count: u64::from(row.member_count),
                    open: row.open,
                })
                .collect()
        })
        .unwrap_or_default();

    // Host-registered: nmp.follow_list (key == "nmp.follow_list",
    //   schema_id == "nmp.nip02.follow_list")
    let follow_count = typed
        .iter()
        .find(|p| p.key == "nmp.follow_list")
        .and_then(|p| nmp_nip02::decode_follow_list(&p.payload).ok())
        .map(|m| m.follows.len())
        .unwrap_or(0);

    // Host-registered: wallet (key == "wallet", schema_id == "nmp.nip47.wallet")
    let wallet = typed
        .iter()
        .find(|p| p.key == "wallet")
        .and_then(|p| nmp_nip47::decode_wallet_status(&p.payload).ok())
        .map(|m| WalletLine {
            status: m.status,
            relay_url: m.relay_url,
            wallet_npub: m.wallet_npub,
            balance_msats: m.balance_msats,
        })
        .unwrap_or_default();

    // ADR-0063 (#1671 Lane G): `resolved_profiles` vestigial decode deleted.
    // The `refs.profile` row-delta projection (merged into `AppState::ref_profiles`,
    // the shell's `RefProfileStore`) is the sole source of hydrated profile facts.
    // `AppState::profile(pubkey)` is the read path; `FeatureSnapshot` no longer
    // carries a profile map.

    FeatureSnapshot {
        accounts,
        active_account,
        outbox,
        outbox_summary,
        history,
        configured_relays,
        wallet,
        dm_conversations,
        group_messages,
        discovered_groups,
        follow_count,
        settings_hub,
    }
}

// ---------------------------------------------------------------------------
// Typed-path helper: build publish history from a decoded `PublishQueueModel`
// ---------------------------------------------------------------------------

fn publish_history_from_queue(
    model: nmp_core::typed_projections::PublishQueueModel,
) -> Vec<PublishHistoryLine> {
    model
        .entries
        .into_iter()
        .rev() // kernel appends, so reverse gives newest-first
        .filter(|row| !row.status.is_empty() && row.status != "accepted_locally")
        .take(20)
        .map(|row| {
            let relays = row
                .relay_outcomes
                .into_iter()
                .map(|r| HistoryRelayLine {
                    relay_url: r.relay_url,
                    status: r.status,
                    relay_reason: format_relay_reason_token(&r.relay_reason),
                    message: format_relay_message_token(&r.message),
                })
                .collect();
            PublishHistoryLine {
                event_id: row.event_id,
                kind: row.kind,
                title: outbox_kind_title(row.kind),
                status: row.status,
                can_retry: row.can_retry,
                relays,
            }
        })
        .collect()
}

pub(crate) fn profile_wire_from_card(
    key: &str,
    card: nmp_core::typed_projections::ProfileCardModel,
) -> ProfileWire {
    let pubkey = if card.pubkey.is_empty() {
        key.to_string()
    } else {
        card.pubkey
    };
    ProfileWire {
        npub: nmp_core::display::to_npub(&pubkey),
        npub_short: nmp_core::display::short_npub(&pubkey),
        pubkey,
        display_name: card
            .display_name
            .or(card.name)
            .filter(|value| !value.trim().is_empty()),
        about: nonempty(card.about),
        picture_url: card.picture_url.filter(|value| !value.trim().is_empty()),
        nip05: nonempty(card.nip05),
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

// ── Publish-outbox shell-side presentation helpers ─────────────────────────
//
// aim.md §2 #4: title/preview/status_label removed from the nmp-core wire.
// The TUI shell computes them here from raw kind/content/status, mirroring the
// iOS (`NotificationsView+OutboxRow.swift`) and Android display layers.

/// Format a raw relay-reason token (e.g. `"nip65_write"`) into the display
/// string the TUI renders. Parameterised tokens (`"discovery_indexer:{kind}"`,
/// `"recipient_inbox:{pubkey}"`) are parsed and formatted here.
/// Unknown tokens pass through verbatim.
fn format_relay_reason_token(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    if let Some(kind) = token.strip_prefix("discovery_indexer:") {
        return format!("Discovery indexer (kind {kind})");
    }
    if let Some(pubkey) = token.strip_prefix("recipient_inbox:") {
        return format!("Inbox relay for {pubkey}");
    }
    match token {
        "nip65_write" => "NIP-65 write relay".to_string(),
        "local_config" => "App relay (local config)".to_string(),
        "explicit" => "Explicit relay".to_string(),
        other => other.to_string(),
    }
}

/// Format a raw relay-message token (e.g. `"waiting_for_ok"`) into the display
/// string the TUI renders. Raw relay protocol error text passes through verbatim.
fn format_relay_message_token(token: &str) -> String {
    match token {
        "waiting_for_connection" => "Waiting for relay connection".to_string(),
        "waiting_for_ok" => "Waiting for relay OK".to_string(),
        "accepted" => "Relay accepted the event".to_string(),
        "timed_out" => "No response from relay".to_string(),
        other => other.to_string(),
    }
}

fn outbox_kind_title(kind: u32) -> String {
    match kind {
        0 => "Profile",
        1 => "Note",
        3 => "Contacts",
        7 => "Reaction",
        10002 => "Relay list",
        _ => "Event",
    }
    .to_string()
}

fn outbox_status_label(status: &str) -> String {
    match status {
        "sending" => "Sending",
        "retrying" => "Retrying",
        "queued" => "Queued",
        "failed" => "Failed",
        "pending" => "Pending",
        other => other,
    }
    .to_string()
}

fn outbox_relay_status_label(status: &str) -> String {
    match status {
        "sending" => "Sending",
        "ok" => "OK",
        "retrying" => "Retrying",
        "pending" => "Pending",
        "failed" => "Failed",
        other => other,
    }
    .to_string()
}

fn outbox_preview(kind: u32, content: &str) -> String {
    // Whether this kind's `content` is opaque ciphertext is a Nostr protocol
    // rule, not a shell decision — read the canonical predicate (#1769).
    if nmp_kinds::is_encrypted_content_kind(kind) {
        return "Encrypted event content hidden".to_string();
    }
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "Event with no text content".to_string();
    }
    // Truncate to 180 chars with ellipsis — mirrors the Rust kernel's old
    // `truncate(trimmed, 180)` helper now removed from nmp-core.
    let mut preview: String = trimmed.chars().take(180).collect();
    if trimmed.chars().count() > 180 {
        preview.push('…');
    }
    preview
}

fn outbox_summary_title(total: u32) -> String {
    if total == 0 {
        return "Nothing waiting".to_string();
    }
    let suffix = if total == 1 { "" } else { "es" };
    format!("{total} pending publish{suffix}")
}

fn outbox_summary_subtitle(
    total: u32,
    sending: u32,
    retrying: u32,
    queued: u32,
    failed: u32,
) -> String {
    if total == 0 {
        return "Your local outbox is clear.".to_string();
    }
    if retrying > 0 {
        return format!("{retrying} waiting to retry, {sending} currently sending.");
    }
    if sending > 0 {
        return format!("{sending} currently sending.");
    }
    if failed > 0 {
        return format!("{failed} failed.");
    }
    let _ = queued;
    "Waiting for relay connections.".to_string()
}

#[cfg(test)]
#[path = "feature_snapshot_typed_roundtrip_tests.rs"]
mod roundtrip_tests;

#[cfg(test)]
mod outbox_preview_tests {
    use super::outbox_preview;

    #[test]
    fn encrypted_kinds_hide_content_plaintext_shows_through() {
        // The leak this replaces (#1769): kinds 4/44/1059 hid content via a
        // hardcoded set. The placeholder must now come from the canonical
        // predicate — gift-wrap ciphertext is hidden...
        assert_eq!(
            outbox_preview(1059, "AAEC-this-would-be-base64-ciphertext"),
            "Encrypted event content hidden",
            "kind:1059 gift-wrap content must be hidden"
        );
        assert_eq!(
            outbox_preview(4, "legacy-nip04-ciphertext?iv=..."),
            "Encrypted event content hidden",
            "kind:4 NIP-04 DM content must be hidden"
        );

        // ...while a plaintext public note shows its content (NOT hidden). This
        // is the load-bearing negative: a regression that hid everything, or a
        // predicate that wrongly classified kind:1 as encrypted, fails here.
        assert_eq!(
            outbox_preview(1, "hello world"),
            "hello world",
            "kind:1 short text note content must render verbatim"
        );
        // kind:14 NIP-17 rumor content is plaintext — it must NOT be hidden,
        // even though its relay presence is privacy-gated elsewhere.
        assert_eq!(
            outbox_preview(14, "decrypted dm body"),
            "decrypted dm body",
            "kind:14 rumor content is plaintext and must render"
        );
    }
}
