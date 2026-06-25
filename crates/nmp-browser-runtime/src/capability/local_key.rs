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
use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::{LocalKeySigner, Signer};
use std::sync::{Arc, Mutex};

/// Local nsec/memory-only signer provider.
///
/// Holds a LocalKeySigner in memory. Never persisted; cleared on logout.
pub struct LocalKeyProvider {
    // In-memory signer (cleared on logout).
    signer: Arc<Mutex<Option<LocalKeySigner>>>,
}

impl LocalKeyProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            signer: Arc::new(Mutex::new(None)),
        })
    }

    /// Load an nsec into memory. This is the only entry point for secret-bearing input.
    /// The caller MUST mark the request `secret_bearing = true`.
    pub fn load_nsec(&self, nsec: String) -> Result<(), String> {
        let signer = LocalKeySigner::from_nsec(&nsec)
            .map_err(|e| format!("Failed to load nsec: {}", e))?;
        let mut stored = self.signer.lock().unwrap();
        *stored = Some(signer);
        Ok(())
    }

    /// Zeroize and forget the held signer. Called on logout or app shutdown.
    pub fn forget(&self) {
        let mut stored = self.signer.lock().unwrap();
        *stored = None;
    }

    /// Check if a signer is loaded (for diagnostics only; never returns the secret).
    pub fn is_loaded(&self) -> bool {
        let stored = self.signer.lock().unwrap();
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
            signer: Arc::clone(&self.signer),
        }
    }
}

impl CapabilityProvider for LocalKeyProvider {
    fn capability(&self) -> CapabilityId {
        CapabilityId::Sign
    }

    fn start(&self) {
        // Memory-only storage; no initialization needed.
    }

    fn stop(&self) {
        // Clear the signer on shutdown.
        self.forget();
    }

    fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch {
        // Check if signer is loaded first (availability before validation).
        let stored = self.signer.lock().unwrap();
        let signer = match stored.as_ref() {
            Some(s) => s,
            None => {
                return CapabilityDispatch {
                    correlation_id: req.correlation_id,
                    capability: req.capability,
                    outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable),
                };
            }
        };

        // Validate that this request is marked secret_bearing if it contains secret material.
        if !req.secret_bearing && req.capability == CapabilityId::Sign {
            return CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
            };
        }

        // Route to the appropriate capability.
        match req.capability {
            CapabilityId::Sign => {
                // Deserialize the unsigned event from payload bytes.
                match serde_json::from_slice::<UnsignedEvent>(&req.payload) {
                    Ok(unsigned) => {
                        // Use nmp_signers to sign (reusing existing LocalKeySigner implementation).
                        match signer.sign(unsigned) {
                            SignerOp::Ready(result) => {
                                match result {
                                    Ok(signed) => {
                                        // Serialize the signed event back to bytes.
                                        match serde_json::to_vec(&signed) {
                                            Ok(bytes) => CapabilityDispatch {
                                                correlation_id: req.correlation_id,
                                                capability: req.capability,
                                                outcome: CapabilityOutcome::Success(bytes),
                                            },
                                            Err(_) => CapabilityDispatch {
                                                correlation_id: req.correlation_id,
                                                capability: req.capability,
                                                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Other),
                                            },
                                        }
                                    }
                                    Err(_) => CapabilityDispatch {
                                        correlation_id: req.correlation_id,
                                        capability: req.capability,
                                        outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Other),
                                    },
                                }
                            }
                            SignerOp::Pending(_) => {
                                // LocalKeySigner only returns Ready, never Pending.
                                CapabilityDispatch {
                                    correlation_id: req.correlation_id,
                                    capability: req.capability,
                                    outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Timeout),
                                }
                            }
                        }
                    }
                    Err(_) => CapabilityDispatch {
                        correlation_id: req.correlation_id,
                        capability: req.capability,
                        outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Invalid),
                    },
                }
            }
            CapabilityId::GetPublicKey => {
                // Return the public key as a string encoded as JSON bytes.
                let pubkey = signer.pubkey().to_hex();
                match serde_json::to_vec(&pubkey) {
                    Ok(bytes) => CapabilityDispatch {
                        correlation_id: req.correlation_id,
                        capability: req.capability,
                        outcome: CapabilityOutcome::Success(bytes),
                    },
                    Err(_) => CapabilityDispatch {
                        correlation_id: req.correlation_id,
                        capability: req.capability,
                        outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Other),
                    },
                }
            }
            _ => CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Failure(CapabilityFailureKind::Unavailable),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_signer_iface::UnsignedEvent;

    #[test]
    fn test_local_key_provider_new() {
        let provider = LocalKeyProvider::new();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_load_nsec() {
        let provider = LocalKeyProvider::new();
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        assert!(provider.load_nsec(TEST_NSEC.to_string()).is_ok());
        assert!(provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_forget() {
        let provider = LocalKeyProvider::new();
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        provider.load_nsec(TEST_NSEC.to_string()).unwrap();
        assert!(provider.is_loaded());

        provider.forget();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_capability() {
        let provider = LocalKeyProvider::new();
        assert_eq!(provider.capability(), CapabilityId::Sign);
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
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        provider.load_nsec(TEST_NSEC.to_string()).unwrap();

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
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        provider.load_nsec(TEST_NSEC.to_string()).unwrap();

        // Create a valid unsigned event and serialize it
        let unsigned = UnsignedEvent {
            pubkey: String::new(),
            kind: 1,
            tags: vec![],
            content: "test event".to_string(),
            created_at: 1_700_000_000,
        };
        let payload = serde_json::to_vec(&unsigned).expect("serialize unsigned");

        let req = CapabilityRequest::new(CapabilityId::Sign, 3, payload)
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
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        provider.load_nsec(TEST_NSEC.to_string()).unwrap();
        assert!(provider.is_loaded());

        provider.stop();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn test_local_key_provider_clone_shares_nsec() {
        let provider = LocalKeyProvider::new();
        const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
        provider.load_nsec(TEST_NSEC.to_string()).unwrap();

        let cloned = provider.clone();
        assert!(cloned.is_loaded());

        cloned.forget();
        assert!(!provider.is_loaded()); // Both share the same signer
    }
}
