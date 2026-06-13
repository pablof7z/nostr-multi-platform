//! Free `build_*` constructors for the backend-transparent signer-port commands
//! (`SignEventForAccount` / `Nip44EncryptForAccount`).
//!
//! These let a spawned worker thread — which holds only a
//! [`CommandSender`](crate::actor::CommandSender) via
//! [`ProtocolCommandContext::command_sender_clone`](super::ProtocolCommandContext::command_sender_clone),
//! not the actor-thread `ctx` — construct a port command and `send` it itself.
//! Extracted from `protocol.rs` to keep that file within its LOC budget; the
//! `ProtocolCommandContext` helper methods (`sign_event_for_account`,
//! `nip44_encrypt_for_account`) delegate here.

use crate::ActorCommand;

/// Build an [`ActorCommand::SignEventForAccount`] (ADR-0043 Decision 2) — the
/// generic, backend-transparent sign-account port.
///
/// The actor's dispatch arm signs (active account when `signer_pubkey` is
/// `None`, else the named roster key) and invokes `continuation` with the
/// resolved [`crate::substrate::SignedEvent`] or an error string — inline for a
/// local key, from the idle-loop drain for a parked NIP-46 bunker. The caller
/// cannot tell which.
///
/// The continuation runs on the actor thread; it must only enqueue further work
/// (D8) and never receives raw key bytes (D13).
pub fn build_sign_event_for_account(
    unsigned: crate::substrate::UnsignedEvent,
    signer_pubkey: Option<String>,
    continuation: impl FnOnce(Result<crate::substrate::SignedEvent, String>) + Send + 'static,
) -> ActorCommand {
    ActorCommand::SignEventForAccount {
        unsigned,
        signer_pubkey,
        continuation: crate::actor::SignContinuation::new(continuation),
    }
}

/// Build an [`ActorCommand::Nip44EncryptForAccount`] (ADR-0050 §D1) — the NIP-44
/// encrypt twin of [`build_sign_event_for_account`].
///
/// The actor's dispatch arm encrypts `plaintext` → `peer_pubkey` with the named
/// (`Some(hex)`) or active (`None`) account and invokes `continuation` with the
/// ciphertext or an error string — inline for a local key, from the idle-loop
/// drain for a parked NIP-46 bunker.
///
/// The continuation runs on the actor thread; it must only enqueue further work
/// (D8) and receives only ciphertext (D13).
pub fn build_nip44_encrypt_for_account(
    peer_pubkey: String,
    plaintext: String,
    signer_pubkey: Option<String>,
    continuation: impl FnOnce(Result<String, String>) + Send + 'static,
) -> ActorCommand {
    ActorCommand::Nip44EncryptForAccount {
        peer_pubkey,
        plaintext,
        signer_pubkey,
        continuation: crate::actor::CipherContinuation::new(continuation),
    }
}

/// Build an [`ActorCommand::Nip44DecryptForAccount`] (ADR-0050 §D1) — the
/// inbound twin of [`build_nip44_encrypt_for_account`].
///
/// The actor's dispatch arm decrypts `ciphertext` (encrypted FROM `peer_pubkey`)
/// with the named (`Some(hex)`) or active (`None`) account and invokes
/// `continuation` with the recovered plaintext or an error string — inline for a
/// local key, from the idle-loop drain for a parked NIP-46 bunker. This is the
/// receive-side primitive the NIP-17 DM inbox composes its two-step gift-UNWRAP
/// chain from (ADR-0050 §D6): outer wrap decrypt → seal decrypt, so a bunker
/// account can unseal a gift-wrap without the inbox ever holding raw `Keys`.
///
/// The continuation runs on the actor thread; it must only enqueue further work
/// (D8) and receives only plaintext (D13).
pub fn build_nip44_decrypt_for_account(
    peer_pubkey: String,
    ciphertext: String,
    signer_pubkey: Option<String>,
    continuation: impl FnOnce(Result<String, String>) + Send + 'static,
) -> ActorCommand {
    ActorCommand::Nip44DecryptForAccount {
        peer_pubkey,
        ciphertext,
        signer_pubkey,
        continuation: crate::actor::CipherContinuation::new(continuation),
    }
}
