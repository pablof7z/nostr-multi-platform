//! T66a actor command handlers — identity / publish / relay-edit.
//!
//! ## D0 boundary
//!
//! The actor lives in `nmp-core`. `nmp-signers` depends on `nmp-core`, so
//! `nmp-core` CANNOT import `nmp-signers` (would be a dependency cycle).
//! `AccountManager` / `LocalKeySigner` / `Nip46Signer` therefore cannot be
//! used here. Instead the actor keeps a local `IdentityRuntime` of bare
//! `nostr::Keys` (for nsec/generated accounts) plus a map of
//! `Box<dyn RemoteSignerHandle>` (for NIP-46 / NIP-07 / hardware signers),
//! and adapts each active account to the kernel's existing `AuthSignerFn`
//! seam (`Kernel::bind_auth_signer`). `RemoteSignerHandle` is defined in
//! `crate::remote_signer` so the actor uses signers without importing the
//! `nmp-signers` crate; concrete impls live in `nmp-signers` and reach the
//! actor through the broker (below). Full `AccountManager` integration is
//! M14 (UniFFI), when the FFI surface can move to a crate that may depend
//! on both `nmp-core` and `nmp-signers`.
//!
//! ## NIP-46
//!
//! Doctrine D0 still forbids `nmp-core -> nmp-signers`, so the NIP-46
//! handshake (kind:24133 relay subscription, connect/get_public_key RPCs)
//! lives in a separate broker crate that depends on BOTH `nmp-core` and
//! `nmp-signers`. The actor's role is purely to host the `Box<dyn
//! RemoteSignerHandle>` once the broker has completed the handshake:
//!
//! - `ActorCommand::SignInBunker { uri }` — actor shape-validates the URI
//!   and seeds `kernel.bunker_handshake` with `"connecting"`. The broker
//!   then drives the real handshake on its own relay client.
//! - `ActorCommand::BunkerHandshakeProgress { stage, message }` — broker
//!   pushes progress (`"connecting"` → `"awaiting_pubkey"` → `"ready"` /
//!   `"failed"`); the actor reflects it on the snapshot.
//! - `ActorCommand::AddRemoteSigner { handle }` — once the handshake
//!   completes (the broker has the user's pubkey from `get_public_key`),
//!   it hands the fully-initialized handle to the actor. The actor
//!   inserts it into `IdentityRuntime.remote_signers`, becomes active if
//!   no account was active, and routes all subsequent `sign_active`
//!   through the handle's `sign(unsigned).wait(timeout)` call.
//! - `ActorCommand::RemoveRemoteSigner { identity_id }` — broker (or UI)
//!   asks the actor to drop the handle (e.g. logout).
//!
//! The actor never imports `nmp-signers`; it only touches the trait. NIP-42
//! is currently cleared while a remote signer is active (the broker's
//! ephemeral key cannot sign NIP-42 challenges as the user); routing NIP-42
//! through the remote signer is a documented follow-up
//! (TODO(nip46-nip42) in `identity.rs:sync_kernel`).

mod identity;
mod publish;
mod relays;
mod wallet;
#[cfg(test)]
mod remote_signer_tests;
#[cfg(test)]
mod tests;

pub(super) use identity::{
    add_remote_signer, bunker_handshake_progress, create_account, remove_account,
    remove_remote_signer, sign_in_bunker, sign_in_nsec, switch_active, IdentityRuntime,
};
pub(super) use publish::{follow, open_timeline, publish_note, publish_unsigned_event, react};
pub(super) use relays::{add_relay, remove_relay};
pub(super) use wallet::{
    handle_nwc_text, wallet_connect, wallet_disconnect, wallet_pay_invoice, WalletRuntime,
};
