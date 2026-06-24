//! Cache-serve regression test: an all-kinds range parser receives kind:1
//! events served from the store.
//!
//! Gap closed by this PR (Finding 3): `shape_needs_ingest_parser_dispatch`
//! only returned true for `#p`+kind:1059 DM shapes, so a KindTime / AuthorKind
//! cache-serve for kind:1 (the follow-feed) would never call
//! `ingest_dispatcher_slot()…dispatch()` for a registered all-kinds range parser
//! (e.g. chirp-tui's `RawCacheIngestParser`, slot `"chirp-tui.raw-cache"`).
//!
//! The architecturally-right fix (owner doctrine: one uniform mechanism):
//! replace the hardcoded shape allowlist with a per-kind registry query
//! (`EventIngestDispatcher::is_interested(kind)`) so ANY registered parser
//! — including future ones — causes cache-serve dispatch without code changes.

pub(super) mod tests_kind0;
pub(super) mod tests_kind1;
pub(super) mod tests_replaceables;

use super::cache_serve_tests::{
    drain_cache_serves, hex_pk, seed_events, signed_note, simulate_cold_restart,
};
use super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

pub(super) struct CapturingIngestParser {
    pub(super) seen_kinds: Mutex<Vec<u32>>,
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
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

pub(super) fn sub_id(seed: u64) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    )
}

pub(super) fn open_author_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

/// Ingest an arbitrary signed event through the REAL live path (`handle_event`
/// → `verify_and_persist` → store + projection), assembling the wire `Value`
/// `serde_json::from_value::<NostrEvent>` expects (NostrEvent is not Serialize).
pub(super) fn live_ingest(kernel: &mut Kernel, sub_id: &str, ev: &NostrEvent) {
    let value = serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "created_at": ev.created_at,
        "kind": ev.kind,
        "tags": ev.tags,
        "content": ev.content,
        "sig": ev.sig,
    });
    kernel.handle_event(RelayRole::Indexer, "wss://relay.test/", sub_id, &value);
}

/// Real signed kind:0 profile in `NostrEvent` shape with the given display name.
pub(super) fn signed_kind0(keys: &::nostr::Keys, display_name: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let content = format!(r#"{{"display_name":"{display_name}"}}"#);
    let ev = EventBuilder::new(Kind::Metadata, content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// Open a kind:0 follow-feed interest for `author_hex` so the cache-serve path
/// will replay stored kind:0 events for that author.
pub(super) fn open_kind0_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([0u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

pub(super) fn profiles_ver(kernel: &Kernel) -> u64 {
    kernel
        .projection_rev_tracker
        .source_versions
        .get(crate::kernel::projection_rev::SRC_PROFILES)
}

/// Real signed kind:10002 (NIP-65) in `NostrEvent` shape with one write relay.
pub(super) fn signed_kind10002(keys: &::nostr::Keys, write_relay: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let ev = EventBuilder::new(Kind::RelayList, "")
        .tags([::nostr::Tag::parse(["r", write_relay, "write"]).expect("valid r tag")])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

pub(super) fn open_kind10002_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([10_002u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

/// Minimal kind:10002 ingest parser writing the substrate mailbox cache —
/// mirrors `nmp_router::Kind10002Parser` (which `nmp-core` cannot depend on).
pub(super) struct TestKind10002Parser {
    pub(super) cache: Arc<dyn crate::substrate::MailboxCache>,
}

impl crate::substrate::IngestParser for TestKind10002Parser {
    fn parse(&self, evt: &crate::store::VerifiedEvent) {
        let raw = evt.raw();
        if raw.kind != 10_002 {
            return;
        }
        let parsed = super::parse_relay_list_to_substrate(&raw.tags);
        let empty = parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
        if empty {
            self.cache.remove(&raw.pubkey);
        } else {
            self.cache.upsert(raw.pubkey.clone(), parsed);
        }
    }
}
