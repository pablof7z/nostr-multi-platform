//! Typed browser capability envelope and provider registry.
//!
//! Each browser signer backend (NIP-07, nsec/local-key, NIP-46) is a provider
//! implementing `CapabilityProvider`. The registry routes typed capability
//! requests to their providers and tracks correlation IDs for result matching.
//!
//! ## Security Contract
//!
//! **Secret redaction (D13):** Capability requests where `secret_bearing == true`
//! MUST NOT be logged, snapshot, or include their payloads in any diagnostics
//! (including action tags or dispatch history). Only the redacted account prefix
//! is permitted. The kernel owns interpretation and filtering; this trait reports
//! raw success/failure only.
//!
//! **No polling (D8):** Results are delivered via correlation-id callback re-entry,
//! never via sleep loops or polling.

pub mod local_key;
pub mod nip07;
pub mod nip46;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Typed browser capability identifiers. Extend via the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityId {
    /// `window.nostr.getPublicKey()` / `window.nostr.signEvent(...)`
    Sign,
    /// Decrypt (NIP-04 or NIP-44)
    Decrypt,
    /// Get public key without signing
    GetPublicKey,
    /// OPFS/persistent storage open
    StorageOpen,
}

impl CapabilityId {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityId::Sign => "sign",
            CapabilityId::Decrypt => "decrypt",
            CapabilityId::GetPublicKey => "get_public_key",
            CapabilityId::StorageOpen => "storage_open",
        }
    }
}

/// Log-safe metadata for a capability request (no secrets).
/// Used for diagnostics, logging, and correlation tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMeta {
    /// Redacted account prefix, if any (e.g., first 16 chars of pubkey).
    pub account_prefix: Option<String>,
    /// Target kind (e.g., event kind 1, storage bucket name).
    pub target_kind: Option<String>,
    /// Human-readable description for diagnostics.
    pub description: Option<String>,
}

impl CapabilityMeta {
    pub fn new() -> Self {
        Self {
            account_prefix: None,
            target_kind: None,
            description: None,
        }
    }

    pub fn with_account_prefix(mut self, prefix: String) -> Self {
        self.account_prefix = Some(prefix);
        self
    }

    pub fn with_target_kind(mut self, kind: String) -> Self {
        self.target_kind = Some(kind);
        self
    }

    pub fn with_description(mut self, desc: String) -> Self {
        self.description = Some(desc);
        self
    }
}

impl Default for CapabilityMeta {
    fn default() -> Self {
        Self::new()
    }
}

/// Typed capability request from the kernel to a provider.
///
/// **Secret handling:** Requests where `secret_bearing == true` (e.g., nsec input)
/// MUST NOT be logged or included in snapshots except for the redacted account prefix.
#[derive(Debug, Clone)]
pub struct CapabilityRequest {
    /// Which capability this request targets.
    pub capability: CapabilityId,
    /// Correlation ID for matching results. Assigned by the kernel.
    pub correlation_id: u64,
    /// Log-safe metadata only (no secrets).
    pub meta: CapabilityMeta,
    /// Opaque payload bytes (unsigned event, ciphertext, nsec, ...).
    pub payload: Vec<u8>,
    /// True if payload carries secret material (nsec, decryption keys, ...).
    /// Excluded from logs, snapshots, and diagnostics (except redacted prefix).
    pub secret_bearing: bool,
}

impl CapabilityRequest {
    pub fn new(capability: CapabilityId, correlation_id: u64, payload: Vec<u8>) -> Self {
        Self {
            capability,
            correlation_id,
            meta: CapabilityMeta::default(),
            payload,
            secret_bearing: false,
        }
    }

    pub fn with_meta(mut self, meta: CapabilityMeta) -> Self {
        self.meta = meta;
        self
    }

    pub fn with_secret_bearing(mut self) -> Self {
        self.secret_bearing = true;
        self
    }
}

