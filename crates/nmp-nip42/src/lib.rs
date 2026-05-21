//! `nmp-nip42` — NIP-42 relay AUTH protocol module.
//!
//! Owns the M5 / T40 surface called out in `docs/plan/m5-nip42.md` and the
//! T40 substitution contract in `docs/plan/m8-subscription-lifecycle.md` §3:
//!
//! - [`state::RelayAuthState`] — canonical per-relay NIP-42 lifecycle,
//!   re-exported from the `nmp-nip42-types` substrate crate (T77).
//!   `nmp_core::subs::RelayAuthState` is now *the same type* (also a
//!   re-export of `nmp_nip42_types::RelayAuthState`), so the old seam
//!   placeholder and the `relay_auth_state_to_subs` translation function
//!   are retired — no conversion is needed across the crate boundary.
//! - [`frame::AuthChallenge`] — `["AUTH", <challenge>]` parser.
//! - [`frame::AuthOk`] — `["OK", <event_id>, <accepted>, <reason>]` matcher
//!   for in-flight kind:22242 events. (OKs for non-auth events are not this
//!   crate's concern; the publish engine in `nmp_core::publish` matches its
//!   own OKs.)
//! - [`builder::build_auth_event`] — kind:22242 [`UnsignedEvent`] template
//!   with the two mandatory tags (`relay`, `challenge`).
//! - [`flow::Nip42Driver`] — per-relay handshake driver. Holds the in-flight
//!   challenge + signed-event-id. Stateful but cheap (one driver per relay).
//!
//! ## Doctrine alignment
//!
//! - **D0** — no app nouns. The crate exposes only protocol primitives
//!   (challenge, auth state, signed-event template) and a stateful driver.
//! - **D7** — the [`Signer`](nmp_core::publish::traits::Signer) reports a
//!   signed event; this crate decides the FSM transitions and what to emit
//!   on the wire. Pause/flush of held REQs is owned by
//!   [`nmp_core::subs::auth_gate::AuthGate`], not here.
//! - **D6** — `Result<…, Nip42Error>` is the internal flow control type;
//!   the caller surfaces the error as a `RelayAuthState::Failed` diagnostic
//!   (the FFI toast bridge is M10.5 scope per `docs/design/ffi-hardening.md`
//!   §7.2 — not added by this crate).
//! - ADR-0007 §1 — `RelayAuthState` enum matches the diagnostics contract
//!   exactly (`NotRequired | ChallengeReceived | Authenticating |
//!   Authenticated | Failed`).
//!
//! ## What this crate does NOT do
//!
//! - It does not own the wire pause/flush queue — that's `subs::AuthGate`.
//! - It does not own publish-side AUTH-REQUIRED retry — that's the publish
//!   engine using its own `publish::traits::Signer::sign_auth` shim.
//! - It does not call the signer asynchronously — `LocalKeySigner` is
//!   synchronous via `SignerOp::Ready`, sufficient for M5; NIP-46 async is
//!   M6 follow-up.
//! - It does not own the FFI surface — the signer is bound by M6's
//!   account-manager wiring, not by a hand-rolled C callback.

pub mod builder;
pub mod flow;
pub mod frame;
pub mod state;

pub use builder::build_auth_event;
pub use flow::{run_handshake, HandshakeOutcome, Nip42Driver, Nip42Error};
pub use frame::{parse_auth_frame, parse_ok_frame, AuthChallenge, AuthOk};
pub use state::RelayAuthState;
