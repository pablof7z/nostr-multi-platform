// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache \
//       --out ios/Chirp/Chirp/Bridge/Generated/KeyedRefCache.generated.swift
//
// Source of truth: KEYED_PROJECTIONS in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Swift differs.
//
// ADR-0063 Lane A (#1671): per-key row cache for keyed reference projections
// (`refs.profile` / `refs.event`). Decodes the `nmp.refs.RefRowDeltaBatch`
// payload and merges row deltas under the five invariants — byte-for-byte
// semantically identical to `nmp_core::refs::RefRowCache` and the Kotlin twin.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation
import Combine
import FlatBuffers
import os.log

private let krcLog = Logger(subsystem: "io.f7z.chirp", category: "KeyedRefCache")

// MARK: - RefRowState (mirror of nmp.refs.RefRowState)
private let kRefRowStateCleared: UInt8 = 1

// MARK: - Types
/// One cached row: the last committed per-key rev and the raw typed payload.
private struct RefRowCacheEntry {
    let rev: UInt64
    let payload: Data
}

/// A per-row change event published when one key commits or clears. A view
/// subscribes filtered on `(projectionKey, rowKey)` so exactly one re-renders.
struct KeyedRowChange: Equatable {
    let projectionKey: String
    let rowKey: String
    /// True when the row was Cleared (removed); false when it committed a value.
    let cleared: Bool
}

// MARK: - KeyedRefCache
/// NMP-owned per-key row cache for keyed reference projections (ADR-0063).
///
/// Thread-safety: fed only from the NMP update callback dispatched to
/// `@MainActor`, identical to `ProjectionMergeCache`.
final class KeyedRefCache {
    /// `projectionKey -> (rowKey -> entry)`.
    private var rows: [String: [String: RefRowCacheEntry]] = [:]
    private var appliedSession: UInt64 = 0
    private var appliedEpoch: UInt64 = 0
    /// D3-5: false until the first post-baseline frame is applied.
    private(set) var baselined: Bool = false
    /// D3-4: latches on any per-row decode-before-commit failure.
    private(set) var needsResync: Bool = false
    /// Per-row change publisher (one event per changed key).
    let rowChanged = PassthroughSubject<KeyedRowChange, Never>()

    /// Hard-reset (kernel session end) so the next frame is a full baseline.
    func reset() {
        rows.removeAll()
        appliedSession = 0
        appliedEpoch = 0
        baselined = false
        needsResync = false
    }

    /// Map a frame's `TypedProjection.key` to its resolver namespace.
    /// Returns nil for a non-keyed projection (the merge is a no-op).
    static func namespace(forProjectionKey key: String) -> String? {
        switch key {
        case "refs.profile": return "profile"
        case "refs.event": return "event"
        default: return nil
        }
    }

    // MARK: - merge
    /// Merge one keyed-projection payload (`nmp.refs.RefRowDeltaBatch` bytes)
    /// under the frame's `sessionId` / `snapshotEpoch`. Returns the set of row
    /// keys whose cached row changed (committed or cleared) this frame.
    ///
    /// Invariants: absent row == Unchanged (retained); explicit Cleared removes;
    /// decode-before-commit per row (malformed row keeps prior + latches
    /// needsResync); session/epoch change or `baseline` rebuilds the full set.
    @discardableResult
    func merge(projectionKey: String, payload: Data, sessionId: UInt64, snapshotEpoch: UInt64) -> Set<String> {
        guard let namespace = Self.namespace(forProjectionKey: projectionKey) else {
            return []
        }
        _ = namespace // routing validated; the cache is keyed by projectionKey

        // D4: mandatory full reset on session/epoch change, before any merge.
        if sessionId != appliedSession || snapshotEpoch != appliedEpoch {
            rows.removeAll()
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = false
        }

        // Decode-before-commit at BATCH grain: a malformed batch fails closed
        // (retain everything, latch resync) rather than corrupting the cache.
        guard !payload.isEmpty else {
            needsResync = true
            return []
        }
        var buffer = ByteBuffer(data: payload)
        let batch: nmp_refs_RefRowDeltaBatch = getRoot(byteBuffer: &buffer)

        // A baseline batch reconstructs its projection wholesale (invariant #3).
        if batch.baseline {
            rows[projectionKey] = [:]
        }
        var ns = rows[projectionKey] ?? [:]
        var changed = Set<String>()

        for row in batch.rows {
            guard let key = row.key else { continue }
            if row.state.rawValue == kRefRowStateCleared {
                // Explicit clear: remove unconditionally.
                if ns.removeValue(forKey: key) != nil {
                    changed.insert(key)
                    rowChanged.send(KeyedRowChange(projectionKey: projectionKey, rowKey: key, cleared: true))
                }
                continue
            }
            // Changed. Reorder/duplicate guard: skip a row not newer than cached.
            let incomingRev = row.rev
            if let cached = ns[key], incomingRev <= cached.rev { continue }
            // Decode-before-commit per row (invariant #2): a Changed row by
            // contract carries non-empty bytes; empty == malformed → keep prior.
            let bytes = Data(row.payload)
            if bytes.isEmpty {
                needsResync = true
                krcLog.error("decode-before-commit failed for projection=\(projectionKey, privacy: .public) key=\(key, privacy: .public) rev=\(incomingRev, privacy: .public) — keeping prior row, needsResync latched")
                continue
            }
            ns[key] = RefRowCacheEntry(rev: incomingRev, payload: bytes)
            changed.insert(key)
            rowChanged.send(KeyedRowChange(projectionKey: projectionKey, rowKey: key, cleared: false))
        }

        rows[projectionKey] = ns
        baselined = true
        return changed
    }

    /// The cached raw payload bytes for one `(projectionKey, rowKey)`, or nil.
    func payload(projectionKey: String, rowKey: String) -> Data? {
        rows[projectionKey]?[rowKey]?.payload
    }

    /// The number of cached rows for a projection (test/diagnostic aid).
    func count(projectionKey: String) -> Int {
        rows[projectionKey]?.count ?? 0
    }

    // MARK: - Per-key accessors
    //
    // One typed accessor per keyed namespace. A view binds
    // `profile(pubkey)` (raw `RefRowDeltaBatch` row payload bytes — the
    // caller decodes with the namespace's typed reader) and subscribes to
    // `rowChanged` filtered on its key, so exactly one view re-renders
    // when that key updates.
    func profile(_ key: String) -> Data? { payload(projectionKey: "refs.profile", rowKey: key) }
    func event(_ key: String) -> Data? { payload(projectionKey: "refs.event", rowKey: key) }
}
