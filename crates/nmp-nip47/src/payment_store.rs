//! Durable per-payment record store.
//!
//! Fixes the double-pay vector: by writing the record BEFORE the kind:23194
//! EVENT frame is enqueued, and by transitioning through Unknown on TTL/disconnect
//! (never directly to Failed), we preserve the ability to reconcile via
//! `lookup_invoice` on reconnect — so a user shown "failed" cannot mint a fresh
//! invoice and pay twice.
//!
//! Durability contract mirrors `FsPublishStore`: atomic rename-over within the
//! same directory, one JSON file per record, corrupt files skipped on load (D6).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const PAYMENTS_DIR: &str = "pending_payments";

/// Lifecycle state of a payment record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentState {
    /// Kind:23194 sent; no response yet.
    PaySent,
    /// Terminal: wallet confirmed payment.
    Succeeded,
    /// Terminal: wallet definitively rejected (PAYMENT_FAILED etc).
    Failed,
    /// Outcome unknown: TTL elapsed or disconnect before response arrived.
    /// Must be reconciled via `lookup_invoice` on reconnect.
    Unknown,
}

impl PaymentState {
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}

/// One record for a `pay_invoice` request. Written to disk BEFORE the
/// kind:23194 frame is enqueued.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PaymentRecord {
    /// The signed kind:23194 event id — the key in `pending_payments`.
    pub request_event_id: String,
    /// The bolt11 invoice string. Used as the `lookup_invoice` param (NIP-47
    /// accepts `invoice` OR `payment_hash`; storing the bolt11 avoids needing
    /// a BOLT11 tagged-field decoder in this crate).
    pub bolt11: String,
    /// Registry-minted action correlation id. `None` for actor-internal
    /// auto-dispatched payments that have no host spinner.
    pub correlation_id: Option<String>,
    /// Amount in millisatoshis (from `amount_msats` param or decoded from HRP).
    pub amount_msats: Option<u64>,
    /// Current lifecycle state.
    pub state: PaymentState,
    /// Payment preimage, set on `Succeeded`.
    pub preimage: Option<String>,
}

/// JSON-file-backed durable payment store.
///
/// Each record lives at `{storage_path}/pending_payments/{request_event_id}.json`.
/// The directory is created lazily on first write. Load on startup replays
/// `PaySent` and `Unknown` records for reconciliation.
pub struct FsPaymentStore {
    path: PathBuf,
}

#[derive(Debug)]
pub enum PaymentStoreError {
    Backend(String),
}

impl std::fmt::Display for PaymentStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "payment store: {msg}"),
        }
    }
}

impl std::error::Error for PaymentStoreError {}

impl FsPaymentStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn payments_dir(&self) -> PathBuf {
        self.path.join(PAYMENTS_DIR)
    }

    fn record_path(&self, request_event_id: &str) -> PathBuf {
        self.payments_dir()
            .join(format!("{}.json", encode_id(request_event_id)))
    }

    fn ensure_dir(&self) -> Result<(), PaymentStoreError> {
        let dir = self.payments_dir();
        fs::create_dir_all(&dir).map_err(|e| {
            PaymentStoreError::Backend(format!(
                "create pending_payments dir {}: {e}",
                dir.display()
            ))
        })
    }

    /// Write (insert-or-update) a payment record. Atomic rename-over within the
    /// `pending_payments` directory so a crash never leaves a half-written file.
    pub fn upsert(&self, record: &PaymentRecord) -> Result<(), PaymentStoreError> {
        self.ensure_dir()?;
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|e| PaymentStoreError::Backend(format!("encode payment record: {e}")))?;
        let final_path = self.record_path(&record.request_event_id);
        let tmp_path = self
            .payments_dir()
            .join(format!(".{}.json.tmp", encode_id(&record.request_event_id)));
        fs::write(&tmp_path, &bytes).map_err(|e| {
            PaymentStoreError::Backend(format!(
                "write temp payment record {}: {e}",
                tmp_path.display()
            ))
        })?;
        fs::rename(&tmp_path, &final_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            PaymentStoreError::Backend(format!(
                "commit payment record {}: {e}",
                final_path.display()
            ))
        })
    }

    /// Delete a record by request event id. A missing file is success
    /// (idempotent — terminal records may be deleted more than once).
    pub fn delete(&self, request_event_id: &str) -> Result<(), PaymentStoreError> {
        let path = self.record_path(request_event_id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(PaymentStoreError::Backend(format!(
                "delete payment record {}: {e}",
                path.display()
            ))),
        }
    }

    /// Load all non-terminal (`PaySent` and `Unknown`) records — the ones that
    /// still need reconciliation. A single corrupt file is skipped, not fatal
    /// (D6 — a bad row must not brick startup).
    pub fn load_unresolved(&self) -> Result<Vec<PaymentRecord>, PaymentStoreError> {
        let dir = self.payments_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(PaymentStoreError::Backend(format!(
                    "read pending_payments dir {}: {e}",
                    dir.display()
                )))
            }
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| PaymentStoreError::Backend(format!("scan dir: {e}")))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_slice::<PaymentRecord>(&bytes) else {
                continue;
            };
            if !record.state.is_terminal() {
                records.push(record);
            }
        }
        records.sort_by(|a, b| a.request_event_id.cmp(&b.request_event_id));
        Ok(records)
    }
}

