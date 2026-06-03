//! T82 integration tests — the discovery seam end-to-end through the kernel.
//!
//! Exercises `collect_unknown_refs` (ingest seam) → `drain_unknown_oneshots`
//! (registry registration + planner trigger) → `drain_lifecycle_tick` (planner
//! wire-frame emission) → `register_planner_wire_frames` (PD-033-C bridge:
//! moves the pending discovery oneshot into `oneshot_subs` keyed by the
//! planner-assigned sub_id) → `complete_unknown_oneshot` (EOSE release),
//! including the load-bearing acceptance criterion: a quoted-note's missing id
//! is discovered and resolvable via a oneshot.
//!
//! PD-033-C Stage 1 rewrite: `drain_unknown_oneshots` no longer emits M1
//! `OutboundMessage` REQs directly. The canonical wire-frame emission flows
//! through the planner's `drain_tick`. The kernel `oneshot_subs` map is keyed
//! on the **planner-assigned `sub_id`** (`sub-<hash>`, not
//! `oneshot-disc-<token>`); the bridge in `register_planner_wire_frames`
//! translates `WireFrame::Req.interest_id` back into the `OneshotToken` so
//! EOSE / store-gate routing keys on the actual wire sub-id.
//!
//! Tests that need the wire-frame side install bootstrap content + indexer
//! relays directly on the lifecycle (the planner-extension PR #365 lanes that
//! production wires from `bootstrap_urls_for_role`).

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const QUOTED_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const MENTIONED_PK: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const KNOWN_ID: &str = "3333333333333333333333333333333333333333333333333333333333333333";

const BOOTSTRAP_CONTENT: &str = "wss://bootstrap-content.test/";
const BOOTSTRAP_INDEXER: &str = "wss://bootstrap-indexer.test/";

fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Configure the lifecycle's planner-extension bootstrap relay lanes (PD-033-C
/// PR #365) so the planner has somewhere to land kernel-driven discovery
/// oneshots. Production wires these from `bootstrap_urls_for_role` in
/// `identity_state::set_configured_relays`; tests that construct
/// a bare `Kernel::new` install them directly.
///
/// Also clears the `cfg(test)` default `wss://purplepag.es` indexer relay so
/// assertions can pin discovery REQs to BOOTSTRAP_CONTENT / BOOTSTRAP_INDEXER
/// rather than collapsing onto the default indexer fallback path.
fn install_bootstrap_relays(kernel: &mut Kernel) {
    let lifecycle = kernel.lifecycle_mut();
    lifecycle.set_indexer_relays(vec![]);
    lifecycle.set_bootstrap_content_relays(vec![BOOTSTRAP_CONTENT.to_string()]);
    lifecycle.set_bootstrap_indexer_relays(vec![BOOTSTRAP_INDEXER.to_string()]);
}

/// Compile-and-register: run the planner's `drain_tick`, then push the
/// emitted frames through the kernel's `register_planner_wire_frames` bridge
/// so `oneshot_subs` is populated under the planner-assigned `sub_id`. This
/// mirrors the production actor's `drain_lifecycle_tick` →
/// `wire_frames_to_outbound` pipeline (`actor/mod.rs:1346-1357` +
/// `actor/outbound.rs`).
fn drain_and_register(kernel: &mut Kernel) -> Vec<WireFrame> {
    let frames = kernel.drain_lifecycle_tick();
    kernel.register_wire_frames_for_test(&frames);
    frames
}

