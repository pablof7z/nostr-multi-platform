//! `DmInboxProjection` — the receive side of NIP-17 private direct messages.
//!
//! # Overview
//!
//! This is the *inbound* counterpart to [`crate::build_dm_rumor`]. It is a
//! [`RawEventObserver`](nmp_core::RawEventObserver) — the kernel's verbatim
//! signed-event tap — registered with a kind:1059 filter. For every accepted
//! gift-wrap envelope it:
//!
//! 1. Parses the verbatim wire JSON into a signed `nostr::Event` (the `sig`
//!    is mandatory — NIP-44 decryption verifies the seal).
//! 2. Unwraps the gift-wrap with the active account's local `nostr::Keys`
//!    (`nmp_nip59::unwrap_gift_wrap`), yielding the sender pubkey and the
//!    inner kind:14 rumor.
//! 3. Accepts only kind:14 rumors, keys them by event id for idempotency,
//!    and groups them per conversation peer.
//!
//! The accumulated state is exposed through [`DmInboxProjection::snapshot_json`]
//! — the exact shape a host `register_snapshot_projection` closure returns —
//! so the inbox surfaces on every kernel snapshot tick.
//!
//! # Why a `RawEventObserver`, not a `KernelEventObserver`
//!
//! The kernel's `KernelEventObserver` delivers a sig-stripped, projection-
//! stable `KernelEvent`. NIP-44 decryption needs the *whole* signed event
//! verbatim (`sig` included), so the inbox plugs into the parallel raw tap —
//! the same seam other kind:1059 consumers use for the raw event tap.
//!
//! # Key seam (ADR-0026 boundary)
//!
//! The projection holds an `Arc<Mutex<Option<nostr::Keys>>>` — the
//! substrate-generic active-local-keys slot the actor writes on every
//! identity mutation, exposed by the FFI shell as
//! `NmpApp::active_local_keys`. When the slot is `None` the user is not
//! signed in (or holds a remote-signer account); every incoming envelope
//! is then a silent no-op. Bunker (NIP-46) decrypt support is gated on
//! ADR-0026 Phase 2 — a remote signer cannot currently unseal a gift-wrap
//! because `unwrap_gift_wrap` needs raw `Keys`.
//!
//! This is the NIP-17 key seam and is DELIBERATELY distinct from any
//! other crate's key slots — each consumer owns its own slot.
//!
//! # D-doctrine
//!
//! * **D3 / D8** — `on_raw_event` runs synchronously on the actor thread
//!   between relay frames. The work is bounded per event (one parse, one
//!   in-process NIP-44 unseal, one map insert); no background tasks, no I/O,
//!   no polling.
//! * **D6** — every failure path is a silent no-op: a poisoned mutex, a
//!   malformed envelope, an envelope addressed to someone else, a non-kind:14
//!   rumor. Nothing panics across the actor boundary.
//! * **D7** — an incoming rumor's `created_at` was stamped by the *sender*;
//!   it is the real send time, not the `0` "kernel please stamp me" sentinel
//!   the outbound builder uses. It is stored verbatim. Presentation layers
//!   format the timestamp for display (aim.md §2 — NMP sends raw Unix
//!   seconds, shells own date formatting).
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/17.md>

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting,
};
use nmp_core::substrate::{BoundedMessageMap, ViewDependencies, MAX_PROJECTION_MESSAGES};
use nmp_core::{KindFilter, RawEventObserver};
use nmp_nip59::KIND_GIFT_WRAP;
use nostr::{Event, JsonUtil};
use serde::{Deserialize, Serialize};

/// NIP-17 kind:14 chat-message rumor — the only inner kind this inbox keeps.
const KIND_CHAT_MESSAGE: u16 = 14;

/// One decrypted NIP-17 direct message, ready for a chat row.
///
/// A flat carrier — threading is represented only by `reply_to`; nested
/// rendering is a host concern. Fields are the minimum a shell needs to draw
/// one message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmMessage {
    /// Inner kind:14 rumor event id (hex). Also the dedupe key in the inbox.
    pub id: String,
    /// Pubkey (hex) of whoever wrote the message — taken from the verified
    /// kind:13 seal, NOT from any tag (a tag could be forged; the seal is
    /// NIP-44-authenticated).
    pub sender_pubkey: String,
    /// Plaintext kind:14 `content`, verbatim.
    pub content: String,
    /// Unix seconds — the rumor's own `created_at`, stamped by the sender
    /// (D7: a received message's timestamp is real, not the `0` sentinel).
    /// Presentation layer formats this for display (aim.md §2: NMP is a
    /// data framework; backend sends raw timestamps, shells own
    /// formatting).
    pub created_at: u64,
    /// When the rumor carries a NIP-10 `["e", <id>, _, "reply"]` marker, the
    /// id of the message this one replies to.
    pub reply_to: Option<String>,
    /// `true` when the local account wrote this message — `sender_pubkey`
    /// equals the active account's pubkey. Pre-classified in Rust so the
    /// host shell never compares pubkeys to decide bubble alignment
    /// (thin-shell rule: that comparison is a protocol decision — the
    /// kind:13 seal authenticated this pubkey, and the host should not
    /// re-do that work).
    pub is_outgoing: bool,
    /// Relay URLs that delivered the gift-wrap envelope for this message.
    /// Populated from the kernel raw observer source provenance and kept
    /// deduplicated in first-seen order.
    #[serde(default)]
    pub source_relays: Vec<String>,
}

