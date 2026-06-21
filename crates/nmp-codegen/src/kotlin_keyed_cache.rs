//! ADR-0063 Lane A (#1671) — generated per-key (row-keyed) reference cache for
//! Kotlin (Android). The Android twin of [`crate::swift_keyed_cache`]: it
//! decodes the `nmp.refs.RefRowDeltaBatch` payload and merges row deltas under
//! the five invariants, byte-for-byte semantically identical to the Swift cache
//! and the Rust reference model (`nmp_core::refs::RefRowCache`).
//!
//! Generates `android/app/src/main/java/org/nmp/android/KeyedRefCache.kt` from
//! [`KEYED_PROJECTIONS`].

use std::path::Path;

use crate::swift_projections_registry::{KeyedProjectionEntry, KEYED_PROJECTIONS};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache --platform kotlin \\
//       --out android/app/src/main/java/org/nmp/android/KeyedRefCache.kt
//
// Source of truth: KEYED_PROJECTIONS in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Kotlin differs.
//
// ADR-0063 Lane A (#1671): per-key row cache for keyed reference projections
// (`refs.profile` / `refs.event`) — byte-for-byte semantically identical to
// `KeyedRefCache.generated.swift` and `nmp_core::refs::RefRowCache`.
// ─────────────────────────────────────────────────────────────────────────────

@file:OptIn(ExperimentalUnsignedTypes::class)

package org.nmp.android

import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import nmp.refs.RefRowDeltaBatch
import nmp.refs.RefRowState

private const val KRC_TAG = \"KeyedRefCache\"

";

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct KotlinKeyedRefCacheCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the Kotlin `KeyedRefCache` source from the keyed-projection registry.
#[must_use]
pub fn render_kotlin_keyed_ref_cache(entries: &[KeyedProjectionEntry]) -> String {
    let mut out = String::from(HEADER);
    out.push_str(STATIC_TYPES);
    out.push_str(&render_routing(entries));
    out.push_str(STATIC_MERGE);
    out.push_str(&render_accessors(entries));
    out.push_str("}\n");
    out
}

fn render_routing(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    /** Map a frame's `TypedProjection.key` to its resolver namespace. */\n\
         \x20\x20\x20\x20private fun namespace(forProjectionKey: String): String? = when (forProjectionKey) {\n",
    );
    for e in entries {
        s.push_str(&format!(
            "        {:?} -> {:?}\n",
            e.projection_key, e.namespace
        ));
    }
    s.push_str(
        "        else -> null\n\
         \x20\x20\x20\x20}\n\n",
    );
    s
}

fn render_accessors(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    // Per-key accessors — one per keyed namespace. A composable reads\n\
         \x20\x20\x20\x20// `profile(pubkey)` (raw row payload bytes; the caller decodes with the\n\
         \x20\x20\x20\x20// namespace's typed reader) and observes `rowChanges` filtered on its\n\
         \x20\x20\x20\x20// key so exactly one composable recomposes when that key updates.\n",
    );
    for e in entries {
        s.push_str(&format!(
            "    fun {}(key: String): ByteArray? = payload({:?}, key)\n",
            e.accessor, e.projection_key
        ));
    }
    s
}

const STATIC_TYPES: &str = r#"/** One cached row: last committed per-key rev + raw typed payload bytes. */
private data class RefRowCacheEntry(val rev: ULong, val payload: ByteArray) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is RefRowCacheEntry) return false
        return rev == other.rev && payload.contentEquals(other.payload)
    }
    override fun hashCode(): Int = 31 * rev.hashCode() + payload.contentHashCode()
}

/** A per-row change: one is delivered to listeners per committed/cleared key. */
data class KeyedRowChange(val projectionKey: String, val rowKey: String, val cleared: Boolean)

