//! M5+M2+M8 integration tests — NIP-42 AUTH wiring in the kernel.
//!
//! These tests drive `kernel::handle_text` with synthetic relay frames (the
//! same I/O surface a real WebSocket worker would produce). No live socket;
//! `MockRelay` would be redundant here because the handshake is deterministic
//! — feed frames in order, observe state + outbound. See task #57.
//!
//! Signer injection uses an inline closure adapter; in production the actor
//! wires `nmp_signers::AccountManager::signer_active()` to the same shape
//! (cross-crate cycle prevented by the callback indirection in
//! `kernel::auth::AuthSignerFn`).
//!
//! Split by behavior area (#962 second wave) into `auth_tests/`:
//!   - `handshake_state_machine_tests` — NIP-42 challenge/authenticate/fail/
//!     retry state transitions and the rev-bump invariant.
//!   - `auth_gate_regression_tests` — no-signer-bound challenge path and
//!     claim-REQ auth gating.
//!   - `relay_url_and_role_isolation_tests` — T125 delivering-relay URL
//!     tagging and per-role signer isolation.

use super::auth_test_helpers::*;
use super::*;
use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::relay::{RelayRoleTestExt, DEFAULT_VISIBLE_LIMIT};
use crate::subs::RelayAuthState;

mod auth_gate_regression_tests;
mod handshake_state_machine_tests;
mod relay_url_and_role_isolation_tests;
