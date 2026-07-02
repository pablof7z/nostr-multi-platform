//! T82 integration tests — the discovery seam end-to-end through the kernel.
//!
//! Exercises `collect_unknown_refs` (ingest seam) → `drain_unknown_oneshots`
//! (registry registration + planner trigger) → `drain_lifecycle_tick` (planner
//! wire-frame emission) → `register_planner_wire_frames` (PD-033-C bridge:
//! moves the pending discovery oneshot into `oneshot_subs` keyed by the
//! planner-assigned sub_id) → `complete_unknown_oneshot` (EOSE release),
//! including the load-bearing acceptance criterion: a quoted-note's missing id
//! is discovered and resolvable via a oneshot. See [`registry_lifecycle`].
//!
//! PD-033-C Stage 1 rewrite: `drain_unknown_oneshots` no longer emits M1
//! `OutboundMessage` REQs directly. The canonical wire-frame emission flows
//! through the planner's `drain_tick`. The kernel `oneshot_subs` map is keyed
//! on the **planner-assigned `sub_id`** (`sub-<hash>`, not
//! `oneshot-disc-<token>`); the bridge in `register_planner_wire_frames`
//! translates `WireFrame::Req.interest_id` back into the `OneshotToken` so
//! EOSE / store-gate routing keys on the actual wire sub-id. [`m1_retirement_gate`]
//! pins the negative-existence side of that migration.
//!
//! [`store_gate_and_pump`] exercises the store-admission and
//! `pending_view_requests` pump paths. [`content_mention_discovery`] covers
//! V-56 content-only `nostr:npub1…` mention discovery.
//!
//! Tests that need the wire-frame side install bootstrap content + indexer
//! relays directly on the lifecycle (the planner-extension PR #365 lanes that
//! production wires from `bootstrap_urls_for_role`).

mod support;

mod registry_lifecycle;
mod store_gate_and_pump;
mod m1_retirement_gate;
mod content_mention_discovery;
