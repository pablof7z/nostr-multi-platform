//! End-to-end integration tests for the full kernel pipeline.
//!
//! Every test here exercises the path:
//!   Action dispatch → subscription planner → relay manager opens REQ →
//!   MockRelay emits EVENT → ingest verifies + persists → reverse-index
//!   updates → view snapshot reflects → app sees rev: u64 bump.
//!
//! # Milestone gate legend
//!
//! Each test carries `#[ignore = "blocked on M<N>: <label>"]`.  The
//! companion `e2e_full_pipeline_audit.rs` fails at CI time when any such
//! tag is present but the referenced milestone is recorded as DONE in
//! GitHub Issues.  The issue queue owns milestone status; the audit enforces
//! un-ignoring.
//!
//! Gate map for this suite:
//!   M2 — subscription compilation + outbox routing + kind:3 auto-tracking
//!   M3 — persistence (LMDB) + full insert invariants
//!   M4 — NIP-77 negentropy sync engine
//!   M5 — NIP-42 relay auth
//!   M6 — sessions + signers (incl. bunker + nsec creation) + write path
//!   M7 — reactions + thread + reply
//!   M8 — relay manager + multi-relay subscription lifecycle
//!
//! Tests 1, 2, 4, 6 gate on M2 + M3 + M8.
//! Test 3 gates on M6 + M7 + M8.
//! Test 5 gates on M5 + M6 + M8.
//!
//! # Layout
//!
//! Each pipeline stage / scenario lives in its own submodule under
//! `e2e_full_pipeline/`, split out of a single oversized file (#962):
//!   - `support` — shared padded-pubkey / mailbox-cache / wire-frame fixtures.
//!   - `profile_pipeline` — Test 1: cold-open profile publish → snapshot.
//!   - `subscription_rewiring` — Test 2: kind:3 follow-list rewires REQs.
//!   - `publish_outbox` — Test 3: PublishEngine → outbox → dispatcher roundtrip.
//!   - `negentropy_watermark` — Test 4: watermark-driven `since` rewrite.
//!   - `auth_gate` — Test 5: NIP-42 auth-pause / flush of pending REQs.
//!   - `concurrent_dispatch` — Test 6: monotonic rev under concurrent submit.
//!   - `signer_pubkey_selector` — Tests 7-8: PublishRaw signer_pubkey selector.

// These constants are used by the audit companion to verify tag format.
pub const GATE_M2: &str = "M2";
pub const GATE_M3: &str = "M3";
pub const GATE_M4: &str = "M4";
pub const GATE_M5: &str = "M5";
pub const GATE_M6: &str = "M6";
pub const GATE_M7: &str = "M7";
pub const GATE_M8: &str = "M8";

mod e2e_profile_actor;

#[path = "e2e_full_pipeline/support.rs"]
mod support;

#[path = "e2e_full_pipeline/profile_pipeline.rs"]
mod profile_pipeline;

#[path = "e2e_full_pipeline/subscription_rewiring.rs"]
mod subscription_rewiring;

#[path = "e2e_full_pipeline/publish_outbox.rs"]
mod publish_outbox;

#[path = "e2e_full_pipeline/negentropy_watermark.rs"]
mod negentropy_watermark;

#[path = "e2e_full_pipeline/auth_gate.rs"]
mod auth_gate;

#[path = "e2e_full_pipeline/concurrent_dispatch.rs"]
mod concurrent_dispatch;

#[path = "e2e_full_pipeline/signer_pubkey_selector.rs"]
mod signer_pubkey_selector;
