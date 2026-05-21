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
//! The projection holds an `Arc<Mutex<Option<nostr::Keys>>>` — the slot the
//! actor writes on every identity mutation (`NmpApp::nip17_local_keys`). When
//! the slot is `None` the user is not signed in (or holds a remote-signer
//! account); every incoming envelope is then a silent no-op. Bunker (NIP-46)
//! decrypt support is gated on ADR-0026 Phase 2 — a remote signer cannot
//! currently unseal a gift-wrap because `unwrap_gift_wrap` needs raw `Keys`.
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
//!   the outbound builder uses. It is stored verbatim.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/17.md>

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
use nmp_core::substrate::ViewDependencies;
use nmp_core::{KindFilter, RawEventObserver};
use nostr::{Event, JsonUtil};
use serde::{Deserialize, Serialize};

/// NIP-59 gift-wrap kind — the opaque outer envelope this projection taps.
const KIND_GIFT_WRAP: u32 = 1059;

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
    pub created_at: u64,
    /// When the rumor carries a NIP-10 `["e", <id>, _, "reply"]` marker, the
    /// id of the message this one replies to.
    pub reply_to: Option<String>,
}

/// One DM thread — every message exchanged with a single peer.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmConversation {
    /// The OTHER party in the thread (hex pubkey) — never the local user.
    pub peer_pubkey: String,
    /// Messages in this thread, ordered newest-first.
    pub messages: Vec<DmMessage>,
}

/// The serialised read-model a DM screen consumes: every conversation the
/// local account participates in.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmInboxSnapshot {
    /// Conversations, ordered by most-recent message (newest thread first).
    pub conversations: Vec<DmConversation>,
}

impl DmInboxSnapshot {
    /// An empty inbox — what a fresh projection (or a poisoned mutex, D6)
    /// reports.
    pub fn empty() -> Self {
        Self {
            conversations: Vec::new(),
        }
    }
}

/// Accumulates decrypted NIP-17 direct messages into a per-peer conversation
/// model.
///
/// Construct with the shared local-keys slot (`NmpApp::nip17_local_keys`),
/// register the same `Arc` as a [`RawEventObserver`] with [`Self::kind_filter`],
/// and capture it in a snapshot-projection closure (`snapshot_json`).
pub struct DmInboxProjection {
    /// Shared local-keys slot. The actor writes the active account's
    /// `nostr::Keys` here on every identity mutation; the projection reads it
    /// to unseal each incoming gift-wrap. `None` → not signed in / remote
    /// signer → every envelope is a silent no-op.
    local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    /// Accepted decrypted messages keyed by inner-rumor event id. The value
    /// pairs the conversation peer with the message. A `BTreeMap` keyed on id
    /// makes ingest idempotent — a re-delivered envelope replaces rather than
    /// duplicates.
    messages: Mutex<BTreeMap<String, (String, DmMessage)>>,
}

impl DmInboxProjection {
    /// Construct an inbox bound to the shared local-keys slot. The message
    /// store starts empty; envelopes arrive via [`RawEventObserver::on_raw_event`].
    pub fn new(local_keys: Arc<Mutex<Option<nostr::Keys>>>) -> Self {
        Self {
            local_keys,
            messages: Mutex::new(BTreeMap::new()),
        }
    }

    /// The kind filter to register this observer with — kind:1059 only.
    pub fn kind_filter() -> KindFilter {
        KindFilter::from_kinds([KIND_GIFT_WRAP])
    }

