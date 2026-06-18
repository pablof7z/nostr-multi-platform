//! LMDB-error classifier for the NMP store open / write paths.
//!
//! Converts raw `heed::Error` / `nmp_nostr_lmdb::StoreError` values into the
//! typed `StoreError` variants introduced in #1521 so that store-open failures,
//! reader exhaustion, map-full, and migration failures are individually
//! testable and log-safe (D6/no-secrets: only fixed classification strings
//! and numeric limits appear in the output — never paths, event content, keys,
//! or other private data).
//!
//! This module is only compiled when the `lmdb-backend` feature is active
//! (it references heed / nmp_nostr_lmdb which are optional deps).

use heed::MdbError;

use crate::StoreError;

/// Classify a `nmp_nostr_lmdb::StoreError` (the error type returned by
/// `Lmdb::open_env` / `Lmdb::with_env`) into a typed `StoreError`.
///
/// `map_size` and `max_readers` are the limits that were passed to
/// `open_env`; they are embedded into `MapFull` / `ReaderExhaustion`
/// variants so the diagnostic is self-contained.
pub(super) fn classify_open_error(
    e: nmp_nostr_lmdb::StoreError,
    map_size: usize,
    max_readers: u32,
) -> StoreError {
    // nmp_nostr_lmdb::StoreError wraps heed::Error under the `Heed` variant.
    // Extract it so we can apply the same fine-grained classification as
    // `classify_heed_err`.  For any other variant (Io, Thread, etc.) fall
    // through to a bounded generic string — never forward the original
    // message which may contain file-system paths.
    match e {
        nmp_nostr_lmdb::StoreError::Heed(h) => classify_heed_direct(h, map_size, max_readers),
        _ => StoreError::Io("lmdb open failed (non-heed internal error)".into()),
    }
}

/// Classify a `heed::Error` into a typed `StoreError`.
///
/// `map_size` and `max_readers` are the limits used when the store was opened;
/// they are embedded verbatim into `MapFull` / `ReaderExhaustion` variants.
///
/// **D6/no-secrets**: the strings produced here are bounded fixed-text
/// classification labels and numeric limits.  No file-system paths, event ids,
/// content, pubkeys, or other private data are included.
pub(super) fn classify_heed_err(e: heed::Error, map_size: usize, max_readers: u32) -> StoreError {
    classify_heed_direct(e, map_size, max_readers)
}

/// Internal implementation shared by both public entry-points.
fn classify_heed_direct(e: heed::Error, map_size: usize, max_readers: u32) -> StoreError {
    match e {
        heed::Error::Mdb(mdb) => classify_mdb(mdb, map_size, max_readers),
        // Encoding / Decoding / DatabaseClosing / BadOpenOptions / Io all
        // fall through to a generic bounded string.  We do not forward the
        // original Display text because it may contain file-system paths.
        _ => StoreError::Io("lmdb i/o or encoding error".into()),
    }
}

/// Classify an `MdbError` (the fine-grained LMDB error enum) into a typed
/// `StoreError`.
///
/// The `MdbError` enum is exhaustively matched via an `Other(c_int)` catch-all
/// so this function is forward-compatible with future lmdb-sys additions
/// without a compile break.
fn classify_mdb(mdb: MdbError, map_size: usize, max_readers: u32) -> StoreError {
    match mdb {
        MdbError::ReadersFull => StoreError::ReaderExhaustion {
            max_readers,
        },
        MdbError::MapFull => StoreError::MapFull {
            map_size_bytes: map_size as u64,
        },
        MdbError::Corrupted | MdbError::Panic | MdbError::Invalid | MdbError::PageNotFound => {
            StoreError::CorruptEnv("lmdb environment corrupted or invalid".into())
        }
        MdbError::VersionMismatch => StoreError::VersionMismatch {
            detail: "lmdb binary version mismatch".into(),
        },
        // All other MdbError variants (NotFound, KeyExist, BadTxn, etc.) are
        // genuine i/o / protocol errors; map to a bounded generic string.
        // The `_` arm also covers any future `MdbError::Other(c_int)` additions.
        _ => StoreError::Io("lmdb mdb error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_full_maps_to_reader_exhaustion() {
        let e = heed::Error::Mdb(MdbError::ReadersFull);
        let result = classify_heed_err(e, 1024, 4);
        assert!(
            matches!(result, StoreError::ReaderExhaustion { max_readers: 4 }),
            "unexpected: {result:?}"
        );
    }

    #[test]
    fn map_full_maps_to_map_full() {
        let e = heed::Error::Mdb(MdbError::MapFull);
        let result = classify_heed_err(e, 8192, 10);
        assert!(
            matches!(result, StoreError::MapFull { map_size_bytes: 8192 }),
            "unexpected: {result:?}"
        );
    }

    #[test]
    fn corrupted_maps_to_corrupt_env() {
        for mdb in [MdbError::Corrupted, MdbError::Panic, MdbError::Invalid, MdbError::PageNotFound] {
            let e = heed::Error::Mdb(mdb);
            let result = classify_heed_err(e, 1024, 4);
            assert!(
                matches!(result, StoreError::CorruptEnv(_)),
                "expected CorruptEnv for {mdb:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn version_mismatch_maps_to_version_mismatch() {
        let e = heed::Error::Mdb(MdbError::VersionMismatch);
        let result = classify_heed_err(e, 1024, 4);
        assert!(
            matches!(result, StoreError::VersionMismatch { .. }),
            "unexpected: {result:?}"
        );
    }

    #[test]
    fn display_contains_no_sensitive_data() {
        // Verify that Display output for each new variant contains only
        // bounded classification text and numeric values — no paths/content.
        let cases: &[StoreError] = &[
            StoreError::ReaderExhaustion { max_readers: 126 },
            StoreError::MapFull { map_size_bytes: 1024 },
            StoreError::CorruptEnv("lmdb environment corrupted or invalid".into()),
            StoreError::VersionMismatch { detail: "lmdb binary version mismatch".into() },
        ];
        for err in cases {
            let msg = err.to_string();
            // Must not contain file separators or typical secret-looking tokens.
            assert!(!msg.contains('/'), "Display leaks path in: {msg}");
            assert!(!msg.contains("secret"), "Display leaks 'secret' in: {msg}");
        }
    }
}
