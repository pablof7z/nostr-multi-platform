//! Non-blocking NIP-44 cipher helpers for the ADR-0050 §D1 cipher port
//! (`Nip44EncryptForAccount` / `Nip44DecryptForAccount`).
//!
//! The cipher siblings of `identity::sign_active_nonblocking` /
//! `sign_with_account_nonblocking`. For a remote (NIP-46 / NIP-55) account they
//! forward to `RemoteSignerHandle::nip44_{encrypt,decrypt}`, returning the
//! `SignerOp` verbatim (typically `SignerOp::Pending`, which the caller parks).
//! For a local account they run `nostr::nips::nip44` **inside the runtime** so
//! the secret key never escapes (D13); the CPU-bound result is folded into a
//! `SignerOp::Ready` so the caller's single `poll()` match handles both backends
//! uniformly. `Err` (a toast string per D6) covers the no-such-account case.
//!
//! Account resolution (active vs named, remote-shadows-local) is delegated to
//! `IdentityRuntime::resolve_cipher_account` so the private key maps stay
//! encapsulated in `identity.rs`.

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::PublicKey;

use super::identity::IdentityRuntime;

/// Non-blocking NIP-44 encrypt with the active (`signer_pubkey == None`) or a
/// named account (ADR-0050 §D1).
pub(crate) fn nip44_encrypt_nonblocking(
    identity: &IdentityRuntime,
    signer_pubkey: Option<&str>,
    peer_pubkey: &str,
    plaintext: &str,
) -> Result<SignerOp<String>, String> {
    let (remote, local) = identity.resolve_cipher_account(signer_pubkey);
    if let Some(handle) = remote {
        return Ok(handle.nip44_encrypt(peer_pubkey, plaintext));
    }
    let keys = local.ok_or_else(|| no_account_error(signer_pubkey))?;
    let peer =
        PublicKey::from_hex(peer_pubkey).map_err(|e| format!("invalid peer pubkey: {e}"))?;
    match nostr::nips::nip44::encrypt(
        keys.secret_key(),
        &peer,
        plaintext,
        nostr::nips::nip44::Version::V2,
    ) {
        Ok(ciphertext) => Ok(SignerOp::ok(ciphertext)),
        Err(e) => Ok(SignerOp::err(SignerError::Backend(format!(
            "local nip44 encrypt failed: {e}"
        )))),
    }
}

/// Non-blocking NIP-44 decrypt — the inbound twin of
/// [`nip44_encrypt_nonblocking`] (ADR-0050 §D1).
pub(crate) fn nip44_decrypt_nonblocking(
    identity: &IdentityRuntime,
    signer_pubkey: Option<&str>,
    peer_pubkey: &str,
    ciphertext: &str,
) -> Result<SignerOp<String>, String> {
    let (remote, local) = identity.resolve_cipher_account(signer_pubkey);
    if let Some(handle) = remote {
        return Ok(handle.nip44_decrypt(peer_pubkey, ciphertext));
    }
    let keys = local.ok_or_else(|| no_account_error(signer_pubkey))?;
    let peer =
        PublicKey::from_hex(peer_pubkey).map_err(|e| format!("invalid peer pubkey: {e}"))?;
    match nostr::nips::nip44::decrypt(keys.secret_key(), &peer, ciphertext) {
        Ok(plaintext) => Ok(SignerOp::ok(plaintext)),
        Err(e) => Ok(SignerOp::err(SignerError::Backend(format!(
            "local nip44 decrypt failed: {e}"
        )))),
    }
}

/// Uniform "no account" error wording for the cipher helpers (D6 toast).
fn no_account_error(signer_pubkey: Option<&str>) -> String {
    match signer_pubkey {
        Some(pk) => format!("no signer for account {pk} — add it first"),
        None => "no active account — sign in first".to_string(),
    }
}
