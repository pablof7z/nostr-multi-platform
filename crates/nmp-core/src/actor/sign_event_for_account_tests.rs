//! ADR-0071 Decision 2 — `ActorCommand::SignEventForAccount` dispatch-arm +
//! idle-loop drain tests for BOTH signing backends.
//!
//! These prove the worker code path is identical for a local nsec and a
//! NIP-46 bunker:
//!
//! 1. **Local (inline)** — a local-key account resolves `SignerOp::Ready` on
//!    the spot, so the dispatch arm invokes the continuation INLINE on the
//!    actor thread with a valid `SignedEvent` (id + sig + pubkey verified). No
//!    op is parked. See [`local_backend_tests`].
//! 2. **Mock bunker (parked → resolved)** — a remote signer returns
//!    `SignerOp::Pending`; the dispatch arm parks a `ParkedOp` with the
//!    `SignContinuation` sink. The continuation has NOT run yet. After the broker
//!    turns the request around, the idle-loop drain
//!    (`resolve_parked_op`) invokes the SAME continuation with the
//!    `SignedEvent`. See [`bunker_backend_tests`].
//! 3. **Mock bunker error** — a broker rejection / dropped channel resolves the
//!    continuation with `Err(_)` so the worker's failure path runs (D6 — no
//!    stuck spinner). See [`bunker_backend_tests`].
//!
//! The continuation in all three is a worker-supplied closure that records the
//! outcome through a shared `Arc<Mutex<..>>` (the Blossom-worker shape, no HTTP).
//!
//! [`no_active_account_tests`] covers the no-account Err path; [`budget_regression_tests`]
//! pins the §D4 named-roster-key budget regression.

mod signer_fixtures_support;

mod budget_regression_tests;
mod bunker_backend_tests;
mod local_backend_tests;
mod no_active_account_tests;
