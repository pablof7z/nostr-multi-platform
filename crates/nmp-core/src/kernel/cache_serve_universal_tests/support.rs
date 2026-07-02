//! Shared fixtures for the universal-acceptance cache-serve suite: an
//! `IngestParser` that records every kind it sees, signed-event / gift-wrap
//! JSON builders, and the interest-open / interest-register helpers the
//! acceptance tests drive directly.

use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
use crate::kernel::Kernel;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::sync::{Arc, Mutex};

/// An `IngestParser` that records the kind of every event it receives.
/// Used to verify kind:1059 events reach the IngestParser seam after cache-serve
/// (PR-1 of the raw-tap retirement ladder).
pub(super) struct CapturingIngestParser {
    seen_kinds: Mutex<Vec<u32>>,
}

impl CapturingIngestParser {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_kinds: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn seen(&self) -> Vec<u32> {
        self.seen_kinds.lock().unwrap().clone()
    }

    pub(super) fn clear(&self) {
        self.seen_kinds.lock().unwrap().clear();
    }
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

/// Build a NIP-01 JSON `Value` for a signed event via `handle_event`-compatible
/// format (same pattern as the Nostr signing helpers in the test suite).
pub(super) fn signed_event_json(
    keys: &::nostr::Keys,
    kind: u32,
    content: &str,
    tags: Vec<Vec<String>>,
    created_at: u64,
) -> serde_json::Value {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let nostr_tags: Vec<Tag> = tags
        .iter()
        .map(|t| Tag::parse(t.as_slice()).expect("well-formed tag"))
        .collect();
    let ev = EventBuilder::new(Kind::from(kind as u16), content)
        .tags(nostr_tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    })
}

/// Build and return a kind:1059 gift-wrap JSON `Value` from `sender` to
/// `receiver`, using `nmp_nip59::gift_wrap_local` — the same pure seal/wrap
/// composition the production DM chain assembles on the actor thread.
pub(super) fn gift_wrap_json(
    sender: &::nostr::Keys,
    receiver: &::nostr::PublicKey,
    content: &str,
    created_at: u64,
) -> (serde_json::Value, String) {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};

    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());

    let envelope =
        nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(created_at))
            .expect("gift_wrap_local succeeds with local keys");

    let tag_vecs: Vec<Vec<String>> = envelope
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    let json = serde_json::json!({
        "id": envelope.id.to_hex(),
        "pubkey": envelope.pubkey.to_hex(),
        "created_at": envelope.created_at.as_secs(),
        "kind": envelope.kind.as_u16(),
        "tags": tag_vecs,
        "content": envelope.content.clone(),
        "sig": envelope.sig.to_string(),
    });
    let id = envelope.id.to_hex();
    (json, id)
}

/// Construct a `SubIdentity` for opening a generic non-feed interest.
pub(super) fn sub_identity(seed: u64) -> SubIdentity {
    SubIdentity::new(SubOwnerKey::new(seed), SubKey::new(seed), SubScope::Global)
}

/// Open a cache-serve interest for `shape` and return whether it was newly
/// installed. Mirrors the production `open_interest_sub` path.
pub(super) fn open_interest(kernel: &mut Kernel, seed: u64, shape: InterestShape) -> bool {
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_identity(seed), interest)
}

/// Enqueue a fresh cache-serve interest via `register_interest`.
pub(super) fn register_one(
    kernel: &mut Kernel,
    owner: &'static str,
    key: SubKey,
    shape: InterestShape,
    reason: &'static str,
) {
    kernel.register_interest(
        &[InterestRegistration {
            identity: SubIdentity::new(SubOwnerKey::new(owner), key, SubScope::Global),
            interest: LogicalInterest {
                shape,
                ..Default::default()
            },
            policy: InterestWrite::EnsureAbsent,
        }],
        reason,
    );
}
