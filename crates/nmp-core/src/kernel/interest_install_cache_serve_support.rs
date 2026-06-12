//! Shared test-support fixtures for the ADR-0045 interest-install cache-serve
//! regression tests (`interest_install_cache_serve_tests`).
//!
//! Extracted into a sibling `_support` module so the test file stays under the
//! 500-LOC hard ceiling (AGENTS.md). The `_support.rs` suffix is the codebase
//! convention for a test-support facade whose `#[cfg(test)]` gate lives in the
//! parent module (doctrine-lint exempts it from D6 via `file_is_test_only`).
//! Helpers are `pub(super)` — visible only to the `kernel` test tree.

use super::Kernel;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::RelayRole;
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// Minimal `IngestParser` that records every `(kind, id)` it receives.
pub(super) struct CapturingParser {
    seen: Mutex<Vec<(u32, String)>>,
}

impl CapturingParser {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn seen_kinds(&self) -> Vec<u32> {
        self.seen.lock().unwrap().iter().map(|(k, _)| *k).collect()
    }

    pub(super) fn seen_ids(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, id)| id.clone())
            .collect()
    }

    pub(super) fn clear(&self) {
        self.seen.lock().unwrap().clear();
    }
}

impl IngestParser for CapturingParser {
    fn parse(&self, evt: &VerifiedEvent) {
        let raw = evt.raw();
        self.seen.lock().unwrap().push((raw.kind, raw.id.clone()));
    }
}

/// Build a `LogicalInterest` for `kind:1` from `author_hex`.
pub(super) fn author_kind1_interest(id: u64, author_hex: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: BTreeSet::from([author_hex.to_string()]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a `LogicalInterest` for `kind:443` (Marmot key-package kind).
pub(super) fn kp_interest(id: u64, target_pubkey: &str) -> LogicalInterest {
    // kind:443 is the Marmot MLS key-package kind. We model it as a
    // #p-tagged interest (the real Marmot interest shape uses #p to match
    // KPs published for a specific recipient pubkey).
    let mut shape = InterestShape {
        kinds: BTreeSet::from([443u32]),
        ..Default::default()
    };
    shape
        .tags
        .insert("p".to_string(), BTreeSet::from([target_pubkey.to_string()]));
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a `SubIdentity` for a generic non-feed interest.
pub(super) fn sub_id(seed: u64) -> SubIdentity {
    SubIdentity::new(SubOwnerKey::new(seed), SubKey::new(seed), SubScope::Global)
}

/// Seed a kind:443 (Marmot KP) event addressed to `target_hex` into the
/// kernel's store via `handle_event` (the live ingest path that persists to the
/// store). Returns the event id.
pub(super) fn seed_kp_event(
    kernel: &mut Kernel,
    keys: &::nostr::Keys,
    target_hex: &str,
    ts: u64,
) -> String {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let target_pk: ::nostr::PublicKey = target_hex.parse().expect("valid hex pubkey");
    let ev = EventBuilder::new(Kind::from(443u16), "kp-payload")
        .tags(vec![Tag::public_key(target_pk)])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with generated keys");
    let tag_vecs: Vec<Vec<String>> = ev
        .tags
        .iter()
        .map(|t: &::nostr::Tag| t.as_slice().to_vec())
        .collect();
    let json = serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": tag_vecs,
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    });
    let id = ev.id.to_hex();
    kernel.handle_event(RelayRole::Content, "wss://relay.test/", "kp-sub", &json);
    id
}

/// Seed a kind:0 (profile metadata) event authored by `keys` into the kernel's
/// store via the live ingest path. Returns the event id. Used by the open_uri
/// regression test (open_uri registers a kind:0 profile interest for the npub).
pub(super) fn seed_kind0_event(kernel: &mut Kernel, keys: &::nostr::Keys, ts: u64) -> String {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let ev = EventBuilder::new(Kind::from(0u16), r#"{"name":"alice"}"#)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with generated keys");
    let json = serde_json::json!({
        "id": ev.id.to_hex(),
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": Vec::<Vec<String>>::new(),
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    });
    let id = ev.id.to_hex();
    kernel.handle_event(RelayRole::Content, "wss://relay.test/", "meta-sub", &json);
    id
}
