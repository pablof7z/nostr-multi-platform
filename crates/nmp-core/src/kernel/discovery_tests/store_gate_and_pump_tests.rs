//! Discovery seam ↔ store-admission and pump integration: a resolved
//! discovery oneshot's event must pass `should_store_event`
//! (`is_discovery_oneshot` routing, T104), and `pending_view_requests` must
//! still register the discovery interest via the M2 registry while emitting
//! no legacy M1 outbound frames.

use super::discovery_fixtures_support::{drain_and_register, install_bootstrap_relays, tag, BOOTSTRAP_CONTENT, QUOTED_ID};
use crate::kernel::Kernel;
use crate::kernel::NostrEvent;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

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
    // ADR-0076 §5.1: store admission is now SHAPE-based, not sub-id-keyed. The
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
