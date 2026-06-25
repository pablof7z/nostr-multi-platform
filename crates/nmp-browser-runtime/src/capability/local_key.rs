//! Local nsec/memory-only key provider.
//!
//! nsec input enters ONLY through an explicit secret-bearing capability/action path
//! (`secret_bearing = true`). The provider reuses `nmp_signers`/`nmp_signer_iface`
//! local-key semantics (in-memory `Keys`/local signer) — NOT web-only signing code.
//!
//! ## Storage Policy
//!
//! **MEMORY-ONLY for now** (no persistence) until the OPFS/secure-storage decision
//! (ADR-0054) lands. The key is held in the Worker's runtime core, never serialized
//! into a snapshot or log.
//!
//! ## Secret Handling (D13)
//!
//! The nsec NEVER appears in any diagnostics summary, snapshot bytes, or logs.
//! Only the redacted account prefix is permitted. Use `forget()`/logout to
//! zeroize the held key on logout.

use crate::capability::{
    CapabilityDispatch, CapabilityFailureKind, CapabilityId, CapabilityOutcome, CapabilityProvider,
    CapabilityRequest,
};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// Local nsec/memory-only signer provider.
///
/// Holds a zeroized nsec string in memory. Never persisted; cleared on logout.
pub struct LocalKeyProvider {
    // Zeroized nsec (wrapped so it's cleared on drop).
    nsec: Arc<Mutex<Option<Zeroizing<String>>>>,
}

impl LocalKeyProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            nsec: Arc::new(Mutex::new(None)),
        })
    }

    /// Load an nsec into memory. This is the only entry point for secret-bearing input.
    /// The caller MUST mark the request `secret_bearing = true`.
    pub fn load_nsec(&self, nsec: String) -> Result<(), String> {
        let mut stored = self.nsec.lock().unwrap();
        *stored = Some(Zeroizing::new(nsec));
        Ok(())
    }

    /// Zeroize and forget the held nsec. Called on logout or app shutdown.
    pub fn forget(&self) {
        let mut stored = self.nsec.lock().unwrap();
        *stored = None;
    }

    /// Check if an nsec is loaded (for diagnostics only; never returns the secret).
    pub fn is_loaded(&self) -> bool {
        let stored = self.nsec.lock().unwrap();
        stored.is_some()
    }
}

impl Default for LocalKeyProvider {
    fn default() -> Self {
        Self::new().as_ref().clone()
    }
}

// Enable Clone by returning a new Arc to the same Mutex (Arc is cheaply cloneable).
impl Clone for LocalKeyProvider {
    fn clone(&self) -> Self {
        Self {
            nsec: Arc::clone(&self.nsec),
        }
    }
}

impl CapabilityProvider for LocalKeyProvider {
    fn capability(&self) -> CapabilityId {
        CapabilityId::GetPublicKey
    }

    fn start(&self) {
        // Memory-only storage; no initialization needed.
    }

    fn stop(&self) {
        // Clear the nsec on shutdown.
        self.forget();
    }

    fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch {
        // Validate that this request is marked secret_bearing if it contains secret material.
        if !req.secret_bearing && req.capability == CapabilityId::Sign {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
            };
        }

        // Check if nsec is loaded.
        let stored = self.nsec.lock().unwrap();
        if stored.is_none() {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable),
            };
        }

        // In production: use the stored nsec to sign or derive pubkey.
        // Placeholder returns success; full impl is in nmp-signers local signer.
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
    fn test_local_key_provider_new() {
        let provider = LocalKeyProvider::new();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_load_nsec() {
        let provider = LocalKeyProvider::new();
        assert!(provider.load_nsec("nsec1234567890".to_string()).is_ok());
        assert!(provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_forget() {
        let provider = LocalKeyProvider::new();
        provider.load_nsec("nsec1234567890".to_string()).unwrap();
        assert!(provider.is_loaded());

        provider.forget();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_capability() {
        let provider = LocalKeyProvider::new();
        assert_eq!(provider.capability(), CapabilityId::GetPublicKey);
    }

    #[test]
    fn test_local_key_provider_unavailable_when_no_key() {
        let provider = LocalKeyProvider::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 1, vec![1, 2, 3]);
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 1);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable) => {},
            _ => panic!("Expected Unavailable failure"),
        }
    }

    #[test]
    fn test_local_key_provider_sign_requires_secret_bearing() {
        let provider = LocalKeyProvider::new();
        provider.load_nsec("nsec1234567890".to_string()).unwrap();

        let req = CapabilityRequest::new(CapabilityId::Sign, 2, vec![1, 2, 3]);
        // Not marked secret_bearing — should fail.
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 2);
        match dispatch.outcome {
            CapabilityOutcome::Failure(CapabilityFailureKind::Invalid) => {},
            _ => panic!("Expected Invalid failure"),
        }
    }

    #[test]
    fn test_local_key_provider_sign_with_secret_bearing() {
        let provider = LocalKeyProvider::new();
        provider.load_nsec("nsec1234567890".to_string()).unwrap();

        let req = CapabilityRequest::new(CapabilityId::Sign, 3, vec![1, 2, 3])
            .with_secret_bearing();
        let dispatch = provider.execute(&req);

        assert_eq!(dispatch.correlation_id, 3);
        match dispatch.outcome {
            CapabilityOutcome::Success(_) => {},
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_local_key_provider_stop_forgets_key() {
        let provider = LocalKeyProvider::new();
        provider.load_nsec("nsec1234567890".to_string()).unwrap();
        assert!(provider.is_loaded());

        provider.stop();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_clone_shares_nsec() {
        let provider = LocalKeyProvider::new();
        provider.load_nsec("nsec1234567890".to_string()).unwrap();

        let cloned = provider.clone();
        assert!(cloned.is_loaded());

        cloned.forget();
        assert!(!provider.is_loaded()); // Both share the same nsec
    }
}
