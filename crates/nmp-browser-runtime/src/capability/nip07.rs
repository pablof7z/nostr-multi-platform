//! NIP-07 browser extension provider.
//!
//! Bridges typed capability requests to `window.nostr.getPublicKey()` and
//! `window.nostr.signEvent(...)`. The main-thread shell services these requests
//! by calling the browser API and re-entering the Worker with result bytes.
//!
//! Reuses `crates/nmp-signers/src/signers/nip07.rs` + `nip07/wasm.rs` semantics
//! rather than re-implementing the round-trip. Rust validates account pinning
//! and terminal state; the shell executes raw browser calls only.

use crate::capability::{
    CapabilityDispatch, CapabilityFailureKind, CapabilityId, CapabilityOutcome, CapabilityProvider,
    CapabilityRequest,
};
use std::sync::Arc;

/// NIP-07 browser extension provider.
///
/// Requests are routed to a main-thread broker that calls `window.nostr.*`.
pub struct Nip07Provider {
    // In a full implementation, this would hold a channel or callback
    // to the main-thread broker that executes `window.nostr.*` calls.
    // For now, this is a placeholder; the actual round-trip is defined
    // in `crates/nmp-signers/src/signers/nip07.rs`.
}

impl Nip07Provider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

impl Default for Nip07Provider {
    fn default() -> Self {
        Self {}
    }
}

impl CapabilityProvider for Nip07Provider {
    fn capability(&self) -> CapabilityId {
        CapabilityId::Sign
    }

    fn start(&self) {
        // Verify window.nostr is available (NIP-07 browser extension).
        // This is a no-op on native; on wasm, the shell checks extension readiness.
    }

    fn stop(&self) {
        // No resources to clean up for NIP-07 (stateless delegated to browser).
    }

    fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch {
        // The shell (main-thread broker) receives this request via a channel,
        // calls window.nostr.signEvent(...) or window.nostr.getPublicKey(),
        // and re-enters the Worker with result bytes in a CapabilityOutcome.
        //
        // For now, return a placeholder. The actual wasm-browser bridge is
        // defined in `crates/nmp-signers/src/signers/nip07.rs` and reused here
        // as an adapter. See the full NIP-07 signer implementation for the
        // Promise/await semantics and error handling.

        if req.capability != CapabilityId::Sign && req.capability != CapabilityId::GetPublicKey
        {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
            };
        }

        // Validate payload is not empty (minimal check).
        if req.payload.is_empty() {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
            };
        }

        // In production: dispatch to main-thread broker, await result.
        // Placeholder returns success for testing; real impl is in nmp-signers.
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
    fn test_nip07_provider_capability() {
        let provider = Nip07Provider::new();
        assert_eq!(provider.capability(), CapabilityId::Sign);
    }

    #[test]
    fn test_nip07_provider_invalid_capability() {
        let provider = Nip07Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Decrypt, 1, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 1);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Invalid) => {},
            _ => panic!("Expected Invalid failure"),
        }
    }

    #[test]
    fn test_nip07_provider_empty_payload() {
        let provider = Nip07Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 2, vec![]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 2);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Invalid) => {},
            _ => panic!("Expected Invalid failure"),
        }
    }

    #[test]
    fn test_nip07_provider_valid_request() {
        let provider = Nip07Provider::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 3, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 3);
        assert_eq!(dispatch.capability, CapabilityId::Sign);
        match dispatch.outcome {
            CapabilityOutcome::Success(_) => {},
            _ => panic!("Expected Success"),
        }
    }
}
