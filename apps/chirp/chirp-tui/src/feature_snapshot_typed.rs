//! PR-B (#991/#979) typed-first decode for [`FeatureSnapshot`].
//!
//! Split out of `feature_snapshot.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). This module owns the FlatBuffers typed-sidecar path:
//! it reads kernel built-in projections via `nmp_core::typed_projections::*`
//! and host-registered projections via the protocol crates' public decoders.
//! The generic `payload:Value` tree is never read.

use crate::feature_snapshot::{
    AccountLine, DmConversationLine, FeatureSnapshot, GroupLine, HistoryRelayLine, MessageLine,
    OutboxLine, OutboxRelayLine, ProfileLine, PublishHistoryLine, RelayEditLine, SummaryLine,
    ThreadLine, WalletLine,
};

pub(crate) fn feature_snapshot_from_flatbuffer(bytes: &[u8]) -> FeatureSnapshot {
    let typed = nmp_core::decode_snapshot_typed_projections(bytes)
        .unwrap_or_default();

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
                    signer: if !row.signer_label.is_empty() {
                        row.signer_label
                    } else {
                        row.signer_kind
                    },
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
    let configured_relays =
        find(nmp_core::typed_projections::CONFIGURED_RELAYS_SCHEMA_ID)
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
        .map(|m| {
            SummaryLine {
                title: "Settings".to_string(),
                subtitle: crate::feature_snapshot::relay_count_subtitle(m.relay_count as u64),
            }
        })
        .unwrap_or_else(|| SummaryLine {
            title: "Settings".to_string(),
            subtitle: String::new(),
        });

    // publish_outbox (key == schema_id == "publish_outbox")
    let outbox = find(nmp_core::typed_projections::PUBLISH_OUTBOX_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_publish_outbox(b).ok())
        .map(|m| {
            m.items
                .into_iter()
                .map(|row| OutboxLine {
                    handle: row.handle,
                    title: row.title,
                    status_label: row.status_label,
                    preview: row.preview,
                    can_retry: row.can_retry,
                    relays: row
                        .relays
                        .into_iter()
                        .map(|r| OutboxRelayLine {
                            relay_url: r.relay_url,
                            status_label: r.status_label,
                            reason: r.relay_reason,
                            message: r.message,
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    // outbox_summary (key == schema_id == "outbox_summary")
    let outbox_summary = find(nmp_core::typed_projections::OUTBOX_SUMMARY_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_outbox_summary(b).ok())
        .map(|m| SummaryLine {
            title: m.title,
            subtitle: m.subtitle,
        })
        .unwrap_or_default();

    // publish_queue (key == schema_id == "publish_queue")
    let history = find(nmp_core::typed_projections::PUBLISH_QUEUE_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_publish_queue(b).ok())
        .map(publish_history_from_queue)
        .unwrap_or_default();

    // author_view (key == schema_id == "author_view"; absent when view closed)
    let author_profile = find(nmp_core::typed_projections::AUTHOR_VIEW_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_author_view(b).ok())
        .map(|m| {
            let display = m
                .profile
                .display_name
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if !m.pubkey.is_empty() {
                        nmp_core::display::short_npub(&m.pubkey)
                    } else {
                        String::new()
                    }
                });
            ProfileLine {
                pubkey: m.pubkey,
                display,
                about: m.profile.about,
                note_count: m.note_count_display,
                action_label: m
                    .primary_action
                    .as_ref()
                    .map(|a| a.label.clone())
                    .unwrap_or_default(),
            }
        });

    // thread_view (key == schema_id == "thread_view"; absent when view closed)
    let thread = find(nmp_core::typed_projections::THREAD_VIEW_SCHEMA_ID)
        .and_then(|b| nmp_core::typed_projections::decode_thread_view(b).ok())
        .map(|m| ThreadLine {
            focused_event_id: m.focused_event_id,
            state: m.state,
            previous_label: m.previous_count_label,
            next_label: m.next_count_label,
            item_count: m.items.len(),
        });

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

    // Host-registered: nmp.nip29.group_chat (key == "nmp.nip29.group_chat")
    let group_messages = find("nmp.nip29.group_chat")
        .and_then(|b| nmp_nip29::decode_group_chat_snapshot(b).ok())
        .map(|m| {
            m.messages
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
        author_profile,
        thread,
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
                    relay_reason: r.relay_reason,
                    message: r.message,
                })
                .collect();
            PublishHistoryLine {
                event_id: row.event_id,
                kind: row.kind,
                title: row.title,
                status: row.status,
                can_retry: row.can_retry,
                relays,
            }
        })
        .collect()
}