/// Collect every `WireFrame::Req` filter string emitted on this tick.
fn planner_req_filters(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn quoted_note_missing_id_is_discovered_and_resolvable_via_oneshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // A kind:1 note quoting an event we do not have, plus a p-tag mention of
    // an unknown pubkey. This is the ingest seam input (borrowed visitor).
    let tags = vec![tag(&["q", QUOTED_ID]), tag(&["p", MENTIONED_PK])];
    kernel.collect_unknown_refs(&tags);

    // Drain → two oneshot interests registered (events + profiles arms). The
    // function no longer emits M1 OutboundMessage REQs (PD-033-C Stage 1).
    let drained = kernel.drain_unknown_oneshots();
    assert!(
        drained.is_empty(),
        "PD-033-C Stage 1: drain_unknown_oneshots must emit NO M1 \
         OutboundMessage frames; got {drained:?}"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        2,
        "one oneshot per missing reference must be registered in the registry"
    );

    // Planner side: drain_lifecycle_tick compiles the two interests into
    // WireFrame::Req frames addressed at the cold-start bootstrap relays;
    // register_planner_wire_frames bridges the planner sub_id back to the
    // OneshotToken in `oneshot_subs`.
    let frames = drain_and_register(&mut kernel);
    let filters = planner_req_filters(&frames);
    let joined_filters = filters.join("\n");
    assert!(
        joined_filters.contains(QUOTED_ID),
        "planner must emit a REQ whose filter carries the quoted-note id; \
         got filters: {filters:?}"
    );
    assert!(
        joined_filters.contains(MENTIONED_PK) && joined_filters.contains("\"kinds\""),
        "planner must emit a REQ whose filter carries the mentioned pubkey \
         under a kind-restricted (kind:0/3/10002) profile fetch; got filters: \
         {filters:?}"
    );

    // The bridge populated `oneshot_subs` keyed by the planner sub_ids so
    // every registered discovery oneshot is recognisable to the EOSE handler.
    let oneshot_sub_ids: Vec<String> = kernel.oneshot_subs.keys().cloned().collect();
    assert_eq!(
        oneshot_sub_ids.len(),
        2,
        "bridge must register both planner sub_ids in oneshot_subs"
    );
    for sub_id in &oneshot_sub_ids {
        assert!(
            kernel.is_discovery_oneshot(sub_id),
            "every bridged oneshot_subs entry must be recognised as a discovery oneshot"
        );
    }

    // Resolve: EOSE on each oneshot sub completes + releases its token.
    for sub_id in &oneshot_sub_ids {
        kernel.complete_unknown_oneshot(sub_id);
    }
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "all oneshots released after EOSE — no lingering subscription"
    );
}

#[test]
fn known_references_do_not_spawn_oneshots_d8_fast_path() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed the in-memory projection so the reference is "known".
    kernel.events.insert(
        KNOWN_ID.to_string(),
        StoredEvent {
            id: KNOWN_ID.to_string(),
            author: "a".repeat(64),
            kind: 1,
            created_at: 0,
            tags: Vec::new(),
            content: String::new(),
            relay_count: 1,
        },
    );
    kernel.collect_unknown_refs(&[tag(&["e", KNOWN_ID])]);
    let drained = kernel.drain_unknown_oneshots();
    assert!(
        drained.is_empty(),
        "known id is not re-fetched (M1 path retired anyway)"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "known references must not register any oneshot in the registry"
    );
}

#[test]
fn drain_is_idempotent_at_kernel_level() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    // First drain registers a oneshot in the registry; second drain with no
    // new refs is a no-op (registry already at steady state).
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "first drain registers exactly one discovery oneshot"
    );
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "second drain with no new refs must not register another oneshot"
    );
}

#[test]
fn duplicate_references_across_events_dedup_before_fetch() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Same quoted id referenced by two separate ingested events.
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    kernel.collect_unknown_refs(&[tag(&["e", QUOTED_ID])]);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "the duplicate id must dedupe into a single registered oneshot"
    );
}

#[test]
fn discovered_event_on_oneshot_sub_passes_the_store_gate() {
    // Regression: without discovery oneshot recognition in `should_store_event`,
    // a resolved quoted-note arriving on its oneshot sub would be dropped
    // (author isn't a timeline author), the cache would stay missing, and the
    // next ingest would re-discover + re-fetch the same id forever.
    //
    // T104: routing is now via `is_discovery_oneshot` (HashMap lookup on the
    // typed OneshotKind), not via `starts_with(ONESHOT_SUB_PREFIX)`. After
    // PD-033-C Stage 1 the key is the planner-assigned `sub_id` (`sub-<hash>`,
    // populated by `register_planner_wire_frames`'s bridge), not the legacy
    // `oneshot-disc-<token>` kernel label. We exercise the full path: drain
    // → planner tick → bridge → store-gate.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(kernel.discovery_in_flight(), 1);
    // Compile + register: the bridge moves the pending discovery oneshot into
    // `oneshot_subs` keyed by the planner-assigned `sub_id`.
    let frames = drain_and_register(&mut kernel);
    assert!(
        frames.iter().any(|f| matches!(f, WireFrame::Req { .. })),
        "planner must emit a REQ for the registered discovery interest; \
         got frames: {frames:?}"
    );
    let oneshot_sub = kernel
        .oneshot_subs
        .keys()
        .next()
        .cloned()
        .expect("bridge must register the planner sub_id in oneshot_subs");

    let quoted = NostrEvent {
        id: QUOTED_ID.to_string(),
        pubkey: "f".repeat(64), // NOT a timeline author
        created_at: 1,
        kind: 1,
        tags: Vec::new(),
        content: "the quoted note".to_string(),
        sig: String::new(),
    };
    assert!(
        kernel.should_store_event(&oneshot_sub, &quoted),
        "a discovered event on its bridged planner sub_id must be storable"
    );
    // ADR-0042 §5.1: store admission is now SHAPE-based, not sub-id-keyed. The
    // discovery oneshot registers an interest with `event_ids = {QUOTED_ID}` in
    // the registry, so the quoted event (id == QUOTED_ID) is storable on ANY
    // sub_id — `matches_active_open_interest` admits it by content. The old
    // assertion that an unrelated sub_id gates it out encoded the pre-M2
    // sub-id-exclusive admission model and is obsolete: an event matching an
    // active registered interest is storable regardless of which wire sub
    // delivered it (the wire sub is a merged compiler hash, not a per-interest
    // key). A truly unmatched event is still dropped — see
    // `should_store_event` returning false below for an id no interest names.
    let unmatched = NostrEvent {
        id: "a".repeat(64), // no active interest names this id/author
        pubkey: "b".repeat(64),
        created_at: 1,
        kind: 1,
        tags: Vec::new(),
        content: "unrelated".to_string(),
        sig: String::new(),
    };
    assert!(
        !kernel.should_store_event("some-other-sub", &unmatched),
        "an event matching NO active interest is still gated out"
    );
}

