//! Tests for the `ptags_are_recipients` semantic gate (issue #1631).
//!
//! Regression suite: kind:3 (contact lists) and other replaceable/addressable
//! events must NOT fan out to the inbox/read relays of every p-tagged pubkey.
//! Those p-tags are list SUBJECTS, not message recipients. Only regular
//! (non-replaceable, non-addressable) events get step-4 inbox fan-out.
//!
//! Split from `tests.rs` to keep both files under the 500 LOC hand-authored
//! ceiling (AGENTS.md).

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD};
use crate::{InMemoryMailboxCache, Kind10002Parser};
use nmp_core::publish::{OutboxResolver, PublishTarget, ResolvedRelay};
use nmp_core::slots::new_indexer_relays_slot;
use nmp_core::substrate::{BlockedRelaySet, MailboxCache};
use nmp_store::{RawEvent, VerifiedEvent};

fn no_block() -> BlockedRelaySet {
    BlockedRelaySet::new()
}

fn urls_of(resolved: &[ResolvedRelay]) -> BTreeSet<String> {
    resolved.iter().map(|r| r.url.clone()).collect()
}

const AUTHOR_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

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

fn mk_resolver(cache: &Arc<InMemoryMailboxCache>) -> Nip65OutboxResolver {
    let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
    Nip65OutboxResolver::new(mailbox_cache, new_indexer_relays_slot())
}

fn pk(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn threshold_recipients() -> Vec<String> {
    let mut recipients = vec![RECIPIENT_HEX.to_string()];
    recipients.extend((0..RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD - 1).map(|i| pk((i + 3) as u8)));
    recipients
}

/// kind:3 contact list — the author's followees are SUBJECTS, not recipients.
/// Even with fewer than RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD followees, the
/// followee's read relay must NOT appear in the resolve output.
#[test]
fn kind3_does_not_fan_out_to_followee_inbox() {
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
            "wss://followee-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    // kind=3, one followee — below threshold, but ptags_are_recipients(3) == false
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        3,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(
        urls.contains("wss://author-write.example"),
        "author write relay must be present"
    );
    assert!(
        !urls.contains("wss://followee-read.example"),
        "kind:3 contact list must NOT fan out to followee inbox relay — \
         followees are list subjects, not message recipients"
    );
}

/// kind:10000 mute list — the muted pubkeys are SUBJECTS, not recipients.
#[test]
fn kind10000_mute_list_does_not_fan_out_to_subject_inbox() {
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
            "wss://muted-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        10_000,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(urls.contains("wss://author-write.example"));
    assert!(
        !urls.contains("wss://muted-read.example"),
        "kind:10000 mute list must NOT fan out to muted pubkey's inbox relay"
    );
}

/// kind:30000 follow set (NIP-51 addressable) — list members are SUBJECTS.
/// Proves the gate covers the full addressable range, not just regular-replaceable.
#[test]
fn kind30000_follow_set_does_not_fan_out_to_subject_inbox() {
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
            "wss://member-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        30_000,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(urls.contains("wss://author-write.example"));
    assert!(
        !urls.contains("wss://member-read.example"),
        "kind:30000 follow set (addressable) must NOT fan out to list-member inbox relay"
    );
}

/// kind:0 profile metadata — replaceable, any p-tag is a subject, not a recipient.
#[test]
fn kind0_profile_does_not_fan_out_to_ptag_inbox() {
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
            "wss://ptag-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        0,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(urls.contains("wss://author-write.example"));
    assert!(
        !urls.contains("wss://ptag-read.example"),
        "kind:0 profile metadata must NOT fan out to p-tagged pubkey's inbox relay"
    );
}

/// Positive pin — kind:1 short text note with a mention STILL fans out to the
/// mentioned pubkey's read relays. This locks the recipient-semantics path so
/// the new gate cannot accidentally break regular note routing.
#[test]
fn kind1_mention_still_fans_out_to_mentioned_pubkey_inbox() {
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
            "wss://mention-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(
        urls.contains("wss://author-write.example"),
        "author write relay must be present for kind:1"
    );
    assert!(
        urls.contains("wss://mention-read.example"),
        "kind:1 mention MUST fan out to mentioned pubkey's inbox relay — \
         p-tags on regular events are message recipients"
    );
}

/// Threshold still applies for regular kinds — even kind:1 with >= 15 p-tags
/// skips inbox fan-out. The `ptags_are_recipients` gate is additive, not a
/// replacement for the volume gate.
#[test]
fn kind1_at_threshold_still_skips_inbox_fanout() {
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
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let recipients = threshold_recipients();
    let resolver = mk_resolver(&cache);
    // kind=1 (recipient semantics) but at/above threshold — no inbox fan-out
    let out = resolver.resolve(
        AUTHOR_HEX,
        &recipients,
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(urls.contains("wss://author-write.example"));
    assert!(
        !urls.contains("wss://recipient-read.example"),
        "kind:1 at/above threshold must NOT fan out to recipient inbox — \
         both gates (semantics AND volume) must pass"
    );
}
