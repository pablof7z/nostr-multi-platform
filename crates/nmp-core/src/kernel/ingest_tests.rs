//! Unit tests for the kernel ingest handler `ingest_contacts` (kind:3) in
//! `kernel/ingest/`.
//!
//! ## Scope vs. the existing `tests.rs` regression suite
//!
//! `kernel/tests.rs` already covers stale re-delivery (D4 supersession) by
//! driving events through `inject_replaceable_event` (store + ingest). These
//! tests are orthogonal: they call the `ingest_contacts` method *directly* —
//! the kernel method invoked AFTER `verify_and_persist` confirms an
//! `Inserted | Replaced`. No store round-trip, no signing: the ingest method
//! consumes a `NostrEvent` (the post-JSON-decode shape) and the contract under
//! test is the store-derived contacts transition + lifecycle mutation it
//! performs.
//!
//! `NostrEvent` is `pub(super)` within `kernel`, so this file (declared as
//! `#[cfg(test)] mod ingest_tests;` in `kernel/test_modules.rs`) constructs it
//! directly — that is the minimal, deterministic fixture for a unit test of
//! these handlers. Real Schnorr signing is unnecessary because the ingest
//! method does not re-verify; the `sig` field is never read past
//! `verify_and_persist`.
//!
//! Kind:10002 (NIP-65) parsing is owned by `nmp-router::Kind10002Parser`.
//! Kernel ingest tests cover the parser-dispatch seam, not NIP-65 tag parsing
//! itself; parser behavior lives in `crates/nmp-router/src/ingest.rs`.
//!
//! Split by behavior area (#962 second wave) into `ingest_tests/`:
//!   - `ingest_support` — shared fixtures (test pubkeys, unsigned/signed
//!     event builders, NIP-01/NIP-65/NIP-17 tag builders).
//!   - `dm_relay_tests` — `recipient_dm_relays` lookup + the F-02
//!     `on_dm_relays_changed` trigger-enqueue regression.
//!   - `contacts_tests` — `ingest_contacts` (kind:3) follow-graph writes and
//!     the active-account-only source recompile trigger.
//!   - `timeline_event_tests` — `ingest_timeline_event` (kind:1) subscribed-
//!     author admission, the ADR-0070 persist-without-project oracle, and
//!     duplicate-delivery idempotence.
//!   - `open_interest_admission_tests` — ADR-0076 §5.1 generic `open_interest`
//!     store admission (author-matched and hashtag-matched, plus the
//!     negative control).

use super::nostr::NostrEvent;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

mod contacts_tests;
mod dm_relay_tests;
mod ingest_support;
mod open_interest_admission_tests;
mod timeline_event_tests;