/**
 * NMP-owned per-key row cache for keyed reference projections (ADR-0063).
 *
 * Thread-safety: fed only from `KernelModel.applyFrame` on the single native
 * update-listener thread, identical to `ProjectionMergeCache`.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class KeyedRefCache {
    // projectionKey -> (rowKey -> entry)
    private val rows = HashMap<String, HashMap<String, RefRowCacheEntry>>()
    private var appliedSession: ULong = 0UL
    private var appliedEpoch: ULong = 0UL
    /** D3-5: false until the first post-baseline frame is applied. */
    var baselined: Boolean = false
        private set
    /** D3-4: latches on any per-row decode-before-commit failure. */
    var needsResync: Boolean = false
        private set
    private val rowChangeListeners = mutableListOf<(KeyedRowChange) -> Unit>()

    /** Register a per-row change listener (one call per committed/cleared key). */
    fun addRowChangeListener(listener: (KeyedRowChange) -> Unit) {
        rowChangeListeners.add(listener)
    }

    /** Hard-reset so the next frame is a full baseline. */
    fun reset() {
        rows.clear()
        appliedSession = 0UL
        appliedEpoch = 0UL
        baselined = false
        needsResync = false
    }

"#;

const STATIC_MERGE: &str = r#"    /**
     * Merge one keyed-projection payload (`nmp.refs.RefRowDeltaBatch` bytes)
     * under the frame's session/epoch. Returns the row keys whose cached row
     * changed (committed or cleared) this frame.
     *
     * Invariants: absent row == Unchanged (retained); explicit Cleared removes;
     * decode-before-commit per row (malformed keeps prior + latches
     * needsResync); session/epoch change or `baseline` rebuilds the full set.
     */
    fun merge(projectionKey: String, payload: ByteArray, sessionId: ULong, snapshotEpoch: ULong): Set<String> {
        if (namespace(forProjectionKey = projectionKey) == null) return emptySet()

        // D4: mandatory full reset on session/epoch change, before any merge.
        if (sessionId != appliedSession || snapshotEpoch != appliedEpoch) {
            rows.clear()
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = false
        }

        // Decode-before-commit at BATCH grain: a malformed/empty batch fails
        // closed (retain everything, latch resync) rather than corrupting cache.
        if (payload.isEmpty()) {
            needsResync = true
            return emptySet()
        }
        val bb = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val batch = RefRowDeltaBatch.getRootAsRefRowDeltaBatch(bb)

        // A baseline batch reconstructs its projection wholesale (invariant #3).
        if (batch.baseline) {
            rows[projectionKey] = HashMap()
        }
        val ns = rows.getOrPut(projectionKey) { HashMap() }
        val changed = mutableSetOf<String>()

        for (i in 0 until batch.rowsLength) {
            val row = batch.rows(i) ?: continue
            val key = row.key ?: continue
            if (row.state == RefRowState.Cleared) {
                // Explicit clear: remove unconditionally.
                if (ns.remove(key) != null) {
                    changed.add(key)
                    notifyRowChange(KeyedRowChange(projectionKey, key, cleared = true))
                }
                continue
            }
            // Changed. Reorder/duplicate guard: skip a row not newer than cached.
            val incomingRev = row.rev
            val cached = ns[key]
            if (cached != null && incomingRev <= cached.rev) continue
            // Decode-before-commit per row (invariant #2): empty == malformed.
            val bytes = ByteArray(row.payloadLength) { j -> row.payload(j).toByte() }
            if (bytes.isEmpty()) {
                needsResync = true
                Log.e(KRC_TAG, "decode-before-commit failed for projection=$projectionKey key=$key rev=$incomingRev — keeping prior row, needsResync latched")
                continue
            }
            ns[key] = RefRowCacheEntry(incomingRev, bytes)
            changed.add(key)
            notifyRowChange(KeyedRowChange(projectionKey, key, cleared = false))
        }

        baselined = true
        return changed
    }

    private fun notifyRowChange(change: KeyedRowChange) {
        for (listener in rowChangeListeners) listener(change)
    }

    /** The cached raw payload bytes for one (projectionKey, rowKey), or null. */
    fun payload(projectionKey: String, rowKey: String): ByteArray? =
        rows[projectionKey]?.get(rowKey)?.payload

    /** The number of cached rows for a projection (test/diagnostic aid). */
    fun count(projectionKey: String): Int = rows[projectionKey]?.size ?: 0

"#;

/// Write the generated Kotlin file to `out_path`.
pub fn generate_kotlin_keyed_ref_cache(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
pub fn check_kotlin_keyed_ref_cache(
    out_path: &Path,
) -> std::io::Result<KotlinKeyedRefCacheCheckOutcome> {
    let rendered = render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(KotlinKeyedRefCacheCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(KotlinKeyedRefCacheCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(KotlinKeyedRefCacheCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "kotlin_keyed_cache_tests.rs"]
mod tests;
