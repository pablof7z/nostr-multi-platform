//! S7 event builders + injectors (ADR-0055 R6-S4 feed-idle capstone).
//!
//! Extracted from `s7_feed_idle.rs` per repo file-size doctrine (500-LOC hard
//! ceiling, split into cohesive submodules — same precedent as R3-S5's
//! s6_gates/s6_oracle split). Owns the test-support event construction and the
//! actor-channel injection helpers; `s7_feed_idle.rs` owns the measurement
//! driver.
//!
//! D0: all injection goes through `actor_sender()` +
//! `ActorCommand::IngestPreVerifiedEvents` (the test-support path, cfg-gated in
//! `nmp-core`); `VerifiedEvent::from_raw_unchecked` is the test-support bypass.

use nmp_store::{RawEvent, VerifiedEvent};
use nmp_core::actor::ActorCommand;
use nmp_core::actor::{TestSupportCommand};
use nmp_native_runtime::NmpApp;

/// 64-hex viewer pubkey (the active account; self-inclusion makes it a "follow"
/// so its root events qualify for the feed).
pub(crate) const VIEWER_PUBKEY: &str =
    "aaaa000000000000000000000000000000000000000000000000000000000001";

/// 64-hex stranger pubkey (NOT followed; its REPLIES are dropped by the engine).
pub(crate) const STRANGER_PUBKEY: &str =
    "bbbb000000000000000000000000000000000000000000000000000000000002";

// ── Event builders ────────────────────────────────────────────────────────────

/// Build a kind:1 ROOT event with a globally-unique 64-hex id from `id_seed`.
///
/// `id_seed` must be unique across all events the harness injects so the kernel
/// store does not dedupe them; the seed/probe windows use disjoint id namespaces.
fn make_event(pubkey: &str, created_at: u64, id_seed: u64) -> VerifiedEvent {
    let id = format!("{:0>16x}{:0>48x}", 0u64, id_seed);
    let raw = RawEvent {
        id: id[..64].to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: Vec::new(),
        content: format!("feed harness event {id_seed} from {}", &pubkey[..8]),
        sig: "0".repeat(128),
    };
    VerifiedEvent::from_raw_unchecked(raw)
}

/// Build a kind:1 REPLY (NIP-10 `e`-tagged root+reply) to `root_id` from `pubkey`.
fn make_reply(pubkey: &str, root_id: &str, created_at: u64, id_seed: u64) -> VerifiedEvent {
    let id = format!("{:0>16x}{:0>48x}", 0u64, id_seed);
    let raw = RawEvent {
        id: id[..64].to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "root".to_string(),
            ],
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "reply".to_string(),
            ],
        ],
        content: format!("reply id={id_seed} from {}", &pubkey[..8]),
        sig: "0".repeat(128),
    };
    VerifiedEvent::from_raw_unchecked(raw)
}

/// A 64-hex root id the harness NEVER injects (disjoint id namespace ≥ 9M).
fn unknown_root_id(seed: u64) -> String {
    format!("{:0>16x}{:0>48x}", 0u64, 9_000_000u64 + seed)[..64].to_string()
}

// ── Injectors ────────────────────────────────────────────────────────────────

/// Send a batch of pre-verified events through the actor channel.
///
/// SAFETY: `app` must be a valid non-null pointer from `nmp_app_new`.
fn send_events(app: *mut NmpApp, events: Vec<VerifiedEvent>) {
    let app_ref = unsafe { &*app };
    app_ref
        .actor_sender()
        .send(ActorCommand::TestSupport(TestSupportCommand::IngestPreVerifiedEvents(events)))
        .ok();
}

/// Inject `count` ROOT events from `pubkey`, ids derived from `id_base + i`,
/// timestamps `base_ts + i` (monotonically increasing → newest = highest i).
///
/// SAFETY: `app` must be a valid non-null pointer from `nmp_app_new`.
pub(crate) fn inject_events_from(
    app: *mut NmpApp,
    pubkey: &str,
    base_ts: u64,
    id_base: u64,
    count: u32,
) {
    let events: Vec<VerifiedEvent> = (0..count as u64)
        .map(|i| make_event(pubkey, base_ts + i, id_base + i))
        .collect();
    send_events(app, events);
}

/// Inject ONE FOLLOWED author's reply to a root the engine never holds.
/// **This is the real over-invalidation probe (review BLOCKER fix).**
///
/// **Why a reply-to-unknown-root, NOT an out-of-window new root:** the
/// OpFeedEngine's `RootFeedSnapshot` carries `page.total_blocks` (a count of ALL
/// roots, not just the visible 80) and `page.has_more`. A NEW root — even one
/// with an OLD `created_at` below the visible window — increments `total_blocks`
/// and can flip `has_more`, so the serialized snapshot legitimately CHANGES and
/// the feed SHOULD re-emit (verified empirically: +160 bytes). That is correct,
/// not a false resend. (Also: the OP-centric engine surfaces ALL roots regardless
/// of author — `ingest_root` is NOT follow-gated; only REPLIES are.)
///
/// A FOLLOWED author's reply to a root the engine does NOT hold is parked in
/// `pending_attributions`: it passes `follow_set.predicate()`, reaches the engine
/// (store outcome Inserted → observer fires), and MUTATES internal state (the
/// pending-attribution map grows) — but surfaces NO card and does NOT touch the
/// `roots` map that drives `total_blocks`. The serialized snapshot is therefore
/// byte-identical → the byte-equality gate MUST omit it. This is the genuine
/// "engine touched, rendered output unchanged" case that exercises the gate as
/// the suppressor.
///
/// SAFETY: `app` must be a valid non-null pointer from `nmp_app_new`.
pub(crate) fn inject_followed_reply_to_unknown_root(
    app: *mut NmpApp,
    created_at: u64,
    id_seed: u64,
) {
    let ev = make_reply(VIEWER_PUBKEY, &unknown_root_id(id_seed), created_at, id_seed);
    send_events(app, vec![ev]);
}

/// Inject `count` STRANGER (non-followed) REPLIES — the secondary predicate
/// sanity check.
///
/// Reply-shaped events from a non-followed author are DROPPED by the engine
/// (`ingest.rs`: "Non-follow replies are dropped (no state change)") before any
/// state mutation → feed trivially byte-identical. This proves the follow
/// predicate filters, NOT that the byte-equality gate suppresses (it would pass
/// even with a broken gate). NB: stranger ROOTS would NOT work — the OP-centric
/// engine surfaces all roots regardless of author, so a stranger root would
/// (correctly) change the feed.
///
/// SAFETY: `app` must be a valid non-null pointer from `nmp_app_new`.
pub(crate) fn inject_stranger_replies(
    app: *mut NmpApp,
    created_at: u64,
    id_base: u64,
    count: u32,
) {
    let events: Vec<VerifiedEvent> = (0..count as u64)
        .map(|i| make_reply(STRANGER_PUBKEY, &unknown_root_id(50_000 + i), created_at + i, id_base + i))
        .collect();
    send_events(app, events);
}
