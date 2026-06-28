//! JSON-fixture decode helpers for [`FeatureSnapshot`].
//!
//! Split out of `feature_snapshot.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). These parse the generic `projections` JSON tree and
//! are used ONLY by `FeatureSnapshot::from_projections` (the test/dev fixture
//! path — ADR-0037). The live FlatBuffers path lives in `feature_snapshot_typed`.

use serde_json::Value;

use crate::feature_snapshot::{
    relay_count_subtitle, AccountLine, DmConversationLine, GroupLine, HistoryRelayLine,
    MessageLine, OutboxLine, OutboxRelayLine, PublishHistoryLine, RelayEditLine, SummaryLine,
    WalletLine,
};

pub(crate) fn accounts_from(projections: &Value) -> Vec<AccountLine> {
    projections
        .get("accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| AccountLine {
            id: string_field(row, "id"),
            display: first_nonempty(row, &["display_name", "displayName", "npub"]),
            npub: string_field(row, "npub"),
            signer: crate::feature_snapshot::signer_label_for_kind(&first_nonempty(
                row,
                &["signer_kind", "signerKind"],
            )),
            active: bool_field(row, "is_active") || bool_field(row, "isActive"),
        })
        .collect()
}

pub(crate) fn outbox_from(projections: &Value) -> Vec<OutboxLine> {
    projections
        .get("publish_outbox")
        .or_else(|| projections.get("publishOutbox"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let kind = row
                .get("kind")
                .and_then(Value::as_u64)
                .and_then(|k| u32::try_from(k).ok())
                .unwrap_or_default();
            let content = string_field(row, "content");
            let status = first_nonempty(row, &["status"]);
            // aim.md §2 #4: title/preview/status_label removed from wire.
            // JSON path computes from raw kind/content/status (mirrors typed path).
            OutboxLine {
                handle: string_field(row, "handle"),
                title: json_outbox_kind_title(kind),
                status_label: json_outbox_status_label(&status),
                preview: json_outbox_preview(kind, &content),
                can_retry: bool_field(row, "can_retry") || bool_field(row, "canRetry"),
                relays: relay_lines_from(row),
            }
        })
        .collect()
}

/// Parse `projections.publish_queue` into newest-first settled history.
pub(crate) fn publish_history_from(projections: &Value) -> Vec<PublishHistoryLine> {
    let Some(rows) = projections.get("publish_queue").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .rev() // kernel appends, so reverse gives newest-first
        .filter(|row| {
            // Only render terminally-settled rows. `accepted_locally` is the
            // in-flight status — those rows already show in the active outbox
            // pane; rendering them in history too would duplicate.
            let status = row
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            !status.is_empty() && status != "accepted_locally"
        })
        .take(20)
        .map(|row| {
            let relays = row
                .get("relay_outcomes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|r| HistoryRelayLine {
                    relay_url: string_field(r, "relay_url"),
                    status: string_field(r, "status"),
                    relay_reason: format_relay_reason_token(&string_field(r, "relay_reason")),
                    message: format_relay_message_token(&string_field(r, "message")),
                })
                .collect();
            let kind = row
                .get("kind")
                .and_then(Value::as_u64)
                .and_then(|k| u32::try_from(k).ok())
                .unwrap_or_default();
            PublishHistoryLine {
                event_id: string_field(row, "event_id"),
                kind,
                title: json_outbox_kind_title(kind),
                status: string_field(row, "status"),
                can_retry: bool_field(row, "can_retry") || bool_field(row, "canRetry"),
                relays,
            }
        })
        .collect()
}

pub(crate) fn relay_lines_from(row: &Value) -> Vec<OutboxRelayLine> {
    row.get("relays")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| {
            let status = first_nonempty(r, &["status"]);
            OutboxRelayLine {
                relay_url: string_field(r, "relay_url"),
                // aim.md §2 #4: status_label removed from wire. Compute from status.
                status_label: json_outbox_relay_status_label(&status),
                reason: format_relay_reason_token(&string_field(r, "relay_reason")),
                message: format_relay_message_token(&string_field(r, "message")),
            }
        })
        .collect()
}