#[test]
fn ingest_then_drain_resolves_through_pending_view_requests() {
    // End-to-end through the kernel's own request pump: collect during ingest,
    // then `pending_view_requests` drains the unknown set into the registry,
    // and `drain_lifecycle_tick` compiles the registered interest into a
    // planner-emitted REQ on the bootstrap content relay.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    // PD-033-C Stage 1: `pending_view_requests` no longer carries the
    // discovery REQ in its M1 OutboundMessage list — that emission moved to
    // the planner. The call still registers the oneshot via the registry +
    // enqueues the planner trigger.
    let pumped = kernel.pending_view_requests();
    assert!(
        pumped.is_empty(),
        "PD-033-C Stage 1: pending_view_requests must emit NO M1 OutboundMessage \
         frames for the discovery seam; got {pumped:?}"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "pending_view_requests must still register the discovery interest \
         via the M2 registry"
    );

    // Planner now owns the wire-frame emission. The compiled REQ lands on the
    // bootstrap content relay (planner-extension PR #365 Case D head check).
    let frames = drain_and_register(&mut kernel);
    assert!(
        frames.iter().any(|f| matches!(
            f,
            WireFrame::Req { relay_url, filter_json, .. }
                if relay_url == BOOTSTRAP_CONTENT
                    && filter_json.contains(QUOTED_ID)
        )),
        "planner drain_tick must emit a discovery REQ on the bootstrap \
         content relay carrying the quoted-note id; got frames: {frames:?}"
    );
    // Bridge confirmation: the planner sub_id is now in oneshot_subs.
    assert_eq!(
        kernel.oneshot_subs.len(),
        1,
        "bridge must register the planner sub_id in oneshot_subs"
    );
}

#[test]
fn completing_unknown_oneshot_for_non_discovery_sub_is_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Must not panic / must not touch in-flight state (D6).
    kernel.complete_unknown_oneshot("seed-timeline");
    assert_eq!(kernel.discovery_in_flight(), 0);
}

#[test]
fn many_unknown_ids_collapse_to_few_batch_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // 120 event ids -> ceil(120\/50) = 3 content REQs would be ideal, but the
    // concurrency cap (MAX_DISCOVERY_CONCURRENCY = 2) throttles us to 1 events
    // arm + 1 profiles arm per drain. The remaining 95 stay queued.
    // 75 pubkeys    -> ceil(75\/50)  = 2 indexer REQs (also throttled).
    let tags: Vec<Vec<String>> = (0u32..120)
        .map(|i| tag(&["e", &format!("{i:0>64x}")]))
        .chain((0u32..75).map(|i| tag(&["p", &format!("{i:0>64x}")])))
        .collect();
    kernel.collect_unknown_refs(&tags);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        2,
        "throttled: 1 events arm + 1 profiles arm registered as oneshots; \
         95 remain queued for the next drain"
    );
}

