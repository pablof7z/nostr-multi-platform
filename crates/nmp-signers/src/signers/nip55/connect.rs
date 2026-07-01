//! NIP-55 first-connect flow (ADR-0048 Stage 2).
//!
//! [`Nip55Connect`] owns the initial `get_public_key` round-trip that happens
//! BEFORE a [`Nip55Signer`] exists: it builds the request (carrying the
//! caller-supplied permission batch, D2 — Rust decides *what* to ask for; the
//! batch itself is an app-owned policy fact per crate-boundaries.md §9, not a
//! framework default), tracks the correlation id, and on a successful reply
//! constructs the fully-initialised signer.
//!
//! The native-runtime adapter (`nmp-native-runtime::external_signer`) holds a
//! `Nip55Connect` while the host round-trip is in flight and resolves it from
//! `deliver`:
//!
//! ```text
//! signin(pkg) ── Nip55Connect::new ── transport.send_request(get_public_key)
//!                                     │ (host: Intent → Amber → approve)
//! deliver(resp) ─ matches()? ── complete() ─→ Nip55Signer ─→ AddSigner
//! ```
//!
//! D7: the host reports the raw pubkey result; identity validation (hex or
//! `npub1…` parse) happens here, in Rust.

use std::sync::Arc;

use nmp_signer_iface::{
    ExternalSignerMethod, ExternalSignerOutcome, ExternalSignerRequest, ExternalSignerResponse,
    ExternalSignerTransport, Nip55Permission, SignerError,
};
use nostr::PublicKey;

use super::{generate_correlation_id, Nip55Signer};

/// An in-flight NIP-55 first-connect (`get_public_key`) round-trip.
#[derive(Debug)]
pub struct Nip55Connect {
    request: ExternalSignerRequest,
}

impl Nip55Connect {
    /// Build the first-connect `get_public_key` request.
    ///
    /// `signer_package` is `None` when the host has not yet resolved which
    /// signer app will answer (the OS resolver picks); `Some` to route to a
    /// specific package. `permissions` is the caller-supplied batch to
    /// request so every subsequent call can use the `ContentResolver`
    /// fast-path (D2) — the batch is app policy (crate-boundaries.md §9),
    /// never decided by this crate.
    #[must_use]
    pub fn new(signer_package: Option<String>, permissions: Vec<Nip55Permission>) -> Self {
        Self {
            request: ExternalSignerRequest {
                correlation_id: generate_correlation_id(),
                method: ExternalSignerMethod::GetPublicKey,
                payload: String::new(),
                current_user: None,
                counterparty: None,
                permissions,
                granted_permissions: Vec::new(),
                signer_package,
                force_interactive: false,
            },
        }
    }

    /// The fully-built request to hand to the transport.
    #[must_use]
    pub fn request(&self) -> &ExternalSignerRequest {
        &self.request
    }

    /// Whether `response` answers this connect round-trip.
    #[must_use]
    pub fn matches(&self, response: &ExternalSignerResponse) -> bool {
        response.correlation_id == self.request.correlation_id
    }

    /// Resolve the connect from the host's raw response.
    ///
    /// On `Ok` the result string is parsed as the user pubkey (hex or
    /// `npub1…` — Amber replies with the bech32 form) and a fully-initialised
    /// [`Nip55Signer`] is returned, carrying the granted permission batch and
    /// the signer package the OS reported (falling back to the requested
    /// package when the reply omits it).
    ///
    /// Errors map 1:1 from the wire outcome (D6 — failures are data):
    /// `Rejected` → [`SignerError::Rejected`], `Unavailable` →
    /// [`SignerError::Unavailable`], `SignerError`/unparseable pubkey →
    /// [`SignerError::Backend`].
    pub fn complete(
        self,
        response: &ExternalSignerResponse,
        transport: Arc<dyn ExternalSignerTransport>,
    ) -> Result<Nip55Signer, SignerError> {
        let result = match &response.outcome {
            ExternalSignerOutcome::Ok { result } => result,
            ExternalSignerOutcome::Rejected { reason } => {
                return Err(SignerError::Rejected(reason.clone()))
            }
            ExternalSignerOutcome::Unavailable { reason } => {
                return Err(SignerError::Unavailable(reason.clone()))
            }
            ExternalSignerOutcome::SignerError { reason } => {
                return Err(SignerError::Backend(reason.clone()))
            }
        };
        let pubkey = PublicKey::parse(result.trim())
            .map_err(|e| SignerError::Backend(format!("nip55 connect: bad pubkey reply: {e}")))?;
        let signer_package = response
            .signer_package
            .clone()
            .or_else(|| self.request.signer_package.clone());
        Ok(Nip55Signer::new(
            pubkey,
            signer_package,
            self.request.permissions,
            transport,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first-connect request carries exactly the caller-supplied
    /// permission batch — `Nip55Connect` never invents a default (issue
    /// #2523 / crate-boundaries.md §9: the batch is app policy).
    #[test]
    fn new_request_carries_caller_supplied_permissions() {
        let permissions = vec![
            Nip55Permission::sign_event(0),
            Nip55Permission::sign_event(1),
            Nip55Permission::sign_event(3),
            Nip55Permission::sign_event(4),
            Nip55Permission::sign_event(7),
            Nip55Permission::nip44_encrypt(),
            Nip55Permission::nip44_decrypt(),
        ];
        let connect = Nip55Connect::new(
            Some("com.greenart7c3.nostrsigner".to_string()),
            permissions.clone(),
        );
        assert_eq!(connect.request().permissions, permissions);
    }

    /// An empty caller-supplied batch is requested verbatim — `Nip55Connect`
    /// does not pad it with a synthesized default.
    #[test]
    fn new_request_with_empty_permissions_stays_empty() {
        let connect = Nip55Connect::new(None, Vec::new());
        assert!(connect.request().permissions.is_empty());
    }
}