pub(crate) fn configured_relays_from(projections: &Value) -> Vec<RelayEditLine> {
    projections
        .get("configured_relays")
        .or_else(|| projections.get("configuredRelays"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| RelayEditLine {
            url: string_field(row, "url"),
            role: string_field(row, "role"),
        })
        .collect()
}

pub(crate) fn wallet_from(wallet: Option<&Value>) -> WalletLine {
    let Some(wallet) = wallet else {
        return WalletLine::default();
    };
    WalletLine {
        status: string_field(wallet, "status"),
        relay_url: first_nonempty(wallet, &["relay_url", "relayUrl"]),
        wallet_npub: first_nonempty(wallet, &["wallet_npub", "walletNpub"]),
        balance_msats: wallet
            .get("balance_msats")
            .or_else(|| wallet.get("balanceMsats"))
            .and_then(Value::as_u64),
    }
}

pub(crate) fn dm_from(projections: &Value) -> Vec<DmConversationLine> {
    projection(projections, "nmp.nip17.dm_inbox")
        .and_then(|dm| dm.get("conversations"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let messages = messages_from(Some(row));
            let peer_pubkey = first_nonempty(row, &["peer_pubkey", "peerPubkey"]);
            // TUI is the presentation layer — backend ships raw hex
            // (aim.md §2). Abbreviate locally for the conversation row
            // header.
            let peer_display = if peer_pubkey.is_empty() {
                String::new()
            } else {
                nmp_core::display::short_npub(&peer_pubkey)
            };
            DmConversationLine {
                peer_pubkey,
                peer_display,
                latest: messages
                    .last()
                    .map(|m| m.content.clone())
                    .unwrap_or_default(),
                messages,
            }
        })
        .collect()
}

pub(crate) fn messages_from(value: Option<&Value>) -> Vec<MessageLine> {
    value
        .and_then(|v| v.get("messages"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| MessageLine {
            id: string_field(row, "id"),
            author: first_nonempty(row, &["sender_pubkey", "senderPubkey", "pubkey"]),
            content: string_field(row, "content"),
            outgoing: bool_field(row, "is_outgoing") || bool_field(row, "isOutgoing"),
        })
        .collect()
}

pub(crate) fn groups_from(projections: &Value) -> Vec<GroupLine> {
    projection(projections, "nmp.nip29.discovered_groups")
        .and_then(|groups| groups.get("groups"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| GroupLine {
            host_relay_url: first_nonempty(row, &["host_relay_url", "hostRelayUrl"]),
            group_id: first_nonempty(row, &["group_id", "groupId"]),
            name: optional_string(row, "name")
                .unwrap_or_else(|| first_nonempty(row, &["group_id", "groupId"])),
            about: string_field(row, "about"),
            member_count: number_field(row, "member_count") + number_field(row, "memberCount"),
            open: bool_field(row, "open"),
        })
        .collect()
}

pub(crate) fn follow_count_from(projections: &Value) -> usize {
    projection(projections, "nmp.follow_list")
        .and_then(|f| f.get("follows"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

// V-112 (ADR-0042): profile_from / thread_from deleted — the author_view /
// thread_view projections they decoded are removed from the kernel.

/// Parse `projections.outbox_summary` into a `SummaryLine`. aim.md §2 #4:
/// `title`/`subtitle` removed from wire — compute from raw per-status counters.
pub(crate) fn outbox_summary_from(value: Option<&Value>) -> SummaryLine {
    let Some(v) = value else {
        return SummaryLine::default();
    };
    let total = u32_field(v, "total");
    let sending = u32_field(v, "sending");
    let retrying = u32_field(v, "retrying");
    let queued = u32_field(v, "queued");
    let failed = u32_field(v, "failed");
    let title = if total == 0 {
        "Nothing waiting".to_string()
    } else {
        let suffix = if total == 1 { "" } else { "es" };
        format!("{total} pending publish{suffix}")
    };
    let subtitle = if total == 0 {
        "Your local outbox is clear.".to_string()
    } else if retrying > 0 {
        format!("{retrying} waiting to retry, {sending} currently sending.")
    } else if sending > 0 {
        format!("{sending} currently sending.")
    } else if failed > 0 {
        format!("{failed} failed.")
    } else {
        let _ = queued;
        "Waiting for relay connections.".to_string()
    };
    SummaryLine { title, subtitle }
}

fn u32_field(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or_default()
}

pub(crate) fn settings_hub_from(value: Option<&Value>) -> SummaryLine {
    let subtitle = value
        .and_then(|v| {
            v.get("relay_count")
                .or_else(|| v.get("relayCount"))
                .and_then(Value::as_u64)
        })
        .map(relay_count_subtitle)
        .unwrap_or_default();
    SummaryLine {
        title: "Settings".to_string(),
        subtitle,
    }
}

pub(crate) fn projection<'a>(projections: &'a Value, key: &str) -> Option<&'a Value> {
    projections
        .get(key)
        .or_else(|| projections.get(key.replace("_", "").as_str()))
}

pub(crate) fn first_nonempty(value: &Value, keys: &[&str]) -> String {
    first_nonempty_option(value, keys).unwrap_or_default()
}

fn first_nonempty_option(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| optional_string(value, key))
}

pub(crate) fn string_field(value: &Value, key: &str) -> String {
    optional_string(value, key).unwrap_or_default()
}

pub(crate) fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(crate) fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn number_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

// ── Publish-outbox JSON-path presentation helpers ───────────────────────────
//
// aim.md §2 #4: title/preview/status_label removed from the nmp-core wire.
// The JSON path computes them from raw kind/content/status, mirroring the
// typed path (`feature_snapshot_typed`) and the iOS/Android shells.

fn json_outbox_kind_title(kind: u32) -> String {
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

fn json_outbox_status_label(status: &str) -> String {
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

fn json_outbox_relay_status_label(status: &str) -> String {
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

/// Format a raw relay-reason token (e.g. `"nip65_write"`) into a display string.
/// Parameterised tokens are parsed; unknown tokens pass through verbatim.
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

/// Format a raw relay-message token (e.g. `"waiting_for_ok"`) into a display
/// string. Raw relay protocol error text passes through verbatim.
fn format_relay_message_token(token: &str) -> String {
    match token {
        "waiting_for_connection" => "Waiting for relay connection".to_string(),
        "waiting_for_ok" => "Waiting for relay OK".to_string(),
        "accepted" => "Relay accepted the event".to_string(),
        "timed_out" => "No response from relay".to_string(),
        other => other.to_string(),
    }
}

fn json_outbox_preview(kind: u32, content: &str) -> String {
    // Encrypted-content classification is a Nostr protocol rule, owned by the
    // canonical predicate rather than re-derived in the shell (#1769).
    if nmp_kinds::is_encrypted_content_kind(kind) {
        return "Encrypted event content hidden".to_string();
    }
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "Event with no text content".to_string();
    }
    let mut preview: String = trimmed.chars().take(180).collect();
    if trimmed.chars().count() > 180 {
        preview.push('…');
    }
    preview
}

#[cfg(test)]
mod json_outbox_preview_tests {
    use super::json_outbox_preview;

    #[test]
    fn encrypted_kinds_hide_content_plaintext_shows_through() {
        // Same canonical-predicate wiring as the typed path (#1769): ciphertext
        // kinds hide their content...
        assert_eq!(
            json_outbox_preview(1059, "ciphertext"),
            "Encrypted event content hidden",
            "kind:1059 gift-wrap content must be hidden"
        );
        // ...plaintext kinds render verbatim (load-bearing negative: a blanket
        // hide or a mis-classified kind:1 fails here).
        assert_eq!(
            json_outbox_preview(1, "hello world"),
            "hello world",
            "kind:1 short text note content must render verbatim"
        );
        assert_eq!(
            json_outbox_preview(14, "decrypted dm body"),
            "decrypted dm body",
            "kind:14 rumor content is plaintext and must render"
        );
    }
}