/// Failure kinds for capability outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityFailureKind {
    /// User denied the capability.
    Denied,
    /// Provider not installed or unavailable.
    Unavailable,
    /// Invalid request (malformed payload, etc.).
    Invalid,
    /// Timeout waiting for result.
    Timeout,
    /// Account mismatch (e.g., active account != signed event pubkey).
    AccountMismatch,
    /// Other error (message in the diagnostic log).
    Other,
}

impl CapabilityFailureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityFailureKind::Denied => "denied",
            CapabilityFailureKind::Unavailable => "unavailable",
            CapabilityFailureKind::Invalid => "invalid",
            CapabilityFailureKind::Timeout => "timeout",
            CapabilityFailureKind::AccountMismatch => "account_mismatch",
            CapabilityFailureKind::Other => "other",
        }
    }
}

/// Result of a capability execution.
#[derive(Debug, Clone)]
pub enum CapabilityOutcome {
    /// Success with result bytes (signed event, decrypted plaintext, ...).
    Success(Vec<u8>),
    /// Failure with kind and optional diagnostic message.
    Failure(CapabilityFailureKind),
}

/// Pending capability dispatch (for correlation tracking).
#[derive(Debug, Clone)]
pub struct CapabilityDispatch {
    /// Correlation ID to match against results.
    pub correlation_id: u64,
    /// Capability being executed.
    pub capability: CapabilityId,
    /// Outcome (success or failure).
    pub outcome: CapabilityOutcome,
}

/// Trait for capability providers (signer backends, storage, ...).
///
/// Implementations:
/// - `Nip07Provider` — `window.nostr.signEvent()` / `window.nostr.getPublicKey()`
/// - `LocalKeyProvider` — nsec/memory-only key storage
/// - `Nip46Provider` — Nostr Connect (bunker)
pub trait CapabilityProvider: Send + Sync {
    /// Which capability this provider handles.
    fn capability(&self) -> CapabilityId;

    /// Idempotent start (initialize resources, connect, ...).
    fn start(&self) {}

    /// Idempotent stop (clean up, zeroize, ...).
    fn stop(&self) {}

    /// Execute a capability request. Blocks until result is ready or timeout.
    /// Returns a correlation-tracked dispatch entry. The caller (kernel) matches
    /// by correlation_id and interprets the outcome (D6 capabilities-not-callbacks).
    ///
    /// Rust (kernel) owns:
    /// - Account pinning validation
    /// - Terminal state tracking
    /// - Retry logic
    /// - Timeout cleanup
    ///
    /// Provider owns:
    /// - Raw capability execution (e.g., browser RPC)
    /// - Raw result bytes
    /// - Denial/unavailable/timeout reporting
    fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch;
}

/// Registry mapping `CapabilityId` -> provider. Supports idempotent start/stop/restart.
pub struct CapabilityRegistry {
    providers: Arc<Mutex<HashMap<CapabilityId, Arc<dyn CapabilityProvider>>>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a provider for its capability. Idempotent (later registrations override).
    pub fn register(&self, provider: Arc<dyn CapabilityProvider>) {
        let cap_id = provider.capability();
        let mut providers = self.providers.lock().unwrap();
        providers.insert(cap_id, provider);
    }

    /// Look up a provider by capability ID.
    pub fn get(&self, capability: CapabilityId) -> Option<Arc<dyn CapabilityProvider>> {
        self.providers.lock().unwrap().get(&capability).cloned()
    }

    /// Route and execute a capability request. Stale/duplicate/unknown results are
    /// dropped with data-shaped diagnostics.
    pub fn execute(&self, req: &CapabilityRequest) -> Result<CapabilityDispatch, String> {
        let provider = self
            .get(req.capability)
            .ok_or_else(|| format!("No provider for capability: {:?}", req.capability))?;
        Ok(provider.execute(req))
    }

