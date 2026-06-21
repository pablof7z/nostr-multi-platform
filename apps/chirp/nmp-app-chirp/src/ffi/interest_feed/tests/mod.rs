//! Tests for the Chirp per-open author / thread flat-feed registration
//! (ADR-0042 §5.1, ADR-0058 §8 6B viewport grow wiring, ADR-0062 observer
//! catch-up).
//!
//! Split into focused modules:
//!   - `harness`          — shared actor-harness helpers (no tests)
//!   - `key_shape`        — unit-level key/shape tests (no actor needed)
//!   - `inject_then_open` — ADR-0062 inject-before-open regression tests
//!   - `open_close`       — open/close lifecycle (seeds + removes projection)
//!   - `structural_pairing` — ADR-0063 D7 Lane H structural feed-author pairing
//!   - `load_older`       — viewport growth via load_older / pull substrate

mod harness;
mod inject_then_open;
mod key_shape;
mod load_older;
mod open_close;
mod structural_pairing;