#[test]
fn oneshot_kind_typed_routing_replaces_string_prefix_matching() {
    // T104 acceptance criterion: `is_discovery_oneshot` returns true only for
    // sub-ids registered in `oneshot_subs` with `OneshotKind::Discovery`.
    // An unregistered sub_id returns false (HashMap lookup, not prefix scan).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // Before any drain, no sub is registered.
    let fake = "sub-deadbeef".to_string();
    assert!(
        !kernel.is_discovery_oneshot(&fake),
        "unregistered sub_id is not a discovery oneshot (HashMap lookup, not prefix scan)"
    );

    // Drain + planner tick + bridge — the canonical sub-id source is the
    // planner's `sub-<hash>` registered in `oneshot_subs` via the bridge.
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(kernel.discovery_in_flight(), 1);
    let _ = drain_and_register(&mut kernel);
    let registered_sub = kernel
        .oneshot_subs
        .keys()
        .next()
        .cloned()
        .expect("bridge must register the planner sub_id in oneshot_subs");
    assert!(
        kernel.is_discovery_oneshot(&registered_sub),
        "registered discovery oneshot is recognised by OneshotKind::Discovery lookup"
    );

    // After EOSE completes and releases the token, the sub is deregistered.
    kernel.complete_unknown_oneshot(&registered_sub);
    assert!(
        !kernel.is_discovery_oneshot(&registered_sub),
        "completed oneshot is removed from oneshot_subs — no longer recognised"
    );
}

// ─── PD-033-C Stage 1 retirement gate ────────────────────────────────────────

/// PD-033-C Stage 1 retirement assertion: the discovery seam must NEVER emit
/// an `oneshot-disc-*` REQ frame via the M1 outbound path (`Kernel::req` →
/// `OutboundMessage`). The canonical emission flows exclusively through the
/// planner's `drain_tick` → `WireFrame::Req`, and the planner uses its own
/// `sub-<hash>` sub_id format (`subs/wire.rs::sub_id_for`).
///
/// Mirrors the shape of `live_follow_feed_path_emits_no_seed_timeline_req` in
/// `t140_m1_retirement_tests.rs` — a negative-existence gate that proves the
/// dual-write deletion stayed deleted (no silent regression to the M1 helper).
#[test]
fn discovery_seam_emits_no_m1_oneshot_disc_outbound_req() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // Mix of unknown event-id + pubkey to exercise BOTH arms of
    // `drain_unknown_oneshots` (the two former call sites of `self.req(...)`).
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID]), tag(&["p", MENTIONED_PK])]);

    // M1 emission paths: `drain_unknown_oneshots` and the
    // `pending_view_requests` pump that calls it.
    let m1_from_drain = kernel.drain_unknown_oneshots();
    // After the drain, the unknown_ids set is empty, so pending_view_requests
    // is observed against the registered (but already drained) state.
    let m1_from_pump = kernel.pending_view_requests();

    let m1_outbound_texts: Vec<&str> = m1_from_drain
        .iter()
        .chain(m1_from_pump.iter())
        .map(|m| m.text.as_str())
        .collect();
    // V-04 Stage 4 / PD-033-C: `ONESHOT_SUB_PREFIX` was deleted alongside
    // `Kernel::req`; the literal `"oneshot-disc-"` is inlined here as the
    // retirement-gate marker. Any outbound text carrying that prefix would
    // indicate a regression to the M1 `oneshot-disc-<token>` sub-id format.
    let leaked: Vec<&&str> = m1_outbound_texts
        .iter()
        .filter(|t| {
            t.contains("oneshot-disc-") || t.contains(QUOTED_ID) || t.contains(MENTIONED_PK)
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "PD-033-C Stage 1 RETIREMENT: the discovery seam must emit ZERO M1 \
         outbound REQs for the discovery oneshot arms (no `oneshot-disc-` \
         prefix, no quoted-note id, no mentioned pubkey leaking through the \
         legacy OutboundMessage path). Leaked: {leaked:?}"
    );

    // Positive parity: the planner must carry the discovery REQs instead.
    let m2_frames = kernel.drain_lifecycle_tick();
    let m2_req_filters = planner_req_filters(&m2_frames);
    let m2_joined = m2_req_filters.join("\n");
    assert!(
        m2_joined.contains(QUOTED_ID),
        "with M1 retired, drain_lifecycle_tick must carry the events-arm \
         discovery REQ; got filters: {m2_req_filters:?}"
    );
    assert!(
        m2_joined.contains(MENTIONED_PK),
        "with M1 retired, drain_lifecycle_tick must carry the profiles-arm \
         discovery REQ; got filters: {m2_req_filters:?}"
    );
}

// ─── V-56: content-level profile mention discovery ───────────────────────────

