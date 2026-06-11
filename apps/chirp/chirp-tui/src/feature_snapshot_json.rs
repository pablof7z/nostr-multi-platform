//! JSON-fixture decode helpers for [`FeatureSnapshot`].
//!
//! Split out of `feature_snapshot.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). These parse the generic `projections` JSON tree and
//! are used ONLY by `FeatureSnapshot::from_projections` (the test/dev fixture
//! path — ADR-0037). The live FlatBuffers path lives in `feature_snapshot_typed`.

use serde_json::Value;

use crate::feature_snapshot::{
    relay_count_subtitle, AccountLine, DmConversationLine, GroupLine, HistoryRelayLine, MessageLine,
    OutboxLine, OutboxRelayLine, ProfileLine, PublishHistoryLine, RelayEditLine, SummaryLine,
    ThreadLine, WalletLine,
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
            signer: first_nonempty(row, &["signer_label", "signerLabel", "signer_kind"]),
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
        .map(|row| OutboxLine {
            handle: string_field(row, "handle"),
            title: string_field(row, "title"),
            status_label: first_nonempty(row, &["status_label", "statusLabel", "status"]),
            preview: string_field(row, "preview"),
            can_retry: bool_field(row, "can_retry") || bool_field(row, "canRetry"),
            relays: relay_lines_from(row),
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
                    relay_reason: string_field(r, "relay_reason"),
                    message: string_field(r, "message"),
                })
                .collect();
            PublishHistoryLine {
                event_id: string_field(row, "event_id"),
                kind: row
                    .get("kind")
                    .and_then(Value::as_u64)
                    .and_then(|k| u32::try_from(k).ok())
                    .unwrap_or_default(),
                // Pre-formatted by the kernel (`PublishQueueEntry.title`) —
                // the TUI no longer owns a kind→label mapping (RMP bible
                // commandment #4: backend owns display strings).
                title: string_field(row, "title"),
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
        .map(|r| OutboxRelayLine {
            relay_url: string_field(r, "relay_url"),
            status_label: first_nonempty(r, &["status_label", "statusLabel"]),
            reason: string_field(r, "relay_reason"),
            message: string_field(r, "message"),
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

pub(crate) fn profile_from(value: Option<&Value>) -> Option<ProfileLine> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let profile = value.get("profile").unwrap_or(value);
    let pubkey = {
        let outer = first_nonempty(value, &["pubkey"]);
        if outer.is_empty() {
            string_field(profile, "pubkey")
        } else {
            outer
        }
    };
    // aim.md §2: ProfileCard ships `display_name: Option<String>` (None
    // when no kind:0); the TUI is the presentation layer, so when
    // display_name is null we fall back to abbreviating the raw hex
    // pubkey ourselves.
    let display = first_nonempty(profile, &["display_name", "displayName"]);
    let display = if display.is_empty() && !pubkey.is_empty() {
        nmp_core::display::short_npub(&pubkey)
    } else {
        display
    };
    Some(ProfileLine {
        pubkey,
        display,
        about: string_field(profile, "about"),
        note_count: first_nonempty(value, &["note_count_display", "noteCountDisplay"]),
        action_label: value
            .get("primary_action")
            .or_else(|| value.get("primaryAction"))
            .map(|a| string_field(a, "label"))
            .unwrap_or_default(),
    })
}

pub(crate) fn thread_from(value: Option<&Value>) -> Option<ThreadLine> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    Some(ThreadLine {
        focused_event_id: first_nonempty(value, &["focused_event_id", "focusedEventId"]),
        state: string_field(value, "state"),
        previous_label: first_nonempty(value, &["previous_count_label", "previousCountLabel"]),
        next_label: first_nonempty(value, &["next_count_label", "nextCountLabel"]),
        item_count: value
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    })
}

pub(crate) fn summary_from(value: Option<&Value>) -> SummaryLine {
    value.map_or_else(SummaryLine::default, |v| SummaryLine {
        title: string_field(v, "title"),
        subtitle: string_field(v, "subtitle"),
    })
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
    keys.iter()
        .find_map(|key| optional_string(value, key))
        .unwrap_or_default()
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
