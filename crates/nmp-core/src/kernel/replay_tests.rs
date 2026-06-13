//! T116 / G1 — reconnect-replay kernel-surface tests.
//!
//! These tests pin the wire-correctness invariant that `Kernel::replay_on_reconnect`
//! re-emits a fresh REQ for every active sub-shape targeting the reconnected
//! URL, that the post-T133 `wire_subs` eviction is repaired (the new REQ
//! re-registers a row), and that the T129 watermark is re-applied so the
//! relay does not re-emit events already in the store.

use std::sync::Arc;

use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn timeline_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

fn timeline_interest_with_since(id: u64, author: &str, since: u64) -> LogicalInterest {
    let mut i = timeline_interest(id, author);
    i.shape.since = Some(since);
    i
}

/// Build a kernel + mailbox + populate `current_plan` with a single
/// author/kind interest routed to one write relay. Returns the resolved
/// relay URL so tests can use it as the reconnect target.
fn kernel_with_one_sub(
    role: RelayRole,
    author: &str,
    relay: &str,
    interest_id: u64,
) -> (Kernel, String) {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: vec![relay.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(timeline_interest(interest_id, author));
    // First recompile populates `current_plan`. The frames returned here are
    // the initial REQ wave; the test deliberately discards them — we only
    // care that the plan is now resident so the subsequent reconnect can
    // replay it.
    let _ = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");
    // Simulate the actor's first dial: register a `WireSub` row for the
    // sub-shape (mirrors what `req_for_relay` does on the normal hot path).
    // We do this through the lifecycle's wire-emit path here to keep the
    // setup honest; the alternative would be to re-emit the initial REQ
    // wave through `kernel.req_for_relay`, but that requires reimplementing
    // `wire::sub_id_for` outside the kernel surface. The simpler proxy:
    // record the sub state by calling `relay_connected` then `relay_closed`
    // to exercise the eviction path the replay must repair.
    kernel.relay_connected(role);
    (kernel, relay.to_string())
}

/// Sanity: with no current plan, replay is a no-op (first connect, before
/// any view has opened).
#[test]
fn replay_on_reconnect_is_noop_without_current_plan() {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let out = kernel.replay_on_reconnect(RelayRole::Content, "wss://r1.example/");
    assert!(out.is_empty(), "no plan ⇒ no replay; got {out:?}");
}

/// G1 — after a sub-shape is in `current_plan`, a reconnect produces a
/// fresh REQ targeting the reconnected URL.
///
/// Uses the canonical URL form (no empty-path trailing slash): `req_for_relay`
/// records `WireSub.relay_url` under the canonical key (T-relay-url-normalize),
/// so the repopulated-wire-sub assertion below compares against that form.
#[test]
fn replay_on_reconnect_reissues_req_for_url() {
    let (mut kernel, relay_url) =
        kernel_with_one_sub(RelayRole::Content, "a", "wss://r1.example", 1);

    // Simulate disconnect → reconnect at the actor seam: the actor saw
    // `Closed`, the kernel evicted all wire_subs for the role at T133
    // (mirror that here by calling `relay_closed` directly — `wire_subs`
    // was already empty because `kernel_with_one_sub` only populates
    // `current_plan`, not the per-sub registry; what we are exercising is
    // the "replay populates an empty wire_subs map" path). Then the
    // worker reconnected and the actor calls `replay_on_reconnect`.
    kernel.relay_closed(RelayRole::Content, &relay_url);

    let out = kernel.replay_on_reconnect(RelayRole::Content, &relay_url);

    // Exactly one REQ frame, targeting the reconnected URL, on the
    // diagnostic lane the actor reported.
    assert!(!out.is_empty(), "replay must emit at least one REQ");
    let reqs: Vec<&OutboundMessage> = out
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect();
    assert!(
        !reqs.is_empty(),
        "every replay frame must be a REQ; got {:?}",
        out.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
    for req in &reqs {
        assert_eq!(
            req.relay_url, relay_url,
            "replay REQ must target the reconnected URL"
        );
        assert_eq!(
            req.role,
            RelayRole::Content,
            "replay REQ inherits the diagnostic lane the actor reported"
        );
        assert!(
            req.text.contains(&pubkey("a")),
            "replay REQ must carry the original filter (author hex); got {}",
            req.text
        );
        assert!(
            req.text.contains("\"kinds\":[1]"),
            "replay REQ must carry the original kinds; got {}",
            req.text
        );
    }

    // The replay must re-register `wire_subs` rows so EOSE/CLOSE on the new
    // socket can correlate (the T133 eviction otherwise leaves the kernel
    // unable to bookkeep the resumed sub).
    let active = kernel.snapshot_active_wire_subs();
    assert!(
        !active.is_empty(),
        "replay must repopulate wire_subs post-T133 eviction; got empty snapshot"
    );
    for (_sub_id, url) in &active {
        assert_eq!(
            url, &relay_url,
            "every repopulated wire-sub must point at the reconnected URL"
        );
    }
}

/// T129 watermark on replay — between the initial recompile and the
/// reconnect the store may have ingested newer events. The replay must
/// re-apply the watermark fn so `since` is bumped past already-stored
/// events; otherwise the relay re-emits everything we already have.
///
/// Owner decision #1281: only interests with an explicit `since` (`Some(t)`)
/// are eligible for the rewrite. We supply since=500 (below the watermark of
/// 1700) so the reconnect replay raises it to 1701.
#[test]
fn replay_applies_t129_watermark_to_since() {
    let role = RelayRole::Content;
    let relay = "wss://r1.example/";
    let author = "b";

    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: vec![relay.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    // Watermark at 1700. Interest has since=500 so the rewrite raises it to
    // 1701 on every replay REQ (#1281: since=None is exempt from the rewrite).
    kernel
        .lifecycle_mut()
        .set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(timeline_interest_with_since(7, author, 500));
    let _ = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");

    kernel.relay_connected(role);
    kernel.relay_closed(role, relay); // T133 eviction

    let out = kernel.replay_on_reconnect(role, relay);
    let req_text = out
        .iter()
        .find(|m| m.text.starts_with("[\"REQ\""))
        .map(|m| m.text.clone())
        .expect("replay must emit a REQ");

    assert!(
        req_text.contains("\"since\":1701"),
        "T129 watermark must be re-applied on replay; got {req_text}"
    );
}

/// Replay onto a URL the plan does not route to is a no-op. Defends against
/// false reconnect-replays for sibling relays (e.g. a different per-author
/// write relay reconnecting must NOT produce REQs for our author).
#[test]
fn replay_for_unknown_url_is_noop() {
    let (mut kernel, relay_url) =
        kernel_with_one_sub(RelayRole::Content, "a", "wss://r1.example/", 1);
    kernel.relay_closed(RelayRole::Content, &relay_url);

    let out = kernel.replay_on_reconnect(RelayRole::Content, "wss://stranger.example/");
    assert!(
        out.is_empty(),
        "replay onto a URL outside current_plan.per_relay must be a no-op; got {out:?}"
    );
}