/// Percent-encode any byte that is not filesystem-safe so an event id can never
/// escape the `pending_payments` directory or collide with `.`/`..`.
fn encode_id(id: &str) -> String {
    if id == "." || id == ".." {
        return id.bytes().map(|b| format!("%{b:02X}")).collect();
    }
    let mut out = String::with_capacity(id.len());
    for &byte in id.as_bytes() {
        let safe = byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-';
        if safe {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, bolt11: &str, state: PaymentState) -> PaymentRecord {
        PaymentRecord {
            request_event_id: id.to_string(),
            bolt11: bolt11.to_string(),
            correlation_id: Some(format!("cid-{id}")),
            amount_msats: Some(1_000),
            state,
            preimage: None,
        }
    }

    #[test]
    fn upsert_and_load_pay_sent() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsPaymentStore::new(dir.path());
        store
            .upsert(&record("abc123", "lnbc1...", PaymentState::PaySent))
            .unwrap();
        let pending = store.load_unresolved().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_event_id, "abc123");
    }

    #[test]
    fn terminal_records_filtered_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsPaymentStore::new(dir.path());
        store
            .upsert(&record("ok1", "lnbc1...", PaymentState::Succeeded))
            .unwrap();
        store
            .upsert(&record("ok2", "lnbc2...", PaymentState::Failed))
            .unwrap();
        store
            .upsert(&record("pending", "lnbc3...", PaymentState::PaySent))
            .unwrap();
        let pending = store.load_unresolved().unwrap();
        assert_eq!(pending.len(), 1, "only non-terminal records returned");
        assert_eq!(pending[0].request_event_id, "pending");
    }

    #[test]
    fn unknown_records_returned_for_reconciliation() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsPaymentStore::new(dir.path());
        store
            .upsert(&record("unk1", "lnbc1...", PaymentState::Unknown))
            .unwrap();
        let pending = store.load_unresolved().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].state, PaymentState::Unknown);
    }

    #[test]
    fn survives_new_instance_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = FsPaymentStore::new(dir.path());
            s.upsert(&record("r1", "lnbc...", PaymentState::PaySent))
                .unwrap();
        }
        {
            let s = FsPaymentStore::new(dir.path());
            let v = s.load_unresolved().unwrap();
            assert_eq!(v.len(), 1, "record survives process restart");
        }
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let s = FsPaymentStore::new(dir.path());
        s.delete("nonexistent").unwrap(); // must not error
    }

    #[test]
    fn upsert_transitions_pay_sent_to_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let s = FsPaymentStore::new(dir.path());
        s.upsert(&record("r1", "lnbc...", PaymentState::PaySent))
            .unwrap();
        let mut r = s.load_unresolved().unwrap().into_iter().next().unwrap();
        r.state = PaymentState::Unknown;
        s.upsert(&r).unwrap();
        let v = s.load_unresolved().unwrap();
        assert_eq!(v[0].state, PaymentState::Unknown);
    }
}
