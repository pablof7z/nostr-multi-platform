//! Unit tests for the push-model [`super::Pool`] API, split by behavior
//! area:
//! 1. [`slot_lifecycle`] — pure structural pool-side bookkeeping. No real
//!    socket; the worker's spawn call is exercised but the URL is a
//!    sentinel that never connects (we only assert the pool-side
//!    bookkeeping).
//! 2. [`socket_lifecycle`] — real-socket end-to-end. Boots a
//!    `tungstenite::server::accept` on a loopback port, drives
//!    `ensure_open` + `send` + `close`, and asserts the `PoolEvent`
//!    stream.
//! 3. [`doctrine_guards`] — source-level doctrine guards for this crate's
//!    layering rules.
//! The full keepalive / reconnect / jitter behaviour is already
//! exercised by [`crate::relay_worker::tests`]. These tests focus on the
//! push-model surface added on top of it.

mod doctrine_guards;
mod slot_lifecycle;
mod socket_lifecycle;