    /// Snapshot the current inbox as a typed [`DmInboxSnapshot`].
    ///
    /// Messages are grouped per peer, each conversation ordered newest-first,
    /// and conversations ordered by their most-recent message (newest thread
    /// first). Ties break on a stable secondary key so the order is total and
    /// deterministic across snapshot ticks.
    ///
    /// D6: a poisoned mutex degrades to [`DmInboxSnapshot::empty`] rather than
    /// panicking — this runs on the actor thread inside a snapshot tick.
    pub fn snapshot(&self) -> DmInboxSnapshot {
        let Ok(messages) = self.messages.lock() else {
            return DmInboxSnapshot::empty();
        };

        // Group messages by conversation peer.
        let mut by_peer: BTreeMap<String, Vec<DmMessage>> = BTreeMap::new();
        for (peer, msg) in messages.values() {
            by_peer.entry(peer.clone()).or_default().push(msg.clone());
        }

        let mut conversations: Vec<DmConversation> = by_peer
            .into_iter()
            .map(|(peer_pubkey, mut msgs)| {
                // Newest-first within the thread. Tie-break on id descending
                // so the order is total even when two messages share a
                // `created_at`.
                msgs.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| b.id.cmp(&a.id))
                });
                DmConversation {
                    peer_pubkey,
                    messages: msgs,
                }
            })
            .collect();

        // Newest conversation first — keyed on the thread's most-recent
        // message (index 0 after the newest-first sort above). Tie-break on
        // peer pubkey descending for a total, stable order.
        conversations.sort_by(|a, b| {
            let a_latest = a.messages.first().map(|m| m.created_at).unwrap_or(0);
            let b_latest = b.messages.first().map(|m| m.created_at).unwrap_or(0);
            b_latest
                .cmp(&a_latest)
                .then_with(|| b.peer_pubkey.cmp(&a.peer_pubkey))
        });

        DmInboxSnapshot { conversations }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `{"conversations": []}` rather than propagating.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "conversations": [] }))
    }

    /// Decrypt and store one accepted kind:1059 envelope. Returns `true` when
    /// a kind:14 rumor was extracted and retained; `false` for every silent
    /// no-op path (not signed in, addressed to someone else, not a kind:14,
    /// poisoned mutex). Factored out of [`RawEventObserver::on_raw_event`] so
    /// the unit tests can assert the outcome.
    fn ingest_gift_wrap(&self, json: &str) -> bool {
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

        let message = DmMessage {
            id: message_id.clone(),
            sender_pubkey,
            content: rumor.content.clone(),
            // D7: the rumor's `created_at` is the sender's real send time.
            created_at: rumor.created_at.as_secs(),
            reply_to: first_reply_e_tag(rumor),
        };

        // Idempotent insert — a re-delivered envelope replaces rather than
        // duplicates. Poisoned mutex → silent no-op (D6).
        let Ok(mut messages) = self.messages.lock() else {
            return false;
        };
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
        let _ = self.ingest_gift_wrap(json);
    }
}

/// Stable, deterministic [`InterestId`] for a pubkey's NIP-17 gift-wrap
/// inbox subscription. The `"nip17.giftwrap"` namespace discriminant keeps it
/// distinct from any other kind:1059 interest — the kernel de-dupes the REQ
/// by filter hash, so two interests for
/// the same `#p` filter coalesce on the wire, but the ids stay separate so
/// each consumer owns its own registration.
fn giftwrap_interest_id(pubkey: &str) -> InterestId {
    InterestId(nmp_core::stable_hash::stable_hash64(("nip17.giftwrap", pubkey)))
}

