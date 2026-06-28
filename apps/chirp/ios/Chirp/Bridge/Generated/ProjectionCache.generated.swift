// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-cache \
//       --out apps/chirp/ios/Chirp/Bridge/Generated/ProjectionCache.generated.swift
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Swift differs.
//
// ADR-0055 R3-S3: NMP-owned rev-aware host apply layer. This cache implements
// the D3-3 merge algorithm exactly so app code (KernelModel accessors, views)
// stays byte-identical and oblivious to delta mechanics.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation
import os.log

private let pcLog = Logger(subsystem: "io.f7z.chirp", category: "ProjectionCache")

// MARK: - CacheEntry
/// One cached projection slot: the raw FlatBuffers payload bytes and
/// the last successfully committed `projectionRev`.
private struct CacheEntry {
    let rev: UInt64
    let schemaId: String
    let schemaVersion: UInt32
    let fileIdentifier: String
    let payload: Data
}

// MARK: - MergeResult
/// Return type of `ProjectionMergeCache.merge(frame:)`. Carries the
/// fully-merged envelope set (fed to the existing TypedXDecoder family)
/// and the set of projection keys whose rev advanced in this frame
/// (used by `KernelModel.apply` to skip unchanged @Published slots).
struct MergeResult {
    /// Fully-reconstituted envelope set: cached rows for omitted keys,
    /// freshly-decoded rows for Changed keys, nothing for Cleared keys.
    /// Feed this set to the existing TypedXDecoder.decode(from:) family.
    let mergedEnvelopes: [TypedProjectionEnvelope]
    /// Keys whose projectionRev advanced (Changed and committed) in this
    /// frame. Also includes Cleared keys so the caller can nil-out their
    /// @Published slots.
    let changedKeys: Set<String>
    /// Sticky flag: true when at least one decode-before-commit failed.
    /// The prior cache entry is retained (no silent corruption), but the
    /// host is known-degraded for that key. Rung 3 logs; Rung 4 resyncs.
    let needsResync: Bool
}

// MARK: - ProjectionMergeCache
/// NMP-owned rev-aware projection cache (ADR-0055 R3-S3).
///
/// Lives in `KernelHandle` (one instance per kernel app). Fed each
/// FlatBuffers frame before the TypedXDecoder family runs. Implements
/// the D3-3 merge algorithm exactly so app code is oblivious to delta
/// mechanics.
///
/// Thread-safety: called only from the NMP update callback
/// (`nmpUpdateCallback`), which fires on the Rust actor thread and is
/// always dispatched to `@MainActor` before `KernelModel.apply`. The
/// cache is NOT shared across threads.
final class ProjectionMergeCache {
    private var cache: [String: CacheEntry] = [:]
    private var appliedSession: UInt64 = 0
    private var appliedEpoch: UInt64 = 0
    /// D3-5: false until the first post-baseline frame is applied.
    /// UI should be gated on this being true.
    private(set) var baselined: Bool = false
    /// D3-4: latches true on any decode-before-commit failure.
    /// Rung 4 drains it via `nmp_app_request_full_snapshot`.
    private(set) var needsResync: Bool = false

    /// Hard-reset the cache (called from `KernelHandle.reset()` /
    /// `resetAndRestart()` so the next frame is treated as a full baseline).
    func reset() {
        cache.removeAll()
        appliedSession = 0
        appliedEpoch = 0
        baselined = false
        needsResync = false
    }

    // MARK: - Decode-before-commit dispatch
    //
    // Each projection key that has a Swift reader type gets a decode probe
    // here. Returns `true` iff the bytes decode successfully. We call the
    // `decode(bytes:)` entry point on the generated TypedXDecoder enum —
    // a non-nil result means the bytes are well-formed. On failure we keep
    // the prior cache entry rather than clobbering it with corrupt bytes.
    private static func decodeSucceeds(key: String, bytes: Data) -> Bool {
        guard !bytes.isEmpty else { return false }
        switch key {
        case "wallet": return TypedWalletDecoder.decode(bytes: bytes) != nil
        case "bunker_handshake": return TypedBunkerHandshakeDecoder.decode(bytes: bytes) != nil
        case "nip46_onboarding": return TypedNip46OnboardingDecoder.decode(bytes: bytes) != nil
        case "signer_state": return TypedSignerStateDecoder.decode(bytes: bytes) != nil
        case "publish_queue": return TypedPublishQueueDecoder.decode(bytes: bytes) != nil
        case "publish_outbox": return TypedPublishOutboxDecoder.decode(bytes: bytes) != nil
        case "outbox_summary": return TypedOutboxSummaryDecoder.decode(bytes: bytes) != nil
        case "configured_relays": return TypedConfiguredRelaysDecoder.decode(bytes: bytes) != nil
        case "relay_role_options": return TypedRelayRoleOptionsDecoder.decode(bytes: bytes) != nil
        case "accounts": return TypedAccountsDecoder.decode(bytes: bytes) != nil
        case "active_account": return TypedActiveAccountDecoder.decode(bytes: bytes) != nil
        case "action_results": return TypedActionResultsDecoder.decode(bytes: bytes) != nil
        case "action_stages": return TypedActionStagesDecoder.decode(bytes: bytes) != nil
        case "action_lifecycle": return TypedActionLifecycleDecoder.decode(bytes: bytes) != nil
        case "profile": return TypedProfileDecoder.decode(bytes: bytes) != nil
        case "nmp.nip29.group_events": return TypedGroupEventsDecoder.decode(bytes: bytes) != nil
        case "nmp.nip17.dm_inbox": return TypedDmInboxDecoder.decode(bytes: bytes) != nil
        case "nmp.follow_list": return TypedFollowListDecoder.decode(bytes: bytes) != nil
        case "nmp.nip29.discovered_groups": return TypedDiscoveredGroupsDecoder.decode(bytes: bytes) != nil
        case "nmp.nip29.group_defaults": return TypedGroupDefaultsDecoder.decode(bytes: bytes) != nil
        case "nmp.nip17.dm_relay_list": return TypedDmRelayListDecoder.decode(bytes: bytes) != nil
        case "relay_diagnostics": return TypedRelayDiagnosticsDecoder.decode(bytes: bytes) != nil
        case "refs.event.envelopes": return TypedRefEventEnvelopesDecoder.decode(bytes: bytes) != nil
        case "settings_hub": return TypedSettingsHubDecoder.decode(bytes: bytes) != nil
        case "nmp.marmot.snapshot": return TypedMarmotSnapshotDecoder.decode(bytes: bytes) != nil
        case "nmp.marmot.messages": return TypedMarmotMessagesDecoder.decode(bytes: bytes) != nil
        default:
            // Tier-1 host projections (feed, follow_list, etc.) that have no
            // swift_reader_type in the registry are always-Changed: we cannot
            // run a typed preflight, so accept them unconditionally.
            return true
        }
    }

