// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache \
//       --out apps/chirp/ios/Chirp/Bridge/Generated/KeyedRefCache.generated.swift
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

    /// ADR-0063 Lane C: wire the real typed decoder at construction so the
    /// decode-before-commit seam validates every `Changed` row against the
    /// namespace's concrete type (no caller setup required).
    init() {
        installTypedRowDecoder()
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

        // D4: an identity (session/epoch) change demands a full rebuild. We do
        // NOT clear here — clearing before the new baseline decodes would empty a
        // live cache on a malformed first frame (fail-open). The reset is
        // DEFERRED into the baseline commit (scratch-then-commit), so the prior
        // cache survives a garbage baseline after an identity bump (BLOCKING-1).
        let identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch

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

        // Whole-batch fail-closed validation (BLOCKING-2/3): a row with NO key,
        // OR an out-of-range `state` discriminant (anything other than
        // Changed=0 / Cleared=1), rejects the ENTIRE batch — prior cache
        // retained, needsResync latched, NOTHING committed. No row is skipped.
        //
        // The state discriminant MUST be read as the RAW on-wire byte, NOT via
        // the flatc typed accessor `row.state` — that accessor coerces any
        // unknown raw byte to `.changed` (`nmp_refs_RefRowState(rawValue:) ??
        // .changed`), so an on-wire `state=255` would silently become Changed=0
        // and slip past a `> Cleared` guard (fail-open). We therefore re-read
        // the raw `state` discriminant byte for every row straight from the
        // FlatBuffer using the public `Table` accessor, mirroring the Kotlin
        // twin which reads `row.state` as a raw `UByte`. A buffer whose shape we
        // cannot re-walk also fails closed.
        guard let rawStates = Self.rawRowStateDiscriminants(&buffer) else {
            needsResync = true
            krcLog.error("RefRowDeltaBatch raw-state scan failed for projection=\(projectionKey, privacy: .public) — rejecting whole batch, needsResync latched")
            return []
        }
        var rowIndex = 0
        for row in batch.rows {
            if row.key == nil {
                needsResync = true
                krcLog.error("RefRowDeltaBatch row missing key for projection=\(projectionKey, privacy: .public) — rejecting whole batch, needsResync latched")
                return []
            }
            // RAW discriminant byte (NOT the coerced typed `row.state`): reject
            // anything outside {Changed=0, Cleared=1} for the WHOLE batch.
            let rawState = rowIndex < rawStates.count ? rawStates[rowIndex] : UInt8.max
            if rawState > kRefRowStateCleared {
                needsResync = true
                krcLog.error("RefRowDeltaBatch row has unknown raw state=\(rawState, privacy: .public) for projection=\(projectionKey, privacy: .public) — rejecting whole batch, needsResync latched")
                return []
            }
            rowIndex += 1
        }
        // The raw scan must observe exactly as many rows as the typed vector; a
        // mismatch means the buffer shape is inconsistent — fail closed.
        if rawStates.count != batch.rows.count {
            needsResync = true
            krcLog.error("RefRowDeltaBatch raw-state count mismatch for projection=\(projectionKey, privacy: .public) — rejecting whole batch, needsResync latched")
            return []
        }

        if batch.baseline {
            return applyBaseline(projectionKey: projectionKey, namespace: namespace, batch: batch, identityChanged: identityChanged, sessionId: sessionId, snapshotEpoch: snapshotEpoch)
        }
        // A non-baseline batch under a changed identity cannot rebuild the full
        // set; fail closed (adopt identity, latch resync, retain prior cache)
        // rather than merge deltas onto a stale-epoch base. The producer always
        // follows an identity bump with a baseline frame.
        if identityChanged {
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = true
            return []
        }
        return applyIncremental(projectionKey: projectionKey, namespace: namespace, batch: batch)
    }

    /// Read the RAW `state` discriminant byte for EVERY row directly from the
    /// FlatBuffer, BYPASSING the flatc typed accessor (`nmp_refs_RefRow.state`),
    /// which coerces any unknown raw value to `.changed` via
    /// `nmp_refs_RefRowState(rawValue:) ?? .changed` and would therefore mask an
    /// on-wire `state=255` as Changed=0. This is the host-side mirror of the
    /// Kotlin twin, whose flatc accessor exposes `row.state` as a raw `UByte`.
    ///
    /// It re-walks the verified buffer with the public `FlatBuffers.Table` API:
    /// resolve the root table, find the `rows` vector (vtable offset 8 on
    /// `RefRowDeltaBatch`), then for each row table read the `state` field
    /// (vtable offset 8 on `RefRow`) as a raw `UInt8` (default 0 when the field
    /// is absent — a legitimately omitted scalar is Changed). Returns the raw
    /// discriminants in row order, or nil if the buffer cannot be re-walked
    /// (caller fails the batch closed). Must be called only AFTER the CHECKED
    /// root decode has verified the buffer.
    private static func rawRowStateDiscriminants(_ buffer: inout ByteBuffer) -> [UInt8]? {
        // Resolve the root table position exactly as `getRoot`/`getCheckedRoot`
        // do: the UOffset at the reader points to the root table.
        let rootPosition = Int32(buffer.read(def: UOffset.self, position: buffer.reader)) &+ Int32(buffer.reader)
        let root = Table(bb: buffer, position: rootPosition)
        // `RefRowDeltaBatch.rows` is vtable offset 8. An absent vector is an
        // empty batch (zero rows) — valid, no discriminants to check.
        let rowsField = root.offset(8)
        if rowsField == 0 { return [] }
        let count = Int(root.vector(count: rowsField))
        if count < 0 { return nil }
        let start = root.vector(at: rowsField)
        var states = [UInt8]()
        states.reserveCapacity(count)
        for i in 0..<count {
            // Each rows[] element is a 4-byte indirect (UOffset) to a RefRow
            // table; dereference it to the row's absolute position.
            let elementOffset = start &+ Int32(i &* 4)
            let rowPosition = elementOffset &+ Int32(buffer.read(def: Int32.self, position: Int(elementOffset)))
            let row = Table(bb: buffer, position: rowPosition)
            // `RefRow.state` is vtable offset 8; read the RAW byte (no enum
            // coercion). An absent scalar field defaults to 0 (Changed).
            let stateField = row.offset(8)
            states.append(stateField == 0 ? 0 : row.readBuffer(of: UInt8.self, at: stateField))
        }
        return states
    }

    /// Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
    /// WHOLE batch): decode every required row into a scratch map first and
    /// replace the projection only after all rows decode. One bad row fails the
    /// entire baseline closed — the prior cache is preserved, needsResync latches.
    ///
    /// When `identityChanged` is set this is the FIRST baseline at a new
    /// session/epoch: on a SUCCESSFUL decode it flips identity and drops every
    /// OTHER projection's prior-epoch rows as part of the atomic commit; on a
    /// decode FAILURE it touches nothing (BLOCKING-1, fail-closed).
    private func applyBaseline(projectionKey: String, namespace: String, batch: nmp_refs_RefRowDeltaBatch, identityChanged: Bool, sessionId: UInt64, snapshotEpoch: UInt64) -> Set<String> {
        var scratch: [String: RefRowCacheEntry] = [:]
        for row in batch.rows {
            // Key + state already validated whole-batch in `merge`.
            let key = row.key!
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

        // Decode succeeded → now (and ONLY now) it is safe to mutate state. On an
        // identity change this is where the DEFERRED reset lands: adopt the new
        // identity and drop every OTHER projection's prior-epoch rows, then
        // treat the prior-epoch slot as gone so every scratch row counts as new.
        if identityChanged {
            for k in rows.keys where k != projectionKey { rows.removeValue(forKey: k) }
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            needsResync = false
        }

        // Atomic commit: diff prior vs scratch so exactly the changed slots
        // re-render (added / updated / dropped ghost), then swap the projection.
        let prior = identityChanged ? [:] : (rows[projectionKey] ?? [:])
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
            // Key + state already validated whole-batch in `merge`.
            let key = row.key!
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

    // MARK: - Typed row decode (ADR-0063 Lane C, #1671)
    //
    // The real per-namespace typed decoders that replace Lane A's
    // raw-bytes passthrough. Each does a CHECKED root decode of the row
    // payload buffer (verifying its OWN file_identifier — KPRF / KCEV —
    // NOT the NRRD batch id) then maps the reader to the Chirp domain
    // type via the hand-written `TypedProjectionGlue`. Invariant #2:
    // `installTypedRowDecoder()` wires these into the decode-before-commit
    // seam so a row only commits if it decodes to the concrete type.
    /// Decode one `KPRF` row payload buffer into `ProfileCard` (ADR-0063 Lane C).
    private func decodeProfileRow(bytes: Data) -> ProfileCard? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ProfileSnapshot
        do {
            reader = try getCheckedRoot(byteBuffer: &buffer, fileId: nmp_kernel_ProfileSnapshot.id)
        } catch {
            return nil
        }
        // Hand-written glue (NOT generated): reader → domain type.
        // See `TypedProjectionGlue.profile`.
        return TypedProjectionGlue.profile(reader)
    }
    /// Decode one `KCEV` row payload buffer into `ClaimedEventDto` (ADR-0063 Lane C).
    private func decodeEventRow(bytes: Data) -> ClaimedEventDto? {
        guard !bytes.isEmpty else { return nil }
        var buffer = ByteBuffer(data: bytes)
        let reader: nmp_kernel_ClaimedEventsSnapshot
        do {
            reader = try getCheckedRoot(byteBuffer: &buffer, fileId: nmp_kernel_ClaimedEventsSnapshot.id)
        } catch {
            return nil
        }
        // Hand-written glue (NOT generated): reader → domain type.
        // See `TypedProjectionGlue.refRowEvent`.
        return TypedProjectionGlue.refRowEvent(reader)
    }
    /// Wire the real typed decode-before-commit seam (ADR-0063 #2): a
    /// `Changed` row commits ONLY if its payload decodes to the
    /// namespace's concrete type. Called from `init` so the typed
    /// contract holds without any caller wiring.
    private func installTypedRowDecoder() {
        rowDecoder = { [weak self] namespace, payload in
            guard let self else { return false }
            switch namespace {
            case "profile": return self.decodeProfileRow(bytes: payload) != nil
            case "event": return self.decodeEventRow(bytes: payload) != nil
            default: return false
            }
        }
    }
    // MARK: - Per-key TYPED accessors (ADR-0063 Lane C, #1671)
    //
    // One TYPED accessor per keyed namespace — the #1671 part-(b) host
    // per-key reactive read API. A view binds `model.profile(pubkey)` and
    // subscribes to `rowChanged` filtered on its key, so exactly one
    // `AvatarView(pubkey:)` re-renders when that pubkey's row updates. The
    // accessor DECODES the cached row-payload buffer through the
    // namespace's typed reader (the SAME buffer the kernel's
    // `ref_*_row_payload` encoder emits) into the concrete domain type —
    // NOT the Lane-A raw `Data` passthrough. A decode miss returns nil.
    func profile(_ key: String) -> ProfileCard? {
        guard let bytes = payload(projectionKey: "refs.profile", rowKey: key) else { return nil }
        return decodeProfileRow(bytes: bytes)
    }
    func event(_ key: String) -> ClaimedEventDto? {
        guard let bytes = payload(projectionKey: "refs.event", rowKey: key) else { return nil }
        return decodeEventRow(bytes: bytes)
    }
}
