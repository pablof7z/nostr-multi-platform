//! LMDB encode/decode layer for the `relay-author-scores-v1` sub-db (W2).
//!
//! # Key format (§8.9)
//! `[32 raw pubkey bytes][1 byte URL-length u8][N URL bytes]`
//!
//! URLs > 255 bytes cannot be keyed (the length field is a `u8`). Such rows
//! are silently skipped with a `tracing::warn!` at the flush call-site
//! (`kernel/relay_score_flush.rs`). The `encode_key` helper returns `None`
//! for oversized URLs so callers can log before skipping.
//!
//! # Value format (§8.9)
//! 24-byte fixed-width record:
//! `[u32 successes BE][u32 failures BE][u64 last_used_unix_s BE][u64 _reserved BE]`
//!
//! # URL canonicalization (§8.10)
//! URLs are canonicalized by the caller (`kernel/relay_score_flush.rs`)
//! using `CanonicalRelayUrl::parse_or_raw` before being handed to this
//! layer. This module stores and retrieves raw bytes without interpreting them.
//!
//! # Crate-graph constraint
//! `nmp-store` does NOT depend on `nmp-core` or `nmp-planner`. All types here
//! are raw primitives: `[u8;32]`, `String`, `(u32, u32, u64)`.

/// LMDB sub-db name for the relay-author-score table.
///
/// Changing this string is a schema-bump: the old table becomes invisible and
/// a new empty one is created on next open (§5 E6).
pub const SUB_DB_NAME: &str = "relay-author-scores-v1";

/// Encode a `(pubkey_bytes, relay_url)` pair into the LMDB key.
///
/// Returns `None` when `relay_url.len() > 255` (the `u8` length prefix limit).
/// Callers should `tracing::warn!` and skip the row in that case.
#[inline]
#[must_use]
pub fn encode_key(pubkey: &[u8; 32], relay_url: &str) -> Option<Vec<u8>> {
    let url_bytes = relay_url.as_bytes();
    let url_len = url_bytes.len();
    if url_len > 255 {
        return None;
    }
    let mut key = Vec::with_capacity(32 + 1 + url_len);
    key.extend_from_slice(pubkey);
    key.push(url_len as u8);
    key.extend_from_slice(url_bytes);
    Some(key)
}

/// Decode a raw LMDB key back into `([u8;32], String)`.
///
/// Returns `None` on any malformed key (wrong length, invalid UTF-8).
/// D6: callers skip malformed rows without panicking.
#[must_use]
fn decode_key(key: &[u8]) -> Option<([u8; 32], String)> {
    if key.len() < 33 {
        return None;
    }
    let pubkey: [u8; 32] = key[..32].try_into().ok()?;
    let url_len = key[32] as usize;
    if key.len() != 32 + 1 + url_len {
        return None;
    }
    let url = std::str::from_utf8(&key[33..]).ok()?.to_string();
    Some((pubkey, url))
}

/// Encode a score value into the 24-byte LMDB value.
#[inline]
#[must_use]
fn encode_value(successes: u32, failures: u32, last_used_unix_s: u64) -> [u8; 24] {
    let mut val = [0u8; 24];
    val[0..4].copy_from_slice(&successes.to_be_bytes());
    val[4..8].copy_from_slice(&failures.to_be_bytes());
    val[8..16].copy_from_slice(&last_used_unix_s.to_be_bytes());
    // bytes 16..24 are the reserved field — zero.
    val
}

/// Decode the 24-byte LMDB value. Returns `None` on wrong length.
#[must_use]
fn decode_value(val: &[u8]) -> Option<(u32, u32, u64)> {
    if val.len() != 24 {
        return None;
    }
    let successes = u32::from_be_bytes(val[0..4].try_into().ok()?);
    let failures = u32::from_be_bytes(val[4..8].try_into().ok()?);
    let last_used_unix_s = u64::from_be_bytes(val[8..16].try_into().ok()?);
    Some((successes, failures, last_used_unix_s))
}

// ─── Public(crate) access helpers ────────────────────────────────────────────

/// Load all score rows from the default sub-db name (`relay-author-scores-v1`).
pub fn load_all_raw(
    store: &crate::LmdbEventStore,
) -> Result<Vec<([u8; 32], String, u32, u32, u64)>, crate::StoreError> {
    load_all_raw_with_name(store, SUB_DB_NAME)
}

/// Load all score rows from a named sub-db (parameterized for schema-bump tests).
pub fn load_all_raw_with_name(
    store: &crate::LmdbEventStore,
    db_name: &str,
) -> Result<Vec<([u8; 32], String, u32, u32, u64)>, crate::StoreError> {
    use heed::types::Bytes;

    let inner = &store.inner;
    let env = &inner.env;

    // Open the named sub-db in a read txn — it's fine if the db doesn't exist;
    // we treat that as an empty result.
    let db_opt = {
        let rtxn = env
            .read_txn()
            .map_err(|e| crate::StoreError::Io(format!("read_txn: {e}")))?;
        env.open_database::<Bytes, Bytes>(&rtxn, Some(db_name))
            .map_err(|e| crate::StoreError::Io(format!("open_database {db_name}: {e}")))?
    };

    let Some(db) = db_opt else {
        return Ok(Vec::new());
    };

    let rtxn = env
        .read_txn()
        .map_err(|e| crate::StoreError::Io(format!("read_txn: {e}")))?;

    let mut out = Vec::new();
    for item in db
        .iter(&rtxn)
        .map_err(|e| crate::StoreError::Io(format!("iter: {e}")))?
    {
        let (k, v) = item.map_err(|e| crate::StoreError::Io(format!("iter item: {e}")))?;
        let Some((pubkey, url)) = decode_key(k) else {
            tracing::warn!("relay-scores: skipping malformed key (len={})", k.len());
            continue;
        };
        let Some((s, f, ts)) = decode_value(v) else {
            tracing::warn!("relay-scores: skipping malformed value (len={})", v.len());
            continue;
        };
        out.push((pubkey, url, s, f, ts));
    }
    Ok(out)
}

/// Persist a batch of score cells to the default sub-db.
///
/// Rows whose URL exceeds 255 bytes are silently skipped (logged by the
/// caller). Within one call this is an all-or-nothing LMDB write txn:
/// an LMDB error rolls back the txn and propagates as `StoreError::Io`.
pub fn put_batch_raw(
    store: &crate::LmdbEventStore,
    cells: Vec<([u8; 32], String, u32, u32, u64)>,
) -> Result<(), crate::StoreError> {
    use heed::types::Bytes;

    if cells.is_empty() {
        return Ok(());
    }

    let inner = &store.inner;
    let env = &inner.env;
    let db = &inner.relay_author_scores;

    let mut wtxn = env
        .write_txn()
        .map_err(|e| crate::StoreError::Io(format!("write_txn: {e}")))?;

    for (pubkey, url, successes, failures, last_used_unix_s) in cells {
        let Some(key) = encode_key(&pubkey, &url) else {
            // URL > 255 bytes — skip silently (caller logs).
            continue;
        };
        let value = encode_value(successes, failures, last_used_unix_s);
        db.put(&mut wtxn, key.as_slice(), &value)
            .map_err(|e| crate::StoreError::Io(format!("put: {e}")))?;
    }

    wtxn.commit()
        .map_err(|e| crate::StoreError::Io(format!("commit: {e}")))?;

    Ok(())
}
