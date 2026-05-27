//! JSON-file-backed [`PublishStore`].
//!
//! `PublishEngine::resume_from_store` replays whatever `load_pending` returns
//! after a kernel restart - so offline-composed publish intents only survive an
//! app kill if the store itself is durable. The other durable impl,
//! [`super::store::DomainPublishStore`], is durable *only* when the crate is
//! built with `--features lmdb-backend`; without that feature it sits on top of
//! the in-memory `MemEventStore` and loses everything on restart.
//!
//! `FsPublishStore` closes that gap with a dependency-free, feature-flag-free
//! backing store: one JSON file per publish intent under
//! `{path}/publish_intents/`. Publish intents are few (one per offline compose)
//! and small (a single signed event plus per-relay state), so a directory of
//! small JSON files is simpler and more portable than dragging in LMDB.
//!
//! Durability contract:
//! - `upsert` writes to a temp file in the *same directory* and `rename`s it
//!   over the target - `rename` within one directory is atomic on POSIX and
//!   NTFS, so a reader (or a crash) never observes a half-written intent.
//! - `delete` removes the file; a missing file is treated as success
//!   (idempotent - the engine may delete a terminal intent more than once).
//! - `load_pending` scans the directory and skips any file that fails to
//!   deserialize rather than aborting the whole resume (D6: a single corrupt
//!   row must not brick startup).

use std::fs;
use std::path::PathBuf;

use super::action::PublishHandle;
use super::traits::{PublishRecord, PublishStore, PublishStoreError};

/// Subdirectory (under the supplied storage path) that holds one JSON file per
/// pending publish intent. Lives alongside the LMDB event store so a single
/// `storage_path` covers all durable kernel state.
const INTENTS_DIR: &str = "publish_intents";

/// JSON-file-backed durable [`PublishStore`].
///
/// Each [`PublishRecord`] is one file: `{path}/publish_intents/{handle}.json`.
/// The directory is created lazily on first write so constructing the store is
/// pure (no I/O, no failure) - matching the other store constructors.
pub struct FsPublishStore {
    /// Root storage directory (the same path handed to the LMDB event store).
    /// The `publish_intents` subdirectory hangs off this.
    path: PathBuf,
}

impl FsPublishStore {
    /// Construct a store rooted at `path`. `path` is the storage directory the
    /// host supplied via `nmp_app_set_storage_path` - the `publish_intents`
    /// subdirectory is created the first time `upsert` runs.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The `publish_intents` directory under the storage root.
    fn intents_dir(&self) -> PathBuf {
        self.path.join(INTENTS_DIR)
    }

    /// Absolute path of the JSON file for `handle`.
    fn record_path(&self, handle: &PublishHandle) -> PathBuf {
        self.intents_dir()
            .join(format!("{}.json", encode_handle(handle)))
    }

    /// Create the `publish_intents` directory if it does not exist yet.
    fn ensure_dir(&self) -> Result<(), PublishStoreError> {
        let dir = self.intents_dir();
        fs::create_dir_all(&dir).map_err(|err| {
            PublishStoreError::Backend(format!(
                "create publish_intents dir {}: {err}",
                dir.display()
            ))
        })
    }
}

impl PublishStore for FsPublishStore {
    fn upsert(&self, record: &PublishRecord) -> Result<(), PublishStoreError> {
        self.ensure_dir()?;
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|err| PublishStoreError::Backend(format!("encode publish record: {err}")))?;

        let final_path = self.record_path(&record.handle);
        // Temp file in the SAME directory as the target so the rename below is
        // a within-directory (atomic) rename, not a cross-filesystem copy.
        // The temp name is keyed on the handle so two concurrent upserts of
        // different intents never collide on the same temp path.
        let tmp_path = self
            .intents_dir()
            .join(format!(".{}.json.tmp", encode_handle(&record.handle)));

        fs::write(&tmp_path, &bytes).map_err(|err| {
            PublishStoreError::Backend(format!(
                "write temp publish record {}: {err}",
                tmp_path.display()
            ))
        })?;
        fs::rename(&tmp_path, &final_path).map_err(|err| {
            // Best-effort cleanup so a failed rename does not leak the temp.
            let _ = fs::remove_file(&tmp_path);
            PublishStoreError::Backend(format!(
                "commit publish record {}: {err}",
                final_path.display()
            ))
        })
    }

    fn delete(&self, handle: &PublishHandle) -> Result<(), PublishStoreError> {
        let path = self.record_path(handle);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            // Idempotent: the engine deletes a terminal intent and may replay
            // that delete after restart - a missing file is not an error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(PublishStoreError::Backend(format!(
                "delete publish record {}: {err}",
                path.display()
            ))),
        }
    }

    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError> {
        let dir = self.intents_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // No directory yet -> nothing was ever persisted -> empty resume.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(err) => {
                return Err(PublishStoreError::Backend(format!(
                    "read publish_intents dir {}: {err}",
                    dir.display()
                )))
            }
        };

        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                PublishStoreError::Backend(format!("scan publish_intents dir: {err}"))
            })?;
            let path = entry.path();
            // Only committed records - skip in-flight `.tmp` files and any
            // non-`.json` debris a host might drop in the directory.
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                // Unreadable file (race with delete, permissions) - skip it
                // rather than abort the whole resume.
                continue;
            };
            let Ok(record) = serde_json::from_slice::<PublishRecord>(&bytes) else {
                // D6: a single corrupt intent must not brick startup.
                continue;
            };
            // Mirror `InMemoryPublishStore`/`DomainPublishStore`: only intents
            // with at least one non-terminal relay are still "pending".
            if record
                .per_relay
                .iter()
                .any(|(_, state)| !state.is_terminal())
            {
                records.push(record);
            }
        }
        records.sort_by(|a, b| a.handle.cmp(&b.handle));
        Ok(records)
    }
}

