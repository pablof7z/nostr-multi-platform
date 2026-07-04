//! Filesystem-backed [`WalletWalStore`] — mirrors `nmp-nip47`'s
//! `FsPaymentStore` durability discipline exactly.
//!
//! Directory layout, rooted at the app's configured `storage_path`:
//!
//! ```text
//! {storage_path}/wallet_operations/{account_pubkey}/{op_id}.json          (saga row)
//! {storage_path}/wallet_operations/{account_pubkey}/{op_id}.payload.json  (resume payload)
//! ```
//!
//! Every write is an atomic rename-over within the account directory so a
//! crash never leaves a half-written file. A single corrupt saga row is
//! skipped on load rather than failing the whole restore (D6 — a bad row must
//! not brick startup), exactly as `FsPaymentStore::load_unresolved` does. Both
//! the account pubkey and the operation id are percent-encoded before they
//! become path segments so neither can escape the store's directory.

use std::fs;
use std::path::PathBuf;

use super::wal::{WalError, WalletWalStore};
use super::{WalletOperation, WalletOperationId};

const WAL_DIR: &str = "wallet_operations";
const PAYLOAD_SUFFIX: &str = ".payload.json";

/// JSON-file-backed durable WAL store. One directory per account; one file per
/// saga row and (optionally) one payload file per operation.
pub struct FsWalletWalStore {
    path: PathBuf,
}

impl FsWalletWalStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn account_dir(&self, account: &str) -> PathBuf {
        self.path.join(WAL_DIR).join(encode_segment(account))
    }

    fn operation_path(&self, account: &str, id: &WalletOperationId) -> PathBuf {
        self.account_dir(account)
            .join(format!("{}.json", encode_segment(id.as_str())))
    }

    fn payload_path(&self, account: &str, id: &WalletOperationId) -> PathBuf {
        self.account_dir(account)
            .join(format!("{}{PAYLOAD_SUFFIX}", encode_segment(id.as_str())))
    }

    fn ensure_dir(&self, account: &str) -> Result<(), WalError> {
        let dir = self.account_dir(account);
        fs::create_dir_all(&dir)
            .map_err(|e| WalError::Backend(format!("create wal dir {}: {e}", dir.display())))
    }

    /// Atomic write: encode to a temp file in the same directory, then rename
    /// over the final path so a reader never observes a partial write.
    fn atomic_write(
        &self,
        account: &str,
        final_path: &PathBuf,
        tmp_name: &str,
        bytes: &[u8],
    ) -> Result<(), WalError> {
        self.ensure_dir(account)?;
        let tmp_path = self.account_dir(account).join(tmp_name);
        fs::write(&tmp_path, bytes)
            .map_err(|e| WalError::Backend(format!("write temp {}: {e}", tmp_path.display())))?;
        fs::rename(&tmp_path, final_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            WalError::Backend(format!("commit {}: {e}", final_path.display()))
        })
    }
}

impl WalletWalStore for FsWalletWalStore {
    fn upsert_operation(&self, account: &str, op: &WalletOperation) -> Result<(), WalError> {
        let bytes = serde_json::to_vec_pretty(op)
            .map_err(|e| WalError::Backend(format!("encode saga row: {e}")))?;
        let final_path = self.operation_path(account, &op.id);
        let tmp_name = format!(".{}.json.tmp", encode_segment(op.id.as_str()));
        self.atomic_write(account, &final_path, &tmp_name, &bytes)
    }

    fn delete_operation(&self, account: &str, id: &WalletOperationId) -> Result<(), WalError> {
        remove_file_idempotent(&self.operation_path(account, id))
    }

    fn load_operations(&self, account: &str) -> Result<Vec<WalletOperation>, WalError> {
        let dir = self.account_dir(account);
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(WalError::Backend(format!(
                    "read wal dir {}: {e}",
                    dir.display()
                )))
            }
        };
        let mut operations = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| WalError::Backend(format!("scan wal dir: {e}")))?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Saga rows only: skip payload files and anything that isn't a
            // `.json` (temp files, unrelated content).
            if name.ends_with(PAYLOAD_SUFFIX) {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            // Corrupt row: skip, never fatal — mirrors `FsPaymentStore`.
            let Ok(op) = serde_json::from_slice::<WalletOperation>(&bytes) else {
                continue;
            };
            operations.push(op);
        }
        operations.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        Ok(operations)
    }

    fn upsert_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
        bytes: &[u8],
    ) -> Result<(), WalError> {
        let final_path = self.payload_path(account, id);
        let tmp_name = format!(".{}{PAYLOAD_SUFFIX}.tmp", encode_segment(id.as_str()));
        self.atomic_write(account, &final_path, &tmp_name, bytes)
    }

    fn load_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
    ) -> Result<Option<Vec<u8>>, WalError> {
        let path = self.payload_path(account, id);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(WalError::Backend(format!(
                "read payload {}: {e}",
                path.display()
            ))),
        }
    }

    fn delete_payload(&self, account: &str, id: &WalletOperationId) -> Result<bool, WalError> {
        let path = self.payload_path(account, id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(WalError::Backend(format!(
                "delete payload {}: {e}",
                path.display()
            ))),
        }
    }
}

/// Delete a file, treating a missing file as success (idempotent).
fn remove_file_idempotent(path: &PathBuf) -> Result<(), WalError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(WalError::Backend(format!(
            "delete {}: {e}",
            path.display()
        ))),
    }
}

/// Percent-encode any byte that is not filesystem-safe so an account pubkey or
/// operation id can never escape the store directory or collide with `.`/`..`.
/// Identical discipline to `FsPaymentStore::encode_id`.
fn encode_segment(segment: &str) -> String {
    if segment == "." || segment == ".." {
        return segment.bytes().map(|b| format!("%{b:02X}")).collect();
    }
    let mut out = String::with_capacity(segment.len());
    for &byte in segment.as_bytes() {
        let safe = byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
        if safe {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
#[path = "wal_fs_tests.rs"]
mod tests;