    /// Start all registered providers. Idempotent.
    pub fn start_all(&self) {
        let providers = self.providers.lock().unwrap();
        for provider in providers.values() {
            provider.start();
        }
    }

    /// Stop all registered providers. Idempotent.
    pub fn stop_all(&self) {
        let providers = self.providers.lock().unwrap();
        for provider in providers.values() {
            provider.stop();
        }
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CapabilityRegistry {
    fn clone(&self) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_id_str() {
        assert_eq!(CapabilityId::Sign.as_str(), "sign");
        assert_eq!(CapabilityId::Decrypt.as_str(), "decrypt");
        assert_eq!(CapabilityId::GetPublicKey.as_str(), "get_public_key");
        assert_eq!(CapabilityId::StorageOpen.as_str(), "storage_open");
    }

    #[test]
    fn test_capability_meta_builder() {
        let meta = CapabilityMeta::new()
            .with_account_prefix("abc123".to_string())
            .with_target_kind("event_kind_1".to_string())
            .with_description("Sign a note".to_string());

        assert_eq!(meta.account_prefix, Some("abc123".to_string()));
        assert_eq!(meta.target_kind, Some("event_kind_1".to_string()));
        assert_eq!(meta.description, Some("Sign a note".to_string()));
    }

    #[test]
    fn test_capability_request_builder() {
        let payload = vec![1, 2, 3];
        let req = CapabilityRequest::new(CapabilityId::Sign, 42, payload.clone())
            .with_secret_bearing();

        assert_eq!(req.capability, CapabilityId::Sign);
        assert_eq!(req.correlation_id, 42);
        assert_eq!(req.payload, payload);
        assert_eq!(req.secret_bearing, true);
    }

    #[test]
    fn test_failure_kind_str() {
        assert_eq!(CapabilityFailureKind::Denied.as_str(), "denied");
        assert_eq!(CapabilityFailureKind::Unavailable.as_str(), "unavailable");
        assert_eq!(CapabilityFailureKind::Invalid.as_str(), "invalid");
        assert_eq!(CapabilityFailureKind::Timeout.as_str(), "timeout");
        assert_eq!(CapabilityFailureKind::AccountMismatch.as_str(), "account_mismatch");
        assert_eq!(CapabilityFailureKind::Other.as_str(), "other");
    }

    struct MockProvider {
        cap_id: CapabilityId,
    }

    impl CapabilityProvider for MockProvider {
        fn capability(&self) -> CapabilityId {
            self.cap_id
        }

        fn execute(&self, req: &CapabilityRequest) -> CapabilityDispatch {
            CapabilityDispatch {
                correlation_id: req.correlation_id,
                capability: req.capability,
                outcome: CapabilityOutcome::Success(vec![1, 2, 3]),
            }
        }
    }

    #[test]
    fn test_registry_register_and_execute() {
        let registry = CapabilityRegistry::new();
        let provider = Arc::new(MockProvider {
            cap_id: CapabilityId::Sign,
        });
        registry.register(provider);

        let req = CapabilityRequest::new(CapabilityId::Sign, 1, vec![]);
        let dispatch = registry.execute(&req).expect("execute should succeed");

        assert_eq!(dispatch.correlation_id, 1);
        assert_eq!(dispatch.capability, CapabilityId::Sign);
    }

    #[test]
    fn test_registry_unknown_capability() {
        let registry = CapabilityRegistry::new();
        let req = CapabilityRequest::new(CapabilityId::Sign, 1, vec![]);

        assert!(registry.execute(&req).is_err());
    }

    #[test]
    fn test_registry_clone_preserves_providers() {
        let registry = CapabilityRegistry::new();
        let provider = Arc::new(MockProvider {
            cap_id: CapabilityId::Sign,
        });
        registry.register(provider);

        let cloned = registry.clone();
        let req = CapabilityRequest::new(CapabilityId::Sign, 1, vec![]);
        assert!(cloned.execute(&req).is_ok());
    }
}