/// V-56 acceptance criterion: a `nostr:npub1…` mention that appears only in
/// note content (no matching `p`-tag) must reach `UnknownIds` and subsequently
/// be emitted as a profiles-arm discovery REQ — so a kind:0 fetch fires and
/// the profile renders without waiting forever.
///
/// Driven through `ingest_timeline_event` (the production hot path) so this
/// test proves that the production ingest path connects content-extracted
/// pubkeys to `UnknownIds` — not just the helper in isolation.
///
/// Uses real Schnorr-signed events (same pattern as `provenance_wire_tests`)
/// so `VerifiedEvent::try_from_raw` passes and discovery code is reached.
/// Uses `diag-firehose-stress` sub_id to bypass the `timeline_authors` gate.
#[test]
fn v56_content_only_npub_mention_feeds_profile_discovery() {
    use crate::nip19::encode_npub;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // A pubkey that is NOT in `profiles` and NOT in any `p`-tag.
    let content_only_pk = MENTIONED_PK;
    let npub = encode_npub(content_only_pk).expect("encode_npub must succeed for a 64-hex string");
    let content = format!("Check out nostr:{npub} — great follow");

    // Real Schnorr-signed event: `VerifiedEvent::try_from_raw` verifies the
    // signature before the event can proceed to discovery. The tags list has
    // NO p-tag for MENTIONED_PK — that's the V-56 regression scenario.
    let keys = ::nostr::Keys::generate();
    let nostr_event = ::nostr::EventBuilder::text_note(content)
        .custom_created_at(::nostr::Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let note = NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: Vec::new(), // deliberately NO p-tag for MENTIONED_PK
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    };

    // Nothing in UnknownIds before ingest.
    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "precondition: unknown_ids must be empty before ingest"
    );

    // Ingest through the production path (diag-firehose bypasses author gate).
    kernel.ingest_timeline_event(
        crate::relay::RelayRole::Content,
        "wss://test.relay/",
        "diag-firehose-stress",
        note,
    );

    // The content-extracted pubkey must have landed in unknown_ids.
    assert!(
        kernel.unknown_ids.pending_len() > 0,
        "V-56: content-only nostr:npub1 mention must add pubkey to UnknownIds \
         pending set after ingest"
    );

    // Drain + planner tick must produce a profiles-arm REQ carrying the pubkey.
    let _ = kernel.drain_unknown_oneshots();
    let frames = drain_and_register(&mut kernel);
    let filters = planner_req_filters(&frames);
    let joined = filters.join("\n");
    assert!(
        joined.contains(content_only_pk),
        "V-56: planner must emit a profiles-arm REQ carrying the content-only \
         mention pubkey; got filters: {filters:?}"
    );
}

/// V-56 fast-path: content with no `nostr:` substring must not record anything
/// in UnknownIds (D8 — zero allocation on the common path).
#[test]
fn v56_no_nostr_uri_in_content_skips_fast_path() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_content_mention_pubkeys("hello world, no mentions here");
    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "V-56 D8 fast-path: content without nostr: must leave UnknownIds empty"
    );
}

/// V-56 dedup: a pubkey that is both in a `p`-tag (already fed via
/// `collect_unknown_refs`) and in content must not double-count in UnknownIds.
#[test]
fn v56_content_mention_dedups_with_p_tag() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    use crate::nip19::encode_npub;

    let pk = MENTIONED_PK;
    let npub = encode_npub(pk).expect("encode_npub must succeed");

    // First, record via the p-tag path.
    kernel.collect_unknown_refs(&[tag(&["p", pk])]);
    let len_after_ptag = kernel.unknown_ids.pending_len();

    // Then scan content that mentions the same pubkey.
    let content = format!("nostr:{npub}");
    kernel.collect_content_mention_pubkeys(&content);

    assert_eq!(
        kernel.unknown_ids.pending_len(),
        len_after_ptag,
        "V-56: content mention of a pubkey already pending via p-tag must not \
         increase the UnknownIds pending count (dedup)"
    );
}

/// V-56 known profile: a pubkey already in the `profiles` projection must not
/// be re-added (D8 dedup gate in `note_pubkey`).
#[test]
fn v56_known_profile_not_re_added() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    use crate::nip19::encode_npub;

    let pk = MENTIONED_PK;
    let npub = encode_npub(pk).expect("encode_npub must succeed");

    // Pre-seed the profiles projection so the pubkey is "known".
    kernel
        .profiles
        .insert(pk.to_string(), super::types::Profile::default());

    let content = format!("Say hello to nostr:{npub}");
    kernel.collect_content_mention_pubkeys(&content);

    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "V-56: a pubkey already in profiles must not be added to UnknownIds \
         (D8 dedup guard in note_pubkey)"
    );
}
