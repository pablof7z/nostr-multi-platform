//! ADR-0045 E2+E3 — **Universal acceptance test** (closes v1-blocker #1086).
//!
//! Requirement (owner-decided, issue #1086): populate a store with feed events
//! + a DM gift-wrap + a thread reply + a long-form article; fresh kernel, zero
//! relay connectivity; open the standard interests; assert feed, DM (IngestParser),
//! thread, and long-form projections ALL render from the store.
//!
//! This is the v1 exit criterion for ADR-0045 §8: one test that falsifies the
//! complete seam. If any engineering increment (E1 feed, E2 DM, E3 thread /
//! long-form) is broken, this test fails. See [`universal_acceptance_tests`].
//!
//! ## Structure
//!
//! - **Phase 1 (seed)**: events are ingested through the live ingest path
//!   (`handle_event` — Schnorr-verify + store + observer fan-out) to populate
//!   the persistent store exactly as production does.
//!
//! - **Phase 2 (cold restart)**: the in-memory caches (`events`, `timeline`)
//!   are cleared — simulating a process restart that discards all in-memory state.
//!
//! - **Phase 3 (serve)**: cache-serve interests are enqueued for each shape
//!   and drained under the aggregate budget. Each interest is opened without
//!   any relay connection.
//!
//! - **Phase 4 (assert)**: every projection path is asserted non-empty.
//!
//! ## Why IngestParser, not DmInboxProjection directly
//!
//! `DmInboxProjection` lives in `nmp-nip17`, which depends on `nmp-core`,
//! creating a circular compile dependency if we imported it here. This test
//! instead verifies the seam that `DmInboxProjection` and `MarmotIngestParser`
//! both ride:
//!
//! - **IngestParser** (`ingest_dispatcher.dispatch()`): all former raw-tap
//!   consumers (NIP-17 DM inbox since PR-1, Marmot since PR-2) now ride this
//!   seam exclusively. `CapturingIngestParser` stands in here to avoid the
//!   circular dep on `nmp-nip17`.
//!
//! The decrypt path itself is exercised by
//! `nmp-nip17::inbox::tests::received_dm_surfaces_in_the_conversation`.
//! See [`e2_ingest_parser_seam_tests`] for the narrower PR-2 regression.

mod universal_fixtures_support;

mod universal_acceptance_tests;
mod e2_ingest_parser_seam_tests;
