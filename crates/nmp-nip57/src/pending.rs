//! Pending zap provider expectations.
//!
//! The LNURL-pay metadata names the Nostr pubkey that is allowed to mint the
//! resulting kind:9735 receipt. The receipt itself has no action correlation id,
//! but its `description` tag embeds the signed kind:9734 zap request, including
//! that request's event id. This registry keeps a bounded map from that request
//! id to the expected LNURL provider pubkey so receipt aggregators can reject
//! forged receipts whose author is not the advertised provider.

use std::sync::{Arc, Mutex, OnceLock};

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nostr::PublicKey;

use crate::decode::{try_from_kernel_event, ZapReceiptRecord};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZapReceiptProviderMismatch {
    pub zap_request_id: String,
    pub expected_provider_pubkey: String,
    pub actual_provider_pubkey: String,
}

pub type PendingZapRegistryHandle = Arc<PendingZapRegistry>;

#[derive(Debug)]
pub struct PendingZapRegistry {
    expected_provider_by_request: Mutex<BoundedMessageMap<String, String>>,
}

static ACTIVE_PENDING_ZAPS: OnceLock<PendingZapRegistryHandle> = OnceLock::new();

#[must_use]
pub fn new_pending_zap_registry() -> PendingZapRegistryHandle {
    Arc::new(PendingZapRegistry::new())
}

#[must_use]
pub fn active_pending_zap_registry() -> PendingZapRegistryHandle {
    Arc::clone(ACTIVE_PENDING_ZAPS.get_or_init(new_pending_zap_registry))
}

#[must_use]
pub fn try_from_kernel_event_validated(event: &KernelEvent) -> Option<ZapReceiptRecord> {
    active_pending_zap_registry().try_from_kernel_event(event)
}

impl Default for PendingZapRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingZapRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            expected_provider_by_request: Mutex::new(BoundedMessageMap::new(
                MAX_PROJECTION_MESSAGES,
            )),
        }
    }

    pub fn remember_expected_provider(
        &self,
        zap_request_id: impl Into<String>,
        expected_provider_pubkey: impl AsRef<str>,
    ) -> Result<(), String> {
        let zap_request_id = zap_request_id.into();
        if zap_request_id.trim().is_empty() {
            return Err("zap request id is empty".to_string());
        }
        let expected_provider_pubkey = canonical_pubkey(expected_provider_pubkey.as_ref())
            .map_err(|e| format!("LNURL nostrPubkey is invalid: {e}"))?;
        let mut guard = self
            .expected_provider_by_request
            .lock()
            .map_err(|_| "zap provider registry is unavailable".to_string())?;
        guard.insert(zap_request_id, expected_provider_pubkey);
        Ok(())
    }

    #[must_use]
    pub fn expected_provider_for(&self, zap_request_id: &str) -> Option<String> {
        self.expected_provider_by_request
            .lock()
            .ok()
            .and_then(|guard| guard.get(zap_request_id).cloned())
    }

    pub fn validate_receipt(
        &self,
        record: &ZapReceiptRecord,
    ) -> Result<(), ZapReceiptProviderMismatch> {
        let Some(zap_request_id) = record.zap_request_id.as_deref() else {
            return Ok(());
        };
        let Some(expected_provider_pubkey) = self.expected_provider_for(zap_request_id) else {
            return Ok(());
        };
        let actual_matches = canonical_pubkey(&record.provider_pubkey)
            .map(|actual| actual == expected_provider_pubkey)
            .unwrap_or(false);
        if actual_matches {
            return Ok(());
        }
        Err(ZapReceiptProviderMismatch {
            zap_request_id: zap_request_id.to_string(),
            expected_provider_pubkey,
            actual_provider_pubkey: record.provider_pubkey.clone(),
        })
    }

    #[must_use]
    pub fn try_from_kernel_event(&self, event: &KernelEvent) -> Option<ZapReceiptRecord> {
        let record = try_from_kernel_event(event)?;
        if let Err(mismatch) = self.validate_receipt(&record) {
            tracing::warn!(
                zap_request_id = %mismatch.zap_request_id,
                expected_provider_pubkey = %mismatch.expected_provider_pubkey,
                actual_provider_pubkey = %mismatch.actual_provider_pubkey,
                "nip57: rejecting zap receipt from unexpected LNURL provider"
            );
            return None;
        }
        Some(record)
    }
}

fn canonical_pubkey(pubkey: &str) -> Result<String, String> {
    PublicKey::from_hex(pubkey)
        .map(|pk| pk.to_hex())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(request_id: Option<&str>, provider: &str) -> ZapReceiptRecord {
        ZapReceiptRecord {
            event_id: "receipt".to_string(),
            provider_pubkey: provider.to_string(),
            recipient_pubkey: "recipient".to_string(),
            zapped_event_id: Some("note".to_string()),
            zapped_address: None,
            zap_request_id: request_id.map(str::to_string),
            sender_pubkey: None,
            amount_msats: Some(1_000),
            bolt11: None,
            preimage: None,
            created_at: 1,
        }
    }

    #[test]
    fn unknown_request_is_allowed() {
        let registry = PendingZapRegistry::new();
        assert!(registry
            .validate_receipt(&record(Some("req-unknown"), &"b".repeat(64)))
            .is_ok());
    }

    #[test]
    fn matching_provider_is_allowed() {
        let registry = PendingZapRegistry::new();
        let provider = "a".repeat(64);
        registry
            .remember_expected_provider("req-match", &provider)
            .expect("valid provider");
        assert!(registry
            .validate_receipt(&record(Some("req-match"), &provider))
            .is_ok());
    }

    #[test]
    fn mismatched_provider_is_rejected() {
        let registry = PendingZapRegistry::new();
        registry
            .remember_expected_provider("req-mismatch", "a".repeat(64))
            .expect("valid provider");
        let err = registry
            .validate_receipt(&record(Some("req-mismatch"), &"b".repeat(64)))
            .expect_err("wrong provider must be rejected");
        assert_eq!(err.zap_request_id, "req-mismatch");
        assert_eq!(err.expected_provider_pubkey, "a".repeat(64));
        assert_eq!(err.actual_provider_pubkey, "b".repeat(64));
    }
}
