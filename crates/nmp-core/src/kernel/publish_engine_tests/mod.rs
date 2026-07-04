#![cfg(test)]
//! T117 integration tests — kernel publish path goes through `PublishEngine`.
//!
//! These tests drive the kernel's engine seam directly:
//! - The engine's `Nip65OutboxResolver` resolves relays from the kernel's
//!   event store. A kind:10002 for the author is seeded via `seed_kind10002`
//!   so `Nip65OutboxResolver` has real NIP-65 write relays to route to.
//!   (T-publish-resolver-indexer / codex f81f735: the old indexer-fallback
//!   path is removed — an author with no kind:10002 produces `NoTargets`, not
//!   a silent publish to arbitrary public relays.)
//! - The engine pushes per-relay `EVENT` frames into the `QueueDispatcher`,
//!   which the kernel drains into `OutboundMessage`s.
//! - OK frames are folded back via `Kernel::handle_publish_ok_at` (the
//!   time-injected variant; the wire path calls `handle_publish_ok` which
//!   reads `SystemTime::now()`).
//! - Retries fire on `tick_publish_engine(now_ms)`.
//!
//! Time is injected throughout (`now_ms` deterministic), no sockets, no
//! sleeps. The four bullets the spec calls out:
//! 1. Successful multi-relay publish: engine settles each per-relay to Ok →
//!    snapshot `recent_ok` carries the relay set.
//! 2. AUTH-REQUIRED on one relay, OK on the other: the auth relay PARKS
//!    (availability gate, no retry budget) until it reaches `Authenticated`,
//!    then re-dispatches and settles; untouched relay stays Ok.
//! 3. Transient failure × 3: 1s backoff → 4s backoff → give-up;
//!    `FailedAfterRetries` row appears on the snapshot.
//! 4. Restart with a Pending row: build a second Kernel sharing the same
//!    `Arc<dyn PublishStore>`; engine resumes via `resume_publish_engine`.

mod helpers_tests;
pub(super) use helpers_tests::{
    fake_signed, now_ms_after_resume, ok_payload, persist_pending_record, seed_kind10002,
    WRITE_R1, WRITE_R2,
};

mod chokepoint_tests;
mod outbox_failed_honesty_tests;
mod t117_tests;
mod t127_tests;
mod user_actions_tests;
