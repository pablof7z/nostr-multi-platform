//! `InboxStore` — the shared, epoch-guarded decrypt-result store for the DM
//! inbox, plus the small pure rumor/relay helpers the port chain uses.
//!
//! Extracted from `inbox.rs` to keep that file under the 500-LOC ceiling. The
//! store is held behind an `Arc` so each in-flight gift-UNWRAP port chain
//! (`super::chain`) carries a clone into its terminal continuation and inserts
//! the decrypted message even though the chain outlives the synchronous
//! `ingest_gift_wrap` call (ADR-0050 §D6). The `generation` counter is the §D6
//! epoch guard against cross-account leaks.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, MAX_PROJECTION_MESSAGES};

use super::{DmConversation, DmMessage};

/// Shared, decrypt-result store for the DM inbox.
pub(crate) struct InboxStore {
    /// Accepted decrypted messages keyed by inner-rumor event id. The value
    /// pairs the conversation peer with the message. Idempotent — a
    /// re-delivered envelope replaces rather than duplicates. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] so a long-running inbox cannot grow
    /// unboundedly across a session; once full, the oldest-by-insertion
    /// rumor is evicted, keeping per-tick snapshot serialisation O(cap).
    messages: Mutex<BoundedMessageMap<String, (String, DmMessage)>>,
    /// Account-switch epoch (§D6). Bumped by [`super::DmInboxProjection::clear`];
    /// each chain captures the value live at launch and a terminal continuation
    /// discards its plaintext if the counter has since advanced (the active
    /// account changed mid-flight) — so a previous account's message can never
    /// leak into the new account's snapshot (#1138, async-chain-safe).
    generation: AtomicU64,
}

impl InboxStore {
    pub(crate) fn new() -> Self {
        Self {
            messages: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
            generation: AtomicU64::new(0),
        }
    }

    /// Current epoch — captured by a chain at launch (§D6 account pinning).
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Insert one decrypted message under epoch `gen`. A no-op (returns `false`)
    /// when `gen` is stale (the account switched mid-flight, §D6) or the mutex
    /// is poisoned (D6). When the id already exists, merges source-relay
    /// provenance instead of duplicating (idempotent re-delivery).
    pub(crate) fn insert(
        &self,
        gen: u64,
        message_id: String,
        peer_pubkey: String,
        message: DmMessage,
        source_relay_url: Option<&str>,
    ) -> bool {
        // §D6 epoch guard — discard a completion for a superseded account.
        if gen != self.generation.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut messages) = self.messages.lock() else {
            return false;
        };
        if let Some((_peer, existing)) = messages.get_mut(&message_id) {
            merge_source_relay(&mut existing.source_relays, source_relay_url);
            return true;
        }
        messages.insert(message_id, (peer_pubkey, message));
        true
    }

    /// Snapshot the current messages grouped per peer (see
    /// [`super::DmInboxProjection::snapshot`] for ordering semantics).
    pub(crate) fn snapshot_conversations(&self) -> Vec<DmConversation> {
        let Ok(messages) = self.messages.lock() else {
            return Vec::new();
        };
        let mut by_peer: BTreeMap<String, Vec<DmMessage>> = BTreeMap::new();
        for (peer, msg) in messages.values() {
            by_peer.entry(peer.clone()).or_default().push(msg.clone());
        }
        let mut conversations: Vec<DmConversation> = by_peer
            .into_iter()
            .map(|(peer_pubkey, mut msgs)| {
                // Chronological within the thread — oldest first, newest last;
                // tie-break on id ascending so the order is total even when two
                // messages share a `created_at`.
                msgs.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                DmConversation {
                    peer_pubkey,
                    messages: msgs,
                }
            })
            .collect();
        // Newest conversation first — keyed on the thread's most-recent message;
        // tie-break on peer pubkey descending for a total, stable order.
        conversations.sort_by(|a, b| {
            let a_latest = a.messages.last().map_or(0, |m| m.created_at);
            let b_latest = b.messages.last().map_or(0, |m| m.created_at);
            b_latest
                .cmp(&a_latest)
                .then_with(|| b.peer_pubkey.cmp(&a.peer_pubkey))
        });
        conversations
    }

    /// Drop all messages and bump the epoch so any chain in flight under the
    /// previous epoch discards its terminal insert (§D6).
    pub(crate) fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut messages) = self.messages.lock() {
            *messages = BoundedMessageMap::new(MAX_PROJECTION_MESSAGES);
        }
    }
}

/// First `["p", <pubkey>]` tag value on a rumor, if any.
pub(crate) fn first_p_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
    rumor.tags.iter().find_map(|tag| {
        let slice = tag.as_slice();
        match slice {
            [name, value, ..] if name == "p" => Some(value.clone()),
            _ => None,
        }
    })
}

/// First NIP-10 reply marker — `["e", <event-id>, <relay-hint>, "reply"]` —
/// on a rumor, returning the referenced event id.
pub(crate) fn first_reply_e_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
    rumor.tags.iter().find_map(|tag| {
        let slice = tag.as_slice();
        match slice {
            [name, value, _hint, marker, ..] if name == "e" && marker == "reply" => {
                Some(value.clone())
            }
            _ => None,
        }
    })
}

pub(crate) fn source_relays_from(source_relay_url: Option<&str>) -> Vec<String> {
    let mut relays = Vec::new();
    merge_source_relay(&mut relays, source_relay_url);
    relays
}

pub(crate) fn merge_source_relay(relays: &mut Vec<String>, source_relay_url: Option<&str>) {
    let Some(source) = source_relay_url.filter(|source| !source.is_empty()) else {
        return;
    };
    if !relays.iter().any(|existing| existing == source) {
        relays.push(source.to_string());
    }
}