/// One DM thread — every message exchanged with a single peer.
///
/// Carries only the raw protocol identifier for the peer. Presentation layers
/// own all formatting: bech32 encoding (`npub1…`), abbreviation, avatar
/// initials, avatar tint colour, and any join against a profile cache for
/// the peer's display name / picture — see aim.md §2 (NMP is a data
/// framework; projection and snapshot code sends raw data only).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmConversation {
    /// The OTHER party in the thread (hex pubkey, 64 chars) — never the
    /// local user.
    pub peer_pubkey: String,
    /// Messages in this thread, ordered chronologically — **oldest first,
    /// newest last**. This is the natural render order of a chat log so the
    /// host shell never re-sorts or reverses (thin-shell rule). The
    /// thread-level "most recent message" used by the inbox sort is
    /// `messages.last()`.
    pub messages: Vec<DmMessage>,
}

/// The serialised read-model a DM screen consumes: every conversation the
/// local account participates in.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmInboxSnapshot {
    /// Conversations, ordered by most-recent message (newest thread first).
    pub conversations: Vec<DmConversation>,
    /// Set to `true` when the active account uses a remote signer (NIP-46
    /// bunker) that cannot unseal gift-wraps — the inbox will always be empty
    /// in this case, and the host should surface a "DM inbox unavailable for
    /// bunker accounts" message instead of an empty list.
    ///
    /// `false` when signed in with local keys (normal) or when not signed in
    /// (the host should hide the DM screen entirely in that case). Additive
    /// field: decoders that pre-date this field read `false` via
    /// `#[serde(default)]`.
    #[serde(default)]
    pub remote_signer_unsupported: bool,
}

impl DmInboxSnapshot {
    /// An empty inbox — what a fresh projection (or a poisoned mutex, D6)
    /// reports.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            conversations: Vec::new(),
            remote_signer_unsupported: false,
        }
    }
}

/// Accumulates decrypted NIP-17 direct messages into a per-peer conversation
/// model.
///
/// Construct with the shared active-local-keys slot
/// (`NmpApp::active_local_keys`), register the same `Arc` as a
/// [`RawEventObserver`] with [`Self::kind_filter`], and capture it in a
/// snapshot-projection closure (`snapshot_json`).
pub struct DmInboxProjection {
    /// Shared local-keys slot. The actor writes the active account's
    /// `nostr::Keys` here on every identity mutation; the projection reads it
    /// to unseal each incoming gift-wrap. `None` → not signed in / remote
    /// signer → every envelope is a silent no-op.
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    /// Accepted decrypted messages keyed by inner-rumor event id. The value
    /// pairs the conversation peer with the message. Idempotent — a
    /// re-delivered envelope replaces rather than duplicates. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] so a long-running inbox cannot grow
    /// unboundedly across a session; once full, the oldest-by-insertion
    /// rumor is evicted, keeping per-tick snapshot serialisation O(cap).
    messages: Mutex<BoundedMessageMap<String, (String, DmMessage)>>,
}

