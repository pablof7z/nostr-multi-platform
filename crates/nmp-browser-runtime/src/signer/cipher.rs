//! Browser signer-provider NIP-44 cipher completion support (#2195).
//!
//! The native actor parks pending `Nip44EncryptForAccount` /
//! `Nip44DecryptForAccount` operations under a `CipherContinuation` sink. The
//! browser runtime mirrors that shape here: local signers resolve inline, while
//! provider-backed signers such as browser NIP-46 return a `SignerOp<String>`
//! that is parked and drained on later pump turns after relay/capability
//! re-entry resolves it.

use nmp_core::actor::CipherContinuation;
use nmp_signer_iface::{SignerError, SignerOp};
use nmp_signers::PublicKey;

use super::registry::CapabilityProviderRegistry;

/// NIP-44 cipher verb to dispatch through a signer's `nip44()` namespace.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Nip44CipherMode {
    Encrypt,
    Decrypt,
}

struct PendingCipherCompletion {
    op: SignerOp<String>,
    continuation: Option<CipherContinuation>,
}

/// Pending provider-backed NIP-44 operations.
#[derive(Default)]
pub(crate) struct PendingCipherCompletions {
    pending: Vec<PendingCipherCompletion>,
}

impl PendingCipherCompletions {
    /// Construct an empty pending-op table.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn insert(&mut self, op: SignerOp<String>, continuation: CipherContinuation) {
        self.pending.push(PendingCipherCompletion {
            op,
            continuation: Some(continuation),
        });
    }

    /// Poll pending cipher operations once, invoking ready continuations.
    ///
    /// D8: this is called only from an already-scheduled `pump()` turn after
    /// relay/capability re-entry. It must not schedule pumps for still-pending
    /// operations, because that would become a poll loop while waiting for the
    /// signer provider.
    pub(crate) fn drain_ready(&mut self) {
        let mut index = 0usize;
        while index < self.pending.len() {
            let Some(outcome) = self.pending[index].op.poll() else {
                index += 1;
                continue;
            };

            let mut pending = self.pending.remove(index);
            if let Some(continuation) = pending.continuation.take() {
                continuation.call(outcome.map_err(format_signer_error));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }
}

/// Dispatch a NIP-44 operation through a registered signer provider.
///
/// The continuation is always resolved or parked; failures are data-shaped
/// `Err(String)` outcomes so protocol workers can settle their own terminal
/// state without a host-side retry path.
pub(crate) fn dispatch_nip44_cipher(
    registry: &CapabilityProviderRegistry,
    pending: &mut PendingCipherCompletions,
    account_pubkey: &str,
    peer_pubkey: &str,
    text: &str,
    mode: Nip44CipherMode,
    continuation: CipherContinuation,
) {
    let op = begin_nip44_cipher(registry, account_pubkey, peer_pubkey, text, mode);
    match op {
        Err(reason) => continuation.call(Err(reason)),
        Ok(mut op) => match op.poll() {
            Some(outcome) => continuation.call(outcome.map_err(format_signer_error)),
            None => pending.insert(op, continuation),
        },
    }
}

fn begin_nip44_cipher(
    registry: &CapabilityProviderRegistry,
    account_pubkey: &str,
    peer_pubkey: &str,
    text: &str,
    mode: Nip44CipherMode,
) -> Result<SignerOp<String>, String> {
    let entry = registry
        .resolve(account_pubkey)
        .ok_or_else(|| format!("browser nip44: no signer for account {account_pubkey}"))?;
    let nip44 = entry
        .signer
        .nip44()
        .ok_or_else(|| format!("browser nip44: signer {account_pubkey} has no nip44 capability"))?;
    let peer = PublicKey::from_hex(peer_pubkey)
        .map_err(|e| format!("browser nip44: invalid peer pubkey: {e}"))?;

    Ok(match mode {
        Nip44CipherMode::Encrypt => nip44.encrypt(&peer, text),
        Nip44CipherMode::Decrypt => nip44.decrypt(&peer, text),
    })
}

fn format_signer_error(error: SignerError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests;
