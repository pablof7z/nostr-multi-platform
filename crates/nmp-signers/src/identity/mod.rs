//! Multi-account runtime state.
//!
//! `AccountManager` holds N signers + an optional `active`.  Switching active
//! is synchronous (NDK race fix `a14c7a78`): observers run after the new
//! signer is in place, never observing pubkey-without-signer.
//!
//! Signer dispatch + the applesauce `SignerMismatchError` post-condition live
//! here, not in the `Signer` trait — keeping the trait minimal and pushing
//! defence-in-depth into the manager.
//!
//! ## `ActiveChangeObserver` — wiring status
//!
//! `AccountManager` exposes an `ActiveChangeObserver` hook for active-account
//! transitions. It has **no production consumer**: the NMP kernel does not use
//! `AccountManager` at all — `nmp-core` owns its own `IdentityRuntime` and
//! rebuilds kind:3 / kind:10002 subscriptions directly on the `SwitchActive`
//! actor command (`nmp-core/src/actor/commands/identity.rs::switch_active`,
//! dispatched from `actor/dispatch.rs`). That direct-dispatch path superseded
//! the observer pattern, so the canned observers (`Kind3RewireObserver`,
//! `ActiveAccountReactor`) were deleted as dead scaffolding.
//!
//! The `ActiveChangeObserver` trait + `AccountManager::observe()` are retained
//! only because `nmp-testing` and the in-crate test suites still exercise
//! `AccountManager`'s transition semantics through it.

mod manager;

#[cfg(test)]
mod tests;

pub use manager::{
    AccountError, AccountManager, ActiveChangeEvent, ActiveChangeObserver, IdentityId,
};
