//! Actor-owned NIP-44 decrypt-session port result vocabulary.
//!
//! These types are signer-capability outcomes, not DM/backfill domain state.
//! Unsupported capability responses are data so a caller can choose the scalar
//! fallback without parsing an error string. Secret-bearing fields are redacted
//! from `Debug`.

use std::collections::BTreeMap;

use nmp_signer_iface::{
    Nip44DecryptBatchItemResult, Nip44DecryptBatchResult, Nip44DecryptSessionGrant, SignerError,
};

#[cfg(all(test, feature = "native"))]
#[path = "nip44_decrypt_session_port_tests.rs"]
mod tests;

/// Outcome of a decrypt-session begin request.
#[derive(Clone, Eq, PartialEq)]
pub enum Nip44DecryptSessionBeginPortResult {
    /// The signer granted a scoped session.
    Granted(Nip44DecryptSessionGrant),
    /// The selected signer does not support the optional session extension.
    Unsupported { reason: String },
}

impl std::fmt::Debug for Nip44DecryptSessionBeginPortResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Granted(grant) => f.debug_tuple("Granted").field(grant).finish(),
            Self::Unsupported { reason } => f
                .debug_struct("Unsupported")
                .field("reason", reason)
                .finish(),
        }
    }
}

/// Outcome of a batch decrypt request.
#[derive(Clone, Eq, PartialEq)]
pub enum Nip44DecryptBatchPortResult {
    /// The signer returned one result for every requested item.
    Batch(Nip44DecryptBatchPortOutcome),
    /// The selected signer does not support batch decrypt sessions.
    Unsupported { reason: String },
}

impl std::fmt::Debug for Nip44DecryptBatchPortResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Batch(outcome) => f.debug_tuple("Batch").field(outcome).finish(),
            Self::Unsupported { reason } => f
                .debug_struct("Unsupported")
                .field("reason", reason)
                .finish(),
        }
    }
}

/// Validated per-batch decrypt outcome.
#[derive(Clone, Eq, PartialEq)]
pub struct Nip44DecryptBatchPortOutcome {
    pub items: Vec<Nip44DecryptBatchItemPortOutcome>,
}

impl std::fmt::Debug for Nip44DecryptBatchPortOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nip44DecryptBatchPortOutcome")
            .field("items_len", &self.items.len())
            .finish()
    }
}

/// One validated batch item outcome.
#[derive(Clone, Eq, PartialEq)]
pub enum Nip44DecryptBatchItemPortOutcome {
    Plaintext { id: String, plaintext: String },
    Failed { id: String, error: String },
}

impl std::fmt::Debug for Nip44DecryptBatchItemPortOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plaintext { id, .. } => f
                .debug_struct("Plaintext")
                .field("id", id)
                .field("plaintext", &"[redacted]")
                .finish(),
            Self::Failed { id, error } => f
                .debug_struct("Failed")
                .field("id", id)
                .field("error", error)
                .finish(),
        }
    }
}

/// Outcome of a decrypt-session end request.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Nip44DecryptSessionEndPortResult {
    Ended { acknowledged: bool },
    Unsupported { reason: String },
}

/// Boxed continuation for begin outcomes.
pub struct Nip44DecryptSessionBeginContinuation(
    pub Box<dyn FnOnce(Result<Nip44DecryptSessionBeginPortResult, String>) + Send>,
);

impl Nip44DecryptSessionBeginContinuation {
    #[must_use]
    pub fn new(
        f: impl FnOnce(Result<Nip44DecryptSessionBeginPortResult, String>) + Send + 'static,
    ) -> Self {
        Self(Box::new(f))
    }

    pub fn call(self, outcome: Result<Nip44DecryptSessionBeginPortResult, String>) {
        (self.0)(outcome);
    }
}

impl std::fmt::Debug for Nip44DecryptSessionBeginContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nip44DecryptSessionBeginContinuation(<nip44 session begin continuation>)")
    }
}

/// Boxed continuation for batch outcomes.
pub struct Nip44DecryptBatchContinuation(
    pub Box<dyn FnOnce(Result<Nip44DecryptBatchPortResult, String>) + Send>,
);