    // MARK: - merge
    /// Run the D3-3 merge algorithm for one incoming frame.
    ///
    /// - If `sessionId` or `snapshotEpoch` changed ⇒ mandatory full-cache
    ///   reset (D4). The kernel guarantees the first post-change frame is a
    ///   full baseline.
    /// - `sessionId == 0` ⇒ no incremental contract (full frame anyway);
    ///   pass envelopes through unchanged without trusting omission (D3-5).
    /// - Changed row, rev > cached.rev ⇒ decode-before-commit; on success
    ///   overwrite cache; on failure keep prior + latch `needsResync`.
    /// - Cleared row ⇒ remove from cache; add key to changedKeys.
    /// - Omitted row (Unchanged) ⇒ retain cached value (no-op).
    ///
    /// Returns the fully-merged envelope set + changed-key set.
    func merge(
        envelopes: [TypedProjectionEnvelope],
        sessionId: UInt64,
        snapshotEpoch: UInt64
    ) -> MergeResult {
        // D3-5: session_id == 0 means no incremental contract.
        // Pass envelopes through as-is; do not trust omission.
        // changedKeys = all keys present (conservative: treat as fully changed).
        if sessionId == 0 {
            let keys = Set(envelopes.map(\.key))
            return MergeResult(mergedEnvelopes: envelopes, changedKeys: keys, needsResync: needsResync)
        }

        // D4: mandatory reset on session or epoch change.
        if sessionId != appliedSession || snapshotEpoch != appliedEpoch {
            cache.removeAll()
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = false
        }

        var changedKeys = Set<String>()

        // Run the merge algorithm over incoming rows.
        for envelope in envelopes {
            switch envelope.state {
            case .cleared:
                // Explicit clear: remove from cache, mark as changed
                // so the caller nils the @Published slot.
                cache.removeValue(forKey: envelope.key)
                changedKeys.insert(envelope.key)
            case .changed:
                let incomingRev = envelope.projectionRev
                // D3 reorder guard: under synchronous in-process delivery this
                // never fires, but belt-and-braces for future async transport.
                if let cached = cache[envelope.key], incomingRev <= cached.rev {
                    continue
                }
                // Decode-before-commit (D3-4): run the typed decoder as a
                // preflight. On success: overwrite cache + advance rev.
                // On failure: keep prior entry + latch needsResync.
                if Self.decodeSucceeds(key: envelope.key, bytes: envelope.payload) {
                    cache[envelope.key] = CacheEntry(
                        rev: incomingRev,
                        schemaId: envelope.schemaId,
                        schemaVersion: envelope.schemaVersion,
                        fileIdentifier: envelope.fileIdentifier,
                        payload: envelope.payload
                    )
                    changedKeys.insert(envelope.key)
                } else {
                    needsResync = true
                    pcLog.error("decode-before-commit failed for key=\(envelope.key, privacy: .public) rev=\(incomingRev, privacy: .public) — keeping prior cache entry, needsResync latched")
                }
            }
        }

        // Reconstruct the full merged envelope set from the cache.
        // Cleared keys are absent from the cache (already removed above),
        // so they are correctly absent from the merged set too.
        let mergedEnvelopes: [TypedProjectionEnvelope] = cache.map { key, entry in
            TypedProjectionEnvelope(
                key: key,
                schemaId: entry.schemaId,
                schemaVersion: entry.schemaVersion,
                fileIdentifier: entry.fileIdentifier,
                payload: entry.payload,
                projectionRev: entry.rev,
                state: .changed
            )
        }

        baselined = true
        return MergeResult(
            mergedEnvelopes: mergedEnvelopes,
            changedKeys: changedKeys,
            needsResync: needsResync
        )
    }
}
