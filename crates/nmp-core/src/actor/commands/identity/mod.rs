//! Actor-local identity runtime + sign-in / switch / remove handlers.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.
//!
//! Sub-modules (all within the 500-LOC ceiling):
//! - `dto`            — DTO types: BunkerHandshakeDto, SignerStateDto, etc.
//! - `runtime`        — IdentityRuntime struct + impl methods
//! - `sign`           — non-blocking signing helpers
//! - `account_ops`    — add_signer, switch_active, remove_account + kernel sync
//! - `create_account` — create_account + cold-start publish helpers
//! - `signer_state`   — bunker/NIP-55 connection-state + handshake lifecycle

mod account_ops;
mod create_account;
mod dto;
mod runtime;
mod sign;
mod signer_state;

// `pub` re-exports: these types are re-exported all the way to `lib.rs`
// for `nmp-ffi`. They must stay `pub` through the entire chain.
pub use dto::{
    new_bunker_handshake_slot, new_signer_state_slot, BunkerHandshakeDto, BunkerHandshakeSlot,
    SignerStateSlot,
};

// Crate-internal re-exports.
pub(crate) use dto::{
    build_nip46_onboarding_dto, BunkerStageKind, Nip46OnboardingDto, SignerStateDto,
};

pub(crate) use runtime::{IdentityId, IdentityRuntime};

pub(crate) use account_ops::{
    add_signer, remove_account, retarget_timeline, switch_active, sync_kernel,
};
pub(crate) use create_account::create_account;

pub(crate) use signer_state::{
    bunker_connection_state_changed, bunker_handshake_progress, nip55_signer_state_changed,
    restore_bunker_session, restore_nip55_session,
};

pub(crate) use sign::{sign_active_nonblocking, sign_with, sign_with_account_nonblocking};

#[cfg(test)]
#[path = "nip46_onboarding_tests.rs"]
mod nip46_onboarding_tests;