impl Nip44DecryptBatchContinuation {
    #[must_use]
    pub fn new(
        f: impl FnOnce(Result<Nip44DecryptBatchPortResult, String>) + Send + 'static,
    ) -> Self {
        Self(Box::new(f))
    }

    pub fn call(self, outcome: Result<Nip44DecryptBatchPortResult, String>) {
        (self.0)(outcome);
    }
}

impl std::fmt::Debug for Nip44DecryptBatchContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nip44DecryptBatchContinuation(<nip44 batch continuation>)")
    }
}

/// Boxed continuation for session-end outcomes.
pub struct Nip44DecryptSessionEndContinuation(
    pub Box<dyn FnOnce(Result<Nip44DecryptSessionEndPortResult, String>) + Send>,
);

impl Nip44DecryptSessionEndContinuation {
    #[must_use]
    pub fn new(
        f: impl FnOnce(Result<Nip44DecryptSessionEndPortResult, String>) + Send + 'static,
    ) -> Self {
        Self(Box::new(f))
    }

    pub fn call(self, outcome: Result<Nip44DecryptSessionEndPortResult, String>) {
        (self.0)(outcome);
    }
}

impl std::fmt::Debug for Nip44DecryptSessionEndContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nip44DecryptSessionEndContinuation(<nip44 session end continuation>)")
    }
}

pub(crate) fn map_begin_result(
    result: Result<Nip44DecryptSessionGrant, SignerError>,
) -> Result<Nip44DecryptSessionBeginPortResult, String> {
    match result {
        Ok(grant) => Ok(Nip44DecryptSessionBeginPortResult::Granted(grant)),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptSessionBeginPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn map_batch_result(
    result: Result<Nip44DecryptBatchResult, SignerError>,
    expected_ids: &[String],
) -> Result<Nip44DecryptBatchPortResult, String> {
    match result {
        Ok(batch) => validate_batch(batch, expected_ids).map(Nip44DecryptBatchPortResult::Batch),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptBatchPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn map_end_result(
    result: Result<bool, SignerError>,
) -> Result<Nip44DecryptSessionEndPortResult, String> {
    match result {
        Ok(acknowledged) => Ok(Nip44DecryptSessionEndPortResult::Ended { acknowledged }),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptSessionEndPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

fn validate_batch(
    batch: Nip44DecryptBatchResult,
    expected_ids: &[String],
) -> Result<Nip44DecryptBatchPortOutcome, String> {
    if batch.items.len() != expected_ids.len() {
        return Err(format!(
            "malformed nip44 decrypt batch result: expected {} item(s), got {}",
            expected_ids.len(),
            batch.items.len()
        ));
    }
    let mut by_id = BTreeMap::new();
    for item in batch.items {
        validate_item_shape(&item)?;
        if !expected_ids.iter().any(|expected| expected == &item.id) {
            return Err(format!(
                "malformed nip44 decrypt batch result: unexpected item id {}",
                item.id
            ));
        }
        if by_id.insert(item.id.clone(), item).is_some() {
            return Err("malformed nip44 decrypt batch result: duplicate item id".to_string());
        }
    }
    let mut items = Vec::with_capacity(expected_ids.len());
    for id in expected_ids {
        let item = by_id
            .get(id)
            .ok_or_else(|| format!("malformed nip44 decrypt batch result: missing item id {id}"))?;
        let outcome = match (&item.plaintext, &item.error) {
            (Some(plaintext), None) => Nip44DecryptBatchItemPortOutcome::Plaintext {
                id: id.clone(),
                plaintext: plaintext.clone(),
            },
            (None, Some(error)) => Nip44DecryptBatchItemPortOutcome::Failed {
                id: id.clone(),
                error: error.clone(),
            },
            _ => {
                return Err(format!(
                    "malformed nip44 decrypt batch item {id}: expected exactly one of plaintext or error"
                ));
            }
        };
        items.push(outcome);
    }
    Ok(Nip44DecryptBatchPortOutcome { items })
}

fn validate_item_shape(item: &Nip44DecryptBatchItemResult) -> Result<(), String> {
    match (&item.plaintext, &item.error) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        _ => Err(format!(
            "malformed nip44 decrypt batch item {}: expected exactly one of plaintext or error",
            item.id
        )),
    }
}
