//! Composition proof for kind:10006 blocked relays in feed acquisition.
//!
//! `blocked_relay_composition.rs` proves `register_substrate` wires the
//! kind:10006 parser and lookup to the same cache. `nmp-core` unit tests prove
//! the lifecycle can subtract an already-provided blocked set. This test pins
//! the cross-crate path those two facts are meant to create: a real
//! kind:10006 parser update changes the blocked set consumed by the next feed
//! compile, so the blocked relay is closed and receives no further REQ.

use std::sync::Arc;

use nmp_core::subs::{SubscriptionLifecycle, WireFrame};
use nmp_core::substrate::BlockedRelayLookup;
use nmp_planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use nmp_router::{InMemoryBlockedRelayCache, Kind10006Parser};
use nmp_store::{RawEvent, VerifiedEvent};

const ACTIVE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BLOCKED_RELAY: &str = "wss://blocked.example";
const ALLOWED_RELAY: &str = "wss://allowed.example";

fn feed_interest() -> LogicalInterest {
    LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [AUTHOR.to_string()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        lifecycle: InterestLifecycle::Tailing,
        ..Default::default()
    }
}

fn mailbox_cache() -> InMemoryMailboxCache {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        AUTHOR.to_string(),
        MailboxSnapshot {
            write_relays: vec![BLOCKED_RELAY.to_string(), ALLOWED_RELAY.to_string()],
            ..Default::default()
        },
    );
    cache
}

fn kind10006(relays: &[&str]) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "11".repeat(32),
        pubkey: ACTIVE.to_string(),
        created_at: 100,
        kind: 10_006,
        tags: relays
            .iter()
            .map(|relay| vec!["relay".to_string(), (*relay).to_string()])
            .collect(),
        content: String::new(),
        sig: "22".repeat(64),
    })
}

fn req_relays(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            WireFrame::Close { .. } => None,
        })
        .collect()
}

fn close_relays(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
            WireFrame::Req { .. } => None,
        })
        .collect()
}

#[test]
fn kind10006_parser_update_closes_blocked_feed_acquisition_relay() {
    let blocked_cache = Arc::new(InMemoryBlockedRelayCache::new());
    let parser = Kind10006Parser::new(Arc::clone(&blocked_cache));
    let mailbox = mailbox_cache();
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle.set_indexer_relays(Vec::new());
    lifecycle.register_for_test(feed_interest());

    let initially_open = lifecycle
        .recompile_and_diff_with_blocked(&mailbox, None, &blocked_cache.blocked_relays(ACTIVE))
        .expect("initial feed acquisition compiles");
    assert_eq!(
        req_relays(&initially_open)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [BLOCKED_RELAY.to_string(), ALLOWED_RELAY.to_string()]
            .into_iter()
            .collect(),
        "precondition: the followed author's mailbox would route to both relays"
    );

    assert!(parser.parse_event(&kind10006(&[BLOCKED_RELAY])));
    let after_block = lifecycle
        .recompile_and_diff_with_blocked(&mailbox, None, &blocked_cache.blocked_relays(ACTIVE))
        .expect("blocked feed acquisition recompile succeeds");

    assert!(
        !req_relays(&after_block).contains(&BLOCKED_RELAY.to_string()),
        "kind:10006 blocked relay must not receive a replacement REQ"
    );
    assert_eq!(
        close_relays(&after_block),
        vec![BLOCKED_RELAY.to_string()],
        "an already-open feed subscription on the newly-blocked relay must be closed"
    );
}

#[test]
fn kind10006_blocked_relay_is_absent_from_first_feed_req() {
    let blocked_cache = Arc::new(InMemoryBlockedRelayCache::new());
    let parser = Kind10006Parser::new(Arc::clone(&blocked_cache));
    assert!(parser.parse_event(&kind10006(&[BLOCKED_RELAY])));

    let mailbox = mailbox_cache();
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle.set_indexer_relays(Vec::new());
    lifecycle.register_for_test(feed_interest());

    let frames = lifecycle
        .recompile_and_diff_with_blocked(&mailbox, None, &blocked_cache.blocked_relays(ACTIVE))
        .expect("feed acquisition compiles with blocked set");
    assert_eq!(
        req_relays(&frames),
        vec![ALLOWED_RELAY.to_string()],
        "fresh feed acquisition must exclude relays from the active account's kind:10006 list"
    );
}