/// Tailing [`LogicalInterest`] for kind:1059 `#p <pubkey>` gift-wraps — the
/// subscription a host pushes (via `NmpApp::push_interest`) so the DM inbox
/// actually receives envelopes.
///
/// Without this interest the kernel opens no REQ for kind:1059 `#p self` and
/// the [`DmInboxProjection`] sits empty regardless of how cleanly it is wired
/// — the "registered but inert" failure mode.
/// NIP-17 must push its own interest so envelopes are routed to this inbox.
///
/// Scope is [`InterestScope::Account`] (pinned to the resolved `pubkey`)
/// rather than `ActiveAccount`: the host resolves the concrete identity at
/// registration time and the subscription must stay pinned to it. The kernel
/// routes the `#p` filter to the account's mailbox relays; the raw-event tap
/// then drives every accepted kind:1059 event into the projection.
pub fn giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    deps.into_logical_interest(
        giftwrap_interest_id(pubkey),
        InterestScope::Account(pubkey.to_string()),
        InterestLifecycle::Tailing,
    )
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
            [name, value, _hint, marker, ..]
                if name == "e" && marker == "reply" =>
            {
                Some(value.clone())
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

    /// Build a signed kind:1059 gift-wrap envelope carrying a kind:14 rumor
    /// from `sender` to `receiver`, mirroring `nmp_nip59::gift_wrap`.
    fn gift_wrapped_dm(
        sender: &Keys,
        receiver: &nostr::PublicKey,
        content: &str,
        created_at: u64,
        reply_to: Option<&str>,
    ) -> Event {
        let mut tags = vec![Tag::public_key(*receiver)];
        if let Some(parent) = reply_to {
            // NIP-10 reply marker — `["e", <id>, <relay-hint>, "reply"]`.
            tags.push(
                Tag::parse([
                    "e".to_string(),
                    parent.to_string(),
                    String::new(),
                    "reply".to_string(),
                ])
                .expect("well-formed e tag"),
            );
        }
        let rumor = EventBuilder::new(Kind::from_u16(14), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .build(sender.public_key());
        nmp_nip59::gift_wrap(sender, receiver, rumor, None)
            .expect("gift wrap succeeds")
    }

    /// A projection bound to `keys` as the active local account.
    fn inbox_for(keys: &Keys) -> DmInboxProjection {
        DmInboxProjection::new(Arc::new(Mutex::new(Some(keys.clone()))))
    }

    #[test]
    fn fresh_inbox_yields_empty_snapshot() {
        let inbox = DmInboxProjection::new(Arc::new(Mutex::new(None)));
        assert_eq!(inbox.snapshot(), DmInboxSnapshot::empty());
        assert_eq!(
            inbox.snapshot_json(),
            serde_json::json!({ "conversations": [] })
        );
    }

    #[test]
    fn kind_filter_is_gift_wrap_only() {
        let filter = DmInboxProjection::kind_filter();
        assert!(filter.matches(1059), "kind:1059 gift-wrap must match");
        assert!(!filter.matches(14), "kind:14 must NOT match — it is sealed");
        assert!(!filter.matches(1), "plain notes must not match");
    }

    #[test]
    fn not_signed_in_is_silent_no_op() {
        // No local keys → every envelope is discarded, no panic.
        let inbox = DmInboxProjection::new(Arc::new(Mutex::new(None)));
        let alice = Keys::generate();
        let bob = Keys::generate();
        let envelope =
            gift_wrapped_dm(&alice, &bob.public_key(), "hi", 100, None);
        assert!(!inbox.ingest_gift_wrap(&envelope.as_json()));
        assert!(inbox.snapshot().conversations.is_empty());
    }

    #[test]
    fn malformed_json_is_silent_no_op() {
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        assert!(!inbox.ingest_gift_wrap("not json at all"));
        assert!(!inbox.ingest_gift_wrap("{}"));
        assert!(inbox.snapshot().conversations.is_empty());
    }

    #[test]
    fn envelope_for_another_recipient_is_discarded() {
        // Alice wraps a DM to Carol; Bob's inbox cannot decrypt it.
        let alice = Keys::generate();
        let bob = Keys::generate();
        let carol = Keys::generate();
        let inbox = inbox_for(&bob);
        let envelope =
            gift_wrapped_dm(&alice, &carol.public_key(), "secret", 100, None);
        assert!(
            !inbox.ingest_gift_wrap(&envelope.as_json()),
            "an envelope sealed for Carol must not decrypt for Bob"
        );
        assert!(inbox.snapshot().conversations.is_empty());
    }

    #[test]
    fn received_dm_surfaces_in_the_conversation() {
        // Alice → Bob. Bob's inbox decrypts and files it under peer = Alice.
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        let envelope = gift_wrapped_dm(
            &alice,
            &bob.public_key(),
            "hello bob",
            12345,
            None,
        );
        assert!(inbox.ingest_gift_wrap(&envelope.as_json()));

        let snap = inbox.snapshot();
        assert_eq!(snap.conversations.len(), 1);
        let convo = &snap.conversations[0];
        assert_eq!(
            convo.peer_pubkey,
            alice.public_key().to_hex(),
            "the conversation peer is the sender"
        );
        assert_eq!(convo.messages.len(), 1);
        let msg = &convo.messages[0];
        assert_eq!(msg.content, "hello bob");
        assert_eq!(msg.sender_pubkey, alice.public_key().to_hex());
        assert_eq!(msg.created_at, 12345, "D7: the rumor's send time verbatim");
        assert_eq!(msg.reply_to, None);
    }

    #[test]
    fn self_copy_files_under_the_recipient_peer() {
        // Bob sends to Alice and gift-wraps a self-copy to himself. Bob's
        // inbox decrypts the self-copy; the peer must be Alice (the `p` tag),
        // NOT Bob.
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        // The self-copy: sender == receiver == Bob, p-tag == Alice.
        let self_copy = {
            let rumor = EventBuilder::new(Kind::from_u16(14), "sent to alice")
                .tags(vec![Tag::public_key(alice.public_key())])
                .custom_created_at(Timestamp::from(500))
                .build(bob.public_key());
            nmp_nip59::gift_wrap(&bob, &bob.public_key(), rumor, None)
                .expect("self-copy gift wrap")
        };
        assert!(inbox.ingest_gift_wrap(&self_copy.as_json()));

        let snap = inbox.snapshot();
        assert_eq!(snap.conversations.len(), 1);
        assert_eq!(
            snap.conversations[0].peer_pubkey,
            alice.public_key().to_hex(),
            "a self-copy files under the recipient, not the local sender"
        );
        assert_eq!(
            snap.conversations[0].messages[0].sender_pubkey,
            bob.public_key().to_hex(),
            "the message author is still Bob (the local sender)"
        );
    }

    #[test]
    fn sent_and_received_share_one_conversation() {
        // A full round-trip: Alice→Bob (received) and Bob→Alice self-copy
        // (sent) both land in the SAME conversation keyed on peer = Alice.
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);

        let received =
            gift_wrapped_dm(&alice, &bob.public_key(), "hi bob", 100, None);
        inbox.ingest_gift_wrap(&received.as_json());

        let sent = {
            let rumor = EventBuilder::new(Kind::from_u16(14), "hi alice")
                .tags(vec![Tag::public_key(alice.public_key())])
                .custom_created_at(Timestamp::from(200))
                .build(bob.public_key());
            nmp_nip59::gift_wrap(&bob, &bob.public_key(), rumor, None)
                .expect("self-copy gift wrap")
        };
        inbox.ingest_gift_wrap(&sent.as_json());

        let snap = inbox.snapshot();
        assert_eq!(
            snap.conversations.len(),
            1,
            "sent + received with one peer is one thread"
        );
        let convo = &snap.conversations[0];
        assert_eq!(convo.messages.len(), 2);
        // Newest-first ordering within the thread.
        assert_eq!(convo.messages[0].content, "hi alice");
        assert_eq!(convo.messages[1].content, "hi bob");
    }

    #[test]
    fn reply_marker_is_extracted() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        let parent_id =
            "cc11223344556677889900aabbccddeeff00112233445566778899aabbccdd00";
        let envelope = gift_wrapped_dm(
            &alice,
            &bob.public_key(),
            "replying",
            300,
            Some(parent_id),
        );
        assert!(inbox.ingest_gift_wrap(&envelope.as_json()));

        let snap = inbox.snapshot();
        assert_eq!(
            snap.conversations[0].messages[0].reply_to.as_deref(),
            Some(parent_id),
            "the NIP-10 reply e-tag must surface as reply_to"
        );
    }

    #[test]
    fn duplicate_envelope_is_not_duplicated() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        let envelope =
            gift_wrapped_dm(&alice, &bob.public_key(), "once", 100, None);
        // Same envelope delivered twice — the inner rumor id is identical.
        inbox.ingest_gift_wrap(&envelope.as_json());
        inbox.ingest_gift_wrap(&envelope.as_json());
        let snap = inbox.snapshot();
        assert_eq!(snap.conversations.len(), 1);
        assert_eq!(
            snap.conversations[0].messages.len(),
            1,
            "a re-delivered envelope must not duplicate the message"
        );
    }

    #[test]
    fn conversations_ordered_by_most_recent_message() {
        // Two peers; the one with the newer message must sort first.
        let alice = Keys::generate();
        let carol = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);

        inbox.ingest_gift_wrap(
            &gift_wrapped_dm(&alice, &bob.public_key(), "older", 100, None)
                .as_json(),
        );
        inbox.ingest_gift_wrap(
            &gift_wrapped_dm(&carol, &bob.public_key(), "newer", 900, None)
                .as_json(),
        );

        let snap = inbox.snapshot();
        assert_eq!(snap.conversations.len(), 2);
        assert_eq!(
            snap.conversations[0].peer_pubkey,
            carol.public_key().to_hex(),
            "the conversation with the newest message sorts first"
        );
    }

    #[test]
    fn drives_through_raw_observer_trait_object() {
        // The projection must be usable as `Arc<dyn RawEventObserver>` — that
        // is exactly how a host FFI registers it.
        let alice = Keys::generate();
        let bob = Keys::generate();
        let proj = Arc::new(inbox_for(&bob));
        let observer: Arc<dyn RawEventObserver> = Arc::clone(&proj) as _;
        let envelope =
            gift_wrapped_dm(&alice, &bob.public_key(), "via trait", 100, None);
        observer.on_raw_event(1059, &envelope.as_json());
        assert_eq!(proj.snapshot().conversations.len(), 1);
    }

    #[test]
    fn giftwrap_inbox_interest_is_account_scoped_and_p_filtered() {
        let interest = giftwrap_inbox_interest("selfpubkey");
        assert!(interest.shape.kinds.contains(&KIND_GIFT_WRAP));
        assert!(interest
            .shape
            .tags
            .get("p")
            .map(|s| s.contains("selfpubkey"))
            .unwrap_or(false));
        assert!(interest.shape.relay_pin.is_none());
        assert!(matches!(
            interest.lifecycle,
            InterestLifecycle::Tailing
        ));
        assert!(matches!(
            interest.scope,
            InterestScope::Account(ref pk) if pk == "selfpubkey"
        ));
    }

    #[test]
    fn giftwrap_interest_id_is_deterministic_per_pubkey() {
        assert_eq!(
            giftwrap_interest_id("abc"),
            giftwrap_interest_id("abc"),
            "same pubkey → same id (idempotent re-registration)"
        );
        assert_ne!(
            giftwrap_interest_id("abc"),
            giftwrap_interest_id("def"),
            "different pubkeys → different ids"
        );
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let inbox = inbox_for(&bob);
        inbox.ingest_gift_wrap(
            &gift_wrapped_dm(&alice, &bob.public_key(), "hi", 100, None)
                .as_json(),
        );
        let snap = inbox.snapshot();
        let encoded = serde_json::to_string(&snap).expect("serialises");
        let decoded: DmInboxSnapshot =
            serde_json::from_str(&encoded).expect("deserialises");
        assert_eq!(snap, decoded);
    }
}
