//! Relay-hint and cold-park coverage for event `resolve_ref`.
//!
//! These tests intentionally live outside `event_claim_tests.rs` so the core
//! refcount/projection tests and the relay-hint routing tests stay under the
//! file-size gate as separate ownership areas.

use super::*;
use crate::kernel::{EventShape, RefLiveness};
use nmp_nostr_id::{NeventData, encode_nevent};
use nmp_nostr_id::{parse_nostr_uri, NostrUri};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

/// Helper: build a 64-hex event id from a single-char prefix (rest zeros).
fn hex64(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

/// Helper: build an `nostr:nevent…` URI for an event with no relay hints.
fn nevent_uri(event_id: &str, kind: Option<u32>, author: Option<&str>) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: author.map(str::to_string),
        kind,
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

/// Helper: build an `nostr:nevent…` URI carrying NIP-19 relay TLVs.
fn nevent_uri_with_relays(event_id: &str, relays: &[&str]) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: relays.iter().map(|r| (*r).to_string()).collect(),
        author: None,
        kind: Some(1),
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

fn event_key_and_hints_from_uri(uri: &str) -> Option<(String, Vec<String>)> {
    match parse_nostr_uri(uri).ok()? {
        NostrUri::Event {
            event_id, relays, ..
        } => Some((event_id, relays)),
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => Some((format!("{kind}:{pubkey}:{identifier}"), relays)),
        NostrUri::Profile { .. } => None,
    }
}

fn resolve_event_uri(
    kernel: &mut Kernel,
    uri: &str,
    consumer_id: impl Into<String>,
    can_send: bool,
    force: bool,
) -> Vec<OutboundMessage> {
    let Some((key, hints)) = event_key_and_hints_from_uri(uri) else {
        return Vec::new();
    };
    kernel.resolve_event_ref(
        key,
        consumer_id.into(),
        EventShape::Embed,
        RefLiveness::CacheOk,
        force,
        can_send,
        hints,
    )
}

/// Helper: drain the planner and collect the relay URLs every compiled REQ
/// targets. Used by the Fix B tests to prove a claim's REQ reaches the hint
/// relay.
fn drained_req_targets(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .drain_lifecycle_tick()
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

/// A claim whose URI carries NIP-19 relay TLVs seeds the INITIAL OneshotApi
/// interest's `hints` with those relays, so the first REQ fans out to
/// publisher-provided content relays plus bootstrap lanes.
#[test]
fn resolve_event_ref_seeds_initial_interest_hints_from_uri_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("e2");
    let uri = nevent_uri_with_relays(&id, &["wss://relay.a.example", "wss://relay.b.example"]);

    let _ = resolve_event_uri(&mut kernel, &uri, "view-0".to_string(), true, false);

    let active = kernel.lifecycle.registry_mut().iter_active();
    let hint_urls: std::collections::BTreeSet<String> = active
        .iter()
        .flat_map(|i| i.hints.iter().map(|h| h.url.clone()))
        .collect();
    assert_eq!(
        hint_urls,
        ["wss://relay.a.example", "wss://relay.b.example"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        "the claim's first REQ must carry the URI relay TLVs as interest hints"
    );
    assert!(
        active
            .iter()
            .flat_map(|i| i.hints.iter())
            .all(|h| h.source == crate::planner::HintSource::UserConfigured),
        "URI-sourced relay hints must use the UserConfigured source variant"
    );
}

/// A claim whose URI carries NO relay TLVs registers an interest with EMPTY
/// hints, byte-identical to the pre-hint behavior.
#[test]
fn resolve_event_ref_without_uri_relays_registers_empty_hints() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("e3");
    let uri = nevent_uri(&id, Some(1), None);

    let _ = resolve_event_uri(&mut kernel, &uri, "view-0".to_string(), true, false);

    let active = kernel.lifecycle.registry_mut().iter_active();
    assert_eq!(active.len(), 1, "exactly one oneshot interest registered");
    assert!(
        active[0].hints.is_empty(),
        "a hint-less claim URI must register an interest with no hints"
    );
}

/// A claim that hits the cold-start `!can_send` branch but carries relay hints
/// must not fully park: it registers the hint-seeded OneshotApi interest so
/// the planner compiles a REQ targeting the hint relay.
#[test]
fn resolve_event_ref_parked_with_uri_hint_registers_and_targets_hint_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("f1");
    let hint = "wss://hint.publisher.example";
    let uri = nevent_uri_with_relays(&id, &[hint]);

    let outbound = resolve_event_uri(&mut kernel, &uri, "view-hint".to_string(), false, false);
    assert!(
        outbound.is_empty(),
        "resolve_event_ref returns Vec::new(); wire frames flow through the planner"
    );

    assert!(
        kernel.event_claim_is_requested_for_test(&id),
        "a parked claim carrying a relay hint must register its hint-seeded interest"
    );
    assert_eq!(
        kernel.pending_event_claims_len_for_test(),
        0,
        "a hint-bearing claim must not be left in pending_event_claims"
    );

    let req_targets = drained_req_targets(&mut kernel);
    assert!(
        req_targets
            .iter()
            .any(|u| u.contains("hint.publisher.example")),
        "a compiled REQ must target the URI hint relay; got {req_targets:?}"
    );
}

/// A claim that hits `!can_send` with NO relay hints still parks. A hint-less
/// cold claim has nowhere to send, so it waits for a bootstrap relay to connect.
#[test]
fn resolve_event_ref_parked_without_uri_hint_still_parks() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("f2");
    let uri = nevent_uri(&id, Some(1), None);

    let _ = resolve_event_uri(&mut kernel, &uri, "view-no-hint".to_string(), false, false);

    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "a hint-less cold claim must not register an interest"
    );
    assert_eq!(
        kernel.pending_event_claims_len_for_test(),
        1,
        "a hint-less cold claim must be parked in pending_event_claims"
    );
}