impl DmInboxProjection {
    /// Construct an inbox bound to the shared local-keys slot. The message
    /// store starts empty; envelopes arrive via [`RawEventObserver::on_raw_event`].
    #[must_use]
    pub fn new(local_keys: Arc<Mutex<Option<nostr::Keys>>>) -> Self {
        Self {
            local_keys,
            messages: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The kind filter to register this observer with — kind:1059 only.
    #[must_use]
    pub fn kind_filter() -> KindFilter {
        KindFilter::from_kinds([KIND_GIFT_WRAP])
    }

    /// Snapshot the current inbox as a typed [`DmInboxSnapshot`].
    ///
    /// Messages are grouped per peer, each conversation ordered
    /// chronologically (oldest first, newest last — the natural render order
    /// of a chat log), and conversations ordered by their most-recent message
    /// (newest thread first). Ties break on a stable secondary key so the
    /// order is total and deterministic across snapshot ticks.
    ///
    /// D6: a poisoned mutex degrades to [`DmInboxSnapshot::empty`] rather than
    /// panicking — this runs on the actor thread inside a snapshot tick.
    ///
    /// When `local_keys` is `None` (bunker / not yet signed in), sets
    /// `DmInboxSnapshot::remote_signer_unsupported` so the host can surface
    /// a meaningful message instead of a misleading empty list (V-08 Stage 1).
    /// ADR-0026 Phase 2 (Stage 3) removes the flag by wiring gift-wrap
    /// unsealing through the remote signer RPC.
    #[must_use]
    pub fn snapshot(&self) -> DmInboxSnapshot {
        let Ok(messages) = self.messages.lock() else {
            return DmInboxSnapshot::empty();
        };

        // V-08 Stage 1: detect whether decryption is impossible because the
        // local-keys slot is absent (bunker / not signed in). A host can use
        // this flag to show "DM inbox unavailable for bunker accounts" instead
        // of a misleading empty-list UX. When the slot lock is poisoned we
        // fall through to an empty-conversations snapshot (D6 degradation) and
        // leave the flag false — a poisoned mutex is a process-internal error,
        // not a user-visible signer constraint.
        let remote_signer_unsupported = self
            .local_keys
            .lock()
            .map(|guard| guard.is_none())
            .unwrap_or(false);

        // Group messages by conversation peer. Each message is cloned out of
        // the bounded store; messages carry only raw protocol data
        // (aim.md §2 — presentation layer formats timestamps and pubkeys).
        let mut by_peer: BTreeMap<String, Vec<DmMessage>> = BTreeMap::new();
        for (peer, msg) in messages.values() {
            by_peer.entry(peer.clone()).or_default().push(msg.clone());
        }

        let mut conversations: Vec<DmConversation> = by_peer
            .into_iter()
            .map(|(peer_pubkey, mut msgs)| {
                // Chronological within the thread — oldest first, newest
                // last. This is the natural render order of a chat log, so
                // the host shell never reverses. Tie-break on id ascending
                // so the order is total even when two messages share a
                // `created_at`.
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

        // Newest conversation first — keyed on the thread's most-recent
        // message (the last entry after the chronological sort above).
        // Tie-break on peer pubkey descending for a total, stable order.
        conversations.sort_by(|a, b| {
            let a_latest = a.messages.last().map_or(0, |m| m.created_at);
            let b_latest = b.messages.last().map_or(0, |m| m.created_at);
            b_latest
                .cmp(&a_latest)
                .then_with(|| b.peer_pubkey.cmp(&a.peer_pubkey))
        });

        DmInboxSnapshot { conversations, remote_signer_unsupported }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `{"conversations": []}` rather than propagating.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "conversations": [], "remote_signer_unsupported": false }))
    }

    /// Decrypt and store one accepted kind:1059 envelope. Returns `true` when
    /// a kind:14 rumor was extracted and retained; `false` for every silent
    /// no-op path (not signed in, addressed to someone else, not a kind:14,
    /// poisoned mutex). Factored out of [`RawEventObserver::on_raw_event`] so
    /// the unit tests can assert the outcome.
    fn ingest_gift_wrap(&self, json: &str, source_relay_url: Option<&str>) -> bool {
        // Parse the verbatim signed event off the borrowed buffer. A
        // malformed envelope is a silent no-op (D6).
        let Ok(event) = Event::from_json(json) else {
            return false;
        };

        // Resolve the active account's keys. `None` (not signed in / remote
        // signer) or a poisoned slot → silent no-op (D6).
        let keys: nostr::Keys = {
            let Ok(guard) = self.local_keys.lock() else {
                return false;
            };
            let Some(keys) = guard.as_ref() else {
                return false;
            };
            keys.clone()
        };
        let local_pubkey = keys.public_key().to_hex();

        // Unseal the gift-wrap. An `Err` means the envelope was not addressed
        // to us (or is another protocol's kind:1059 traffic) — a silent no-op,
        // never a panic (D6). `gift` is `mut` so
        // the canonical rumor id can be computed below if absent.
        let Ok(mut gift) = nmp_nip59::unwrap_gift_wrap(&keys, &event) else {
            return false;
        };

        // Only kind:14 chat-message rumors belong in the DM inbox. Rumors
        // of any other kind that happen to unwrap are discarded here.
        if gift.rumor.kind.as_u16() != KIND_CHAT_MESSAGE {
            return false;
        }

        let sender_pubkey = gift.sender.to_hex();
        // The rumor's id may be `None` if the sender did not pre-compute it;
        // `UnsignedEvent::id()` derives the canonical NIP-01 id deterministically
        // (and memoises it). Compute it up front so the inbox always has a
        // stable dedupe key.
        let message_id = gift.rumor.id().to_hex();
        let rumor = &gift.rumor;

        // The conversation peer is the OTHER party. The self-copy envelope
        // (the sender gift-wraps to their own pubkey so sent messages stay
        // readable) carries `sender == local`; for it the peer is the `p`-tag
        // recipient. The recipient's own copy carries `sender == them`; for it
        // the peer is the sender.
        let peer_pubkey = if sender_pubkey == local_pubkey {
            match first_p_tag(rumor) {
                Some(p) => p,
                // A self-copy with no `p` tag is malformed — discard (D6).
                None => return false,
            }
        } else {
            sender_pubkey.clone()
        };

        // Pre-classify outgoing vs incoming so the host shell never compares
        // pubkeys to align a bubble. The kind:13 seal authenticated
        // `sender_pubkey`; replaying that comparison in the shell would be
        // protocol logic leaking out (thin-shell rule).
        let is_outgoing = sender_pubkey == local_pubkey;
        let message = DmMessage {
            id: message_id.clone(),
            sender_pubkey,
            content: rumor.content.clone(),
            // D7: the rumor's `created_at` is the sender's real send time.
            created_at: rumor.created_at.as_secs(),
            reply_to: first_reply_e_tag(rumor),
            is_outgoing,
            source_relays: source_relays_from(source_relay_url),
        };

        // Idempotent insert — a re-delivered envelope updates source
        // provenance rather than duplicating the message. Poisoned mutex →
        // silent no-op (D6).
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
}

impl RawEventObserver for DmInboxProjection {
    /// One accepted inbound signed event (verbatim flat NIP-01 JSON, `sig`
    /// included). The kind filter guarantees `kind == 1059`; `ingest_gift_wrap`
    /// does the unseal + store. Every failure is a silent no-op (D6); the
    /// projection mutation is the load-bearing effect a later snapshot tick
    /// surfaces.
    fn on_raw_event(&self, _kind: u32, json: &str) {
        let _ = self.ingest_gift_wrap(json, None);
    }

    fn on_raw_event_with_source(&self, _kind: u32, json: &str, source_relay_url: Option<&str>) {
        let _ = self.ingest_gift_wrap(json, source_relay_url);
    }
}

/// Stable id for the active-account-owned gift-wrap inbox interest.
///
/// The id is intentionally independent of the pubkey so an account switch
/// replaces the prior `#p` filter instead of accumulating one long-lived
/// subscription per account.
#[must_use]
pub fn active_giftwrap_inbox_interest_id() -> InterestId {
    InterestId(nmp_core::stable_hash::stable_hash64(
        "nip17.giftwrap.active",
    ))
}

/// Tailing [`LogicalInterest`] for kind:1059 `#p <pubkey>` gift-wraps — the
/// subscription a host pushes (via `NmpApp::push_interest`) so the DM inbox
/// actually receives envelopes.
///
/// The filter targets a concrete `#p <pubkey>` because NIP-17 gift-wraps are
/// addressed to a real account. The stable id + [`InterestScope::ActiveAccount`]
/// scope makes the registration lifecycle single-slot: re-pushing for a new
/// active account replaces the old filter, and logout withdraws one known id.
/// The kernel routes the `#p` filter to the account's kind:10050 DM relays via
/// [`PTagRouting::Nip17DmRelays`]; if the kind:10050 list is unknown or empty,
/// the compiler emits no subscription instead of falling back to public NIP-65
/// read relays.
#[must_use]
pub fn active_giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        active_giftwrap_inbox_interest_id(),
        InterestScope::ActiveAccount,
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip17DmRelays;
    interest
}

/// First `["p", <pubkey>]` tag value on a rumor, if any.
fn first_p_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
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
fn first_reply_e_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
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

fn source_relays_from(source_relay_url: Option<&str>) -> Vec<String> {
    let mut relays = Vec::new();
    merge_source_relay(&mut relays, source_relay_url);
    relays
}

fn merge_source_relay(relays: &mut Vec<String>, source_relay_url: Option<&str>) {
    let Some(source) = source_relay_url.filter(|source| !source.is_empty()) else {
        return;
    };
    if !relays.iter().any(|existing| existing == source) {
        relays.push(source.to_string());
    }
}

#[cfg(test)]
#[path = "inbox/tests.rs"]
mod tests;
