//! Bug 1 (privacy) — `Nip65OutboxResolver` must exclude blocked relays from
//! EVERY publish resolution lane.
//!
//! Before the fix the publish-side outbox resolver had no blocked set and
//! returned every relay unfiltered — so an author's events leaked to relays
//! they explicitly blocked. The subscribe-side
//! `GenericOutboxRouter` always filtered blocked relays per-lane; the
//! publish-side resolver now matches that behaviour via a `blocked` param.
//!
//! Split into its own test file (rather than appended to `tests.rs`) so the
//! pre-existing over-500-LOC `tests.rs` does not grow further (AGENTS.md
//! file-size ceiling).

use std::sync::Arc;

use super::{Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD};
use crate::{InMemoryMailboxCache, Kind10002Parser};
use nmp_core::publish::{OutboxResolver, PublishTarget, ResolvedRelay};
use nmp_core::slots::{new_indexer_relays_slot, IndexerRelaysSlot};
use nmp_core::substrate::{BlockedRelaySet, MailboxCache};
use nmp_store::{RawEvent, VerifiedEvent};

const AUTHOR_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

// Silence unused-import lints when the threshold constant is only referenced
// in comments below — referenced here so the module compiles cleanly.
const _: usize = RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD;

fn indexer_slot_with(urls: Vec<String>) -> IndexerRelaysSlot {
    let slot = new_indexer_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

fn seed_kind10002(cache: &Arc<InMemoryMailboxCache>, author_hex: &str, tags: Vec<Vec<String>>) {
    let prefix = &author_hex[..2];
    let id = format!("{:0<64}", format!("{}e10002", prefix));
    let raw = RawEvent {
        id,
        pubkey: author_hex.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    Kind10002Parser::new(Arc::clone(cache)).parse_event(&verified);
}

fn resolver(cache: &Arc<InMemoryMailboxCache>) -> Nip65OutboxResolver {
    let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
    Nip65OutboxResolver::new(mailbox_cache, new_indexer_relays_slot())
}

fn urls_of(resolved: &[ResolvedRelay]) -> Vec<String> {
    resolved.iter().map(|r| r.url.clone()).collect()
}

fn blocked_with(urls: &[&str]) -> BlockedRelaySet {
    let mut set = BlockedRelaySet::new();
    for u in urls {
        set.insert((*u).to_string());
    }
    set
}

#[test]
fn blocked_relay_excluded_from_publish_resolution() {
    // Author's kind:10002 write set has two relays; one is blocked.
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![
            vec!["r".into(), "wss://good.example".into(), "write".into()],
            vec!["r".into(), "wss://blocked.example".into(), "write".into()],
        ],
    );
    let resolver = resolver(&cache);

    let blocked = blocked_with(&["wss://blocked.example"]);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &blocked);
    let urls = urls_of(&out);

    assert!(
        urls.contains(&"wss://good.example".to_string()),
        "unblocked write relay must survive, got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://blocked.example".to_string()),
        "blocked write relay must be excluded from publish resolution, got {urls:?}"
    );
}

#[test]
fn blocked_relay_excluded_from_recipient_inbox() {
    // Author writes to a good relay; recipient's kind:10002 read set has a
    // blocked relay that must not appear in the recipient-inbox fan-out.
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    seed_kind10002(
        &cache,
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://blocked-inbox.example".into(),
            "read".into(),
        ]],
    );
    let resolver = resolver(&cache);

    let blocked = blocked_with(&["wss://blocked-inbox.example"]);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &blocked,
    );
    let urls = urls_of(&out);

    assert!(
        urls.contains(&"wss://author-write.example".to_string()),
        "author write relay must survive, got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://blocked-inbox.example".to_string()),
        "blocked recipient inbox relay must be excluded, got {urls:?}"
    );
}

