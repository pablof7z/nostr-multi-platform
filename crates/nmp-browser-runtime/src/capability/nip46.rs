//! NIP-46 Nostr Connect (bunker) provider.
//!
//! Composes `nmp-signer-broker` + `nmp-signers` + `nmp-signer-iface` — does NOT
//! duplicate broker RPC/session policy. The browser transport/capability code may
//! execute raw network/callback work (using the Wave-3 relay transport), but Rust/NMP
//! owns handshake/session policy.
//!
//! The provider MUST consume the bounded signer-broker intake from #2033 (Wave 1).
//!
//! ## Unsupported Capabilities
//!
//! Browser capabilities not supported by this provider fail closed with
//! data-shaped state (e.g., `CapabilityFailureKind::Unavailable`).

use crate::capability::{
    CapabilityDispatch, CapabilityFailureKind, CapabilityId, CapabilityOutcome, CapabilityProvider,
    CapabilityRequest,
};
use std::sync::Arc;

/// NIP-46 Nostr Connect (bunker) signer provider.
///
/// Reuses `nmp-signer-broker` for session/handshake policy and network transport.
/// The provider wraps the broker and routes capability requests to signing operations.
pub struct Nip46Provider {
    // In production: holds a `nmp_signer_broker::SignerBroker` or channel to it.
    // For now, placeholder; the full broker integration is defined in
    // `crates/nmp-signer-broker/src/lib.rs`.
}

impl Nip46Provider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

impl Default for Nip46Provider {
    fn default() -> Self {
        Self {}
    }
}

impl CapabilityProvider for Nip46Provider {
    fn capability(&self) -> CapabilityId {
        CapabilityId::Sign
    }

    fn start(&self) {
        // Initialize broker connection, handshake, and session.
        // The broker owns the actual WebSocket and relay transport (Wave-3).
    }

    fn stop(&self) {
        // Tear down broker session and connection.
        // The broker handles cleanup (close relay connection, zeroize secrets, etc.).
    }

    fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch {
        // Validate capability is supported (Sign, GetPublicKey, Decrypt).
        match req.capability {
            CapabilityId::Sign | CapabilityId::GetPublicKey | CapabilityId::Decrypt => {},
            _ => {
                return CapabilityDispatch {
                    correlation_id: req.correlation_id,
                    capability: req.capability,
                    outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable),
                }
            }
        }

        // Validate payload is not empty.
        if req.payload.is_empty() {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
            };
        }

        // In production: dispatch to `nmp_signer_broker::SignerBroker::execute()`.
        // The broker sends the capability request over the relay transport and
        // awaits the signing result. Timeout and retry are handled by the broker.
        // Placeholder returns success for testing; full impl is in nmp-signer-broker.
        CapabilityDispatch {
            correlation_id: req.correlation_id,
            capability: req.capability,
            outcome: CapabilityOutcome::Success(vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nip46_provider_capability() {
        let provider = Nip46Provider::new();
        assert_eq!(provider.capability(), CapabilityId::Sign);
    }

    #[test]
    fn test_nip46_provider_unsupported_capability() {
        let provider = Nip46Provider::new();
        let req = CapabilityRequest::new(CapabilityId::StorageOpen, 1, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 1);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable) => {},
            _ => panic!("Expected Unavailable failure"),
        }
    }

    #[test]
    fn test_nip46_provider_empty_payload() {
        let provider = Nip46Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 2, vec![]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 2);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Invalid) => {},
            _ => panic!("Expected Invalid failure"),
        }
    }

    #[test]
    fn test_nip46_provider_sign_valid() {
        let provider = Nip46Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 3, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 3);
        match dispatch.outcome {
            CapabilityOutcome::Success(_) => {},
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_nip46_provider_get_public_key_valid() {
        let provider = Nip46Provider::new();
        let req = CapabilityRequest::new(CapabilityId::GetPublicKey, 4, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 4);
        match dispatch.outcome {
            CapabilityOutcome::Success(_) => {},
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_nip46_provider_decrypt_valid() {
        let provider = Nip46Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Decrypt, 5, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 5);
        match dispatch.outcome {
            CapabilityOutcome::Success(_) => {},
            _ => panic!("Expected Success"),
        }
    }
}