/// Encode a [`PublishHandle`] into a filesystem-safe filename stem.
///
/// In production the handle is the signed event id - 64 lowercase hex chars,
/// already safe (see `kernel::publish_engine::run_publish_engine`). But the
/// `PublishStore` trait types the handle as a bare `String`, so a defensive,
/// dependency-free encoding keeps the store correct for *any* handle:
///
/// - `[A-Za-z0-9._-]` pass through unchanged (covers every real event id).
/// - everything else (`/`, `..` segments, spaces, unicode) is percent-encoded
///   as `%XX` over the UTF-8 bytes - collision-free and traversal-proof.
/// - a handle that is exactly `.` or `..` is escaped so it can never name the
///   directory itself or its parent.
fn encode_handle(handle: &str) -> String {
    if handle == "." || handle == ".." {
        // Escape the dots so the filename is a literal, not a path segment.
        return handle.bytes().map(|b| format!("%{b:02X}")).collect();
    }
    let mut out = String::with_capacity(handle.len());
    for &byte in handle.as_bytes() {
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
    use crate::publish::PerRelayState;
    use crate::substrate::{SignedEvent, UnsignedEvent};

    fn record(handle: &str, state: PerRelayState) -> PublishRecord {
        PublishRecord {
            handle: handle.to_string(),
            event: SignedEvent {
                id: format!("{handle:0<64}"),
                sig: "a".repeat(128),
                unsigned: UnsignedEvent {
                    pubkey: "b".repeat(64),
                    kind: 1,
                    tags: Vec::new(),
                    content: "offline publish".to_string(),
                    created_at: 1_700_000_000,
                },
            },
            per_relay: vec![("wss://relay.test".to_string(), state)],
            pending_retries: Vec::new(),
            relay_reasons: Vec::new(),
        }
    }

    /// `upsert` then `load_pending` on the SAME store returns the record, and
    /// terminal records are filtered out - matching the in-memory contract.
    #[test]
    fn fs_publish_store_writes_and_reads_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsPublishStore::new(dir.path());

        store
            .upsert(&record("pending", PerRelayState::Pending))
            .expect("write pending record");
        store
            .upsert(&record(
                "done",
                PerRelayState::Ok {
                    acked_at_ms: 1_700_000_000_000,
                },
            ))
            .expect("write terminal record");

        let pending = store.load_pending().expect("load pending records");
        assert_eq!(pending.len(), 1, "terminal record must be filtered out");
        assert_eq!(pending[0].handle, "pending");
        assert_eq!(pending[0].event.unsigned.content, "offline publish");

        store
            .delete(&"pending".to_string())
            .expect("delete pending");
        assert!(
            store
                .load_pending()
                .expect("pending after delete")
                .is_empty(),
            "store must be empty after deleting the only pending intent"
        );
    }

    /// The durability guarantee: a record written by one `FsPublishStore`
    /// instance is visible to a *fresh* instance pointing at the same path -
    /// this is exactly the app-restart path `resume_from_store` depends on.
    #[test]
    fn fs_publish_store_survives_new_instance_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");

        {
            // First "process": persist an offline-composed intent.
            let writer = FsPublishStore::new(dir.path());
            writer
                .upsert(&record("offline-intent", PerRelayState::Pending))
                .expect("write intent");
        }

        {
            // Second "process": a brand-new store over the same directory
            // must replay the intent - proving it survived "app kill".
            let reader = FsPublishStore::new(dir.path());
            let pending = reader.load_pending().expect("load after restart");
            assert_eq!(pending.len(), 1, "intent must survive a restart");
            assert_eq!(pending[0].handle, "offline-intent");
            assert_eq!(
                pending[0].per_relay[0].1,
                PerRelayState::Pending,
                "per-relay state must round-trip intact"
            );
        }
    }

    /// `load_pending` on a never-written store (no `publish_intents` dir yet)
    /// is an empty resume, not an error.
    #[test]
    fn fs_publish_store_load_pending_empty_when_unwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsPublishStore::new(dir.path());
        assert!(store.load_pending().expect("load with no dir").is_empty());
    }

    /// `delete` of a never-written handle is a no-op success (idempotent).
    #[test]
    fn fs_publish_store_delete_missing_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsPublishStore::new(dir.path());
        store
            .delete(&"never-existed".to_string())
            .expect("delete of missing handle must succeed");
    }

    /// A handle containing path-traversal characters is encoded into a single
    /// safe filename inside `publish_intents/` - it never escapes the dir.
    #[test]
    fn fs_publish_store_handles_unsafe_handle_chars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsPublishStore::new(dir.path());
        let nasty = "../../etc/passwd";

        store
            .upsert(&record(nasty, PerRelayState::Pending))
            .expect("write record with traversal chars");

        // The encoded file lives directly under publish_intents/, nowhere else.
        let intents = dir.path().join(INTENTS_DIR);
        let files: Vec<_> = fs::read_dir(&intents)
            .expect("read intents dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(files.len(), 1, "exactly one encoded file, no traversal");

        let pending = store.load_pending().expect("load pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].handle, nasty,
            "the original handle round-trips inside the record body"
        );
    }
}