#[test]
fn blocked_relay_excluded_from_indexer_fanout() {
    // Discovery kind (kind:0) fans out to the indexer relays; a blocked
    // indexer must be excluded from the output.
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = Nip65OutboxResolver::new(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        indexer_slot_with(vec![
            "wss://good-indexer.example".to_string(),
            "wss://blocked-indexer.example".to_string(),
        ]),
    );

    let blocked = blocked_with(&["wss://blocked-indexer.example"]);
    // kind:0 is a discovery kind → lane 3 (indexer fan-out) fires.
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 0, &blocked);
    let urls = urls_of(&out);

    assert!(
        urls.contains(&"wss://good-indexer.example".to_string()),
        "unblocked indexer must survive, got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://blocked-indexer.example".to_string()),
        "blocked indexer must be excluded from discovery fan-out, got {urls:?}"
    );
}

#[test]
fn blocked_relay_excluded_with_differing_case() {
    // The kind:10002 ingest path canonicalises URLs (lowercase host), so a
    // write entry `wss://Block.Example` is stored as `wss://block.example`.
    // A blocked set carrying `wss://block.example` must therefore match —
    // even though the originating kind:10002 tag used mixed case. This proves
    // the canonicalisation parity between kind:10002 ingest and blocked
    // lookup entries (Bug 2 ⇄ Bug 1 interaction).
    //
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://Block.Example".into(),
            "write".into(),
        ]],
    );
    let resolver = resolver(&cache);

    // Block set built from the canonical (lowercase) form — the same form the
    // blocked-relay lookup produces from a `wss://Block.Example` source.
    let blocked = blocked_with(&["wss://block.example"]);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &blocked);
    let urls = urls_of(&out);

    assert!(
        !urls.contains(&"wss://block.example".to_string()),
        "canonicalised blocked relay must match the canonical write entry, got {urls:?}"
    );
    assert!(
        urls.is_empty(),
        "only relay was blocked → empty resolution, got {urls:?}"
    );
}

#[test]
fn non_canonical_source_tag_is_blocked_when_canonical_form_is_in_blocked_set() {
    // Security: the blocked set stores canonical (lowercase) relay URLs.  A
    // kind:10002 tag with a non-canonical spelling (e.g. mixed-case host
    // `wss://Block.Example/`) must be canonicalized by `Kind10002Parser`
    // before the resolver ever reads the mailbox cache. Otherwise the
    // non-canonical spelling could escape the blocked-set filter.
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        // Intentionally NON-canonical: uppercase host + trailing slash.
        // Canonical form is `wss://block.example` (no trailing slash, lowercase).
        vec![vec![
            "r".into(),
            "wss://Block.Example/".into(),
            "write".into(),
        ]],
    );
    let resolver = resolver(&cache);

    // Blocked set uses the canonical form.
    let blocked = blocked_with(&["wss://block.example"]);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &blocked);
    let urls = urls_of(&out);

    assert!(
        !urls.contains(&"wss://Block.Example/".to_string()),
        "non-canonical raw form must not appear in output; parser canonicalization feeds the resolver, got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://block.example".to_string()),
        "canonical form of blocked relay must not appear in output, got {urls:?}"
    );
    assert!(
        urls.is_empty(),
        "only relay was a non-canonical spelling of a blocked relay → must resolve empty, got {urls:?}"
    );
}

#[test]
fn non_canonical_tag_without_matching_block_canonicalizes_and_is_included() {
    // Complement of the above: a non-canonical source tag for an UNBLOCKED
    // relay must resolve from the parser-owned cache to the canonical form.
    // Fail-closed does not mean all non-canonical URLs are dropped — only
    // un-canonicalizable ones are.
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        // Non-canonical: uppercase host + trailing slash.
        vec![vec![
            "r".into(),
            "wss://Good.Example/".into(),
            "write".into(),
        ]],
    );
    let resolver = resolver(&cache);

    // Nothing blocked.
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::Auto,
        1,
        &BlockedRelaySet::new(),
    );
    let urls = urls_of(&out);

    // The resolver must return the CANONICAL form, not the raw source form.
    assert!(
        urls.contains(&"wss://good.example".to_string()),
        "non-canonical tag for unblocked relay must resolve to its canonical form, got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://Good.Example/".to_string()),
        "raw non-canonical form must not appear in output; parser canonicalization feeds the resolver, got {urls:?}"
    );
}
