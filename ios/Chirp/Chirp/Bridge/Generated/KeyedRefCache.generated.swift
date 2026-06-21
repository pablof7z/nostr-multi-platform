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

    /// Decode-before-commit seam (ADR-0063 invariant #2). The per-namespace
    /// typed-row validator the cache runs on a `Changed` row's payload BEFORE it
    /// replaces a slot: it returns true iff `payload` decodes to the namespace's
    /// concrete ref type (`refs.profile` → ProfileRef/ProfileCard, `refs.event`
    /// → EventEmbed). The default accepts any non-empty payload; Lane C injects
    /// the real decoder here. A row that fails is NOT committed — the prior row
    /// is retained and `needsResync` latches.
    var rowDecoder: (_ namespace: String, _ payload: Data) -> Bool = { _, payload in
        !payload.isEmpty
    }

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
    /// Invariants: absent row == Unchanged (retained); explicit Cleared removes
    /// (rev-safe); decode-before-commit per row (malformed row keeps prior +
    /// latches needsResync); session/epoch change or `baseline` rebuilds the
    /// full set. A baseline commits atomically (scratch-then-commit). A garbage
    /// batch fails closed (CHECKED decode) — prior cache retained, resync latched.
    @discardableResult
    func merge(projectionKey: String, payload: Data, sessionId: UInt64, snapshotEpoch: UInt64) -> Set<String> {
        guard let namespace = Self.namespace(forProjectionKey: projectionKey) else {
            return []
        }

        // D4: mandatory full reset on session/epoch change, before any merge.
        if sessionId != appliedSession || snapshotEpoch != appliedEpoch {
            rows.removeAll()
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = false
        }

        // Fail-closed CHECKED decode at BATCH grain: verify the `NRRD`
        // file_identifier AND run the FlatBuffers verifier BEFORE any cache
        // mutation. Empty, wrong-file-id, or structurally-invalid bytes retain
        // the prior cache + latch needsResync rather than trapping.
        guard !payload.isEmpty else {
            needsResync = true
            return []
        }
        var buffer = ByteBuffer(data: payload)
        let batch: nmp_refs_RefRowDeltaBatch
        do {
            batch = try getCheckedRoot(byteBuffer: &buffer, fileId: nmp_refs_RefRowDeltaBatch.id)
        } catch {
            needsResync = true
            krcLog.error("malformed RefRowDeltaBatch for projection=\(projectionKey, privacy: .public) — retaining prior cache, needsResync latched: \(String(describing: error), privacy: .public)")
            return []
        }

        if batch.baseline {
            return applyBaseline(projectionKey: projectionKey, namespace: namespace, batch: batch)
        }
        return applyIncremental(projectionKey: projectionKey, namespace: namespace, batch: batch)
    }

    /// Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
    /// WHOLE batch): decode every required row into a scratch map first and
    /// replace the projection only after all rows decode. One bad row fails the
    /// entire baseline closed — the prior cache is preserved, needsResync latches.
    private func applyBaseline(projectionKey: String, namespace: String, batch: nmp_refs_RefRowDeltaBatch) -> Set<String> {
        var scratch: [String: RefRowCacheEntry] = [:]
        for row in batch.rows {
            guard let key = row.key else { continue }
            if row.state.rawValue == kRefRowStateCleared {
                // A defensive Cleared inside a baseline just means the key is
                // absent from the rebuilt set.
                scratch.removeValue(forKey: key)
                continue
            }
            let bytes = Data(row.payload)
            // Decode-before-commit per row via the typed seam; ANY failure fails
            // the whole baseline closed — prior cache intact, resync latched.
            if bytes.isEmpty || !rowDecoder(namespace, bytes) {
                needsResync = true
                krcLog.error("decode-before-commit failed in baseline for projection=\(projectionKey, privacy: .public) key=\(key, privacy: .public) — preserving prior cache, needsResync latched")
                return []
            }
            // Duplicate-key guard within one baseline: last-rev wins.
            if let existing = scratch[key], row.rev <= existing.rev { continue }
            scratch[key] = RefRowCacheEntry(rev: row.rev, payload: bytes)
        }

        // Atomic commit: diff prior vs scratch so exactly the changed slots
        // re-render (added / updated / dropped ghost), then swap the projection.
        let prior = rows[projectionKey] ?? [:]
        var changed = Set<String>()
        for (key, entry) in scratch where prior[key]?.payload != entry.payload {
            changed.insert(key)
        }
        for key in prior.keys where scratch[key] == nil {
            changed.insert(key)
        }
        rows[projectionKey] = scratch
        baselined = true
        for key in changed {
            rowChanged.send(KeyedRowChange(projectionKey: projectionKey, rowKey: key, cleared: scratch[key] == nil))
        }
        return changed
    }

    /// Steady-state incremental merge with rev-safe clears and the per-row
    /// decode-before-commit seam.
    private func applyIncremental(projectionKey: String, namespace: String, batch: nmp_refs_RefRowDeltaBatch) -> Set<String> {
        var ns = rows[projectionKey] ?? [:]
        var changed = Set<String>()

        for row in batch.rows {
            guard let key = row.key else { continue }
            if row.state.rawValue == kRefRowStateCleared {
                // Rev-safe clear: remove only if the clear's rev is NEWER than
                // the cached row, so a stale reordered clear can never delete a
                // newer live row. A clear for an absent key is a no-op.
                if let cached = ns[key], row.rev > cached.rev {
                    ns.removeValue(forKey: key)
                    changed.insert(key)
                    rowChanged.send(KeyedRowChange(projectionKey: projectionKey, rowKey: key, cleared: true))
                }
                continue
            }
            // Changed. Reorder/duplicate guard: skip a row not newer than cached.
            let incomingRev = row.rev
            if let cached = ns[key], incomingRev <= cached.rev { continue }
            // Decode-before-commit per row (invariant #2) via the typed seam:
            // empty OR invalid bytes → keep the prior row, latch needsResync.
            let bytes = Data(row.payload)
            if bytes.isEmpty || !rowDecoder(namespace, bytes) {
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
