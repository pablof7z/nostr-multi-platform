//! T120b / G4 — end-to-end tests for the production `run_relay_worker`
//! against a hermetic loopback WebSocket server, split by behavior area:
//! 1. [`support`] — shared loopback-server / event-drain fixtures used by
//!    this module and the sibling `control_disconnect_tests`,
//!    `preamble_tests` suites in `super::super`.
//! 2. [`keepalive_wiring`] — proves the worker actually emits
//!    `Message::Ping(_)` after idle, reconnects on a missing pong, and
//!    swallows inbound pongs. The keepalive FSM itself is unit-tested in
//!    `crate::keepalive`; these tests pin the wiring.
//! 3. [`preconnect_buffering`] — T130: frames sent to the worker before
//!    the socket opens are buffered and land on the wire post-Open.
//! 4. [`backoff_hint`] — V-58: `SetBackoffHint` reconnect-schedule
//!    behaviour, including composition with the V-92 healthy-session
//!    reset.

mod backoff_hint;
mod keepalive_wiring;
mod preconnect_buffering;
pub(super) mod support;
