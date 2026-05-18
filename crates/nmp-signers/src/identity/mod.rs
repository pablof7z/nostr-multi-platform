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
//! kind:3 auto-rewire (`Kind3RewireObserver`) listens for active-account flips
//! and re-derives the active account's follow-set + relay-list.  The kernel
//! installs this observer at startup; tests can install a no-op or instrumented
//! observer.

mod manager;
mod rewire;

#[cfg(test)]
mod tests;

pub use manager::{
    AccountError, AccountManager, ActiveChangeEvent, ActiveChangeObserver, IdentityId,
};
pub use rewire::{Kind3RewireEvent, Kind3RewireObserver};
