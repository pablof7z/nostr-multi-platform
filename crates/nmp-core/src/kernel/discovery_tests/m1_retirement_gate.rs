//! PD-033-C Stage 1 retirement gate.
//!
//! Negative-existence assertion: the discovery seam must NEVER emit an
//! `oneshot-disc-*` REQ frame via the M1 outbound path (`Kernel::req` →
//! `OutboundMessage`). The canonical emission flows exclusively through the
//! planner's `drain_tick` → `WireFrame::Req`, and the planner uses its own
//! `sub-<hash>` sub_id format (`subs/wire.rs::sub_id_for`).
//!
//! Mirrors the shape of `live_follow_feed_path_emits_no_seed_timeline_req` in
//! `t140_m1_retirement_tests.rs` — a negative-existence gate that proves the
//! dual-write deletion stayed deleted (no silent regression to the M1 helper).

use super::support::{install_bootstrap_relays, planner_req_filters, tag, MENTIONED_PK, QUOTED_ID};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

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
