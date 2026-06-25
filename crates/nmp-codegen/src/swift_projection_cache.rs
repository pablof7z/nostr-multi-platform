//! R3-S3 (ADR-0055) — generated `ProjectionMergeCache` for Swift (iOS).
//!
//! Generates `apps/chirp/ios/Chirp/Bridge/Generated/ProjectionCache.generated.swift`
//! from the SAME projection registry as [`crate::swift_typed_decoders`], so the
//! cache can never drift from the decoder set.
//!
//! ## The generated cache (D3-3 algorithm)
//!
//! The cache holds a `[String: CacheEntry]` map keyed by projection key. Each
//! `CacheEntry` carries the raw payload bytes and the last committed rev. On
//! every frame the cache runs the merge algorithm:
//!
//! - `Changed` row, rev > cached.rev → decode-before-commit; on success
//!   overwrite cache + advance rev; on failure keep prior + latch
//!   `needsResync`.
//! - `Cleared` row → remove from cache.
//! - Omitted row (Unchanged) → retain cached value (no-op).
//!
//! After merging, the cache re-constructs the FULL merged `[TypedProjectionEnvelope]`
//! set (cached bytes rebuilt as envelopes) and returns it alongside the set of
//! keys whose rev advanced in this frame. The existing `TypedXDecoder.decode(from:)`
//! family is then fed this merged set — byte-identical to today for unchanged
//! projections.
//!
//! ## Decode-before-commit (D3-4)
//!
//! For each `Changed` row the cache calls the decoder's `decode(bytes:)` entry
//! point as a preflight. Only on a non-nil result does it overwrite the cache.
//! This prevents a corrupt payload from blanking a healthy cached value.
//!
//! ## session_id == 0 handling (D3-5)
//!
//! A frame with `sessionId == 0` is treated as "no incremental contract" — the
//! host does not trust omission. The cache is NOT cleared on such a frame; the
//! frame's envelopes are passed through unchanged as the merged set. UI is
//! gated on `baselined == true`.

use std::path::Path;

use crate::swift_projections_registry::{SnapshotProjectionEntry, SNAPSHOT_PROJECTIONS};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-cache \\
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

private let pcLog = Logger(subsystem: \"io.f7z.chirp\", category: \"ProjectionCache\")

";

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct ProjectionCacheCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the `ProjectionMergeCache` Swift source from the registry.
#[must_use]
pub fn render_projection_cache(entries: &[SnapshotProjectionEntry]) -> String {
    // Collect only entries that have a typed sidecar with a swift_reader_type —
    // these are the projections the cache can validate via decode-before-commit.
    let typed_entries: Vec<&SnapshotProjectionEntry> = entries
        .iter()
        .filter(|e| {
            e.typed_sidecar
                .as_ref()
                .and_then(|s| s.swift_reader_type)
                .is_some()
        })
        .collect();

    let mut out = String::from(HEADER);

    // ── CacheEntry private struct ─────────────────────────────────────────────
    out.push_str(
        "// MARK: - CacheEntry\n\
         /// One cached projection slot: the raw FlatBuffers payload bytes and\n\
         /// the last successfully committed `projectionRev`.\n\
         private struct CacheEntry {\n\
         \x20\x20\x20\x20let rev: UInt64\n\
         \x20\x20\x20\x20let schemaId: String\n\
         \x20\x20\x20\x20let schemaVersion: UInt32\n\
         \x20\x20\x20\x20let fileIdentifier: String\n\
         \x20\x20\x20\x20let payload: Data\n\
         }\n\n",
    );

    // ── MergeResult ───────────────────────────────────────────────────────────
    out.push_str(
        "// MARK: - MergeResult\n\
         /// Return type of `ProjectionMergeCache.merge(frame:)`. Carries the\n\
         /// fully-merged envelope set (fed to the existing TypedXDecoder family)\n\
         /// and the set of projection keys whose rev advanced in this frame\n\
         /// (used by `KernelModel.apply` to skip unchanged @Published slots).\n\
         struct MergeResult {\n\
         \x20\x20\x20\x20/// Fully-reconstituted envelope set: cached rows for omitted keys,\n\
         \x20\x20\x20\x20/// freshly-decoded rows for Changed keys, nothing for Cleared keys.\n\
         \x20\x20\x20\x20/// Feed this set to the existing TypedXDecoder.decode(from:) family.\n\
         \x20\x20\x20\x20let mergedEnvelopes: [TypedProjectionEnvelope]\n\
         \x20\x20\x20\x20/// Keys whose projectionRev advanced (Changed and committed) in this\n\
         \x20\x20\x20\x20/// frame. Also includes Cleared keys so the caller can nil-out their\n\
         \x20\x20\x20\x20/// @Published slots.\n\
         \x20\x20\x20\x20let changedKeys: Set<String>\n\
         \x20\x20\x20\x20/// Sticky flag: true when at least one decode-before-commit failed.\n\
         \x20\x20\x20\x20/// The prior cache entry is retained (no silent corruption), but the\n\
         \x20\x20\x20\x20/// host is known-degraded for that key. Rung 3 logs; Rung 4 resyncs.\n\
         \x20\x20\x20\x20let needsResync: Bool\n\
         }\n\n",
    );

    // ── ProjectionMergeCache ──────────────────────────────────────────────────
    out.push_str(
        "// MARK: - ProjectionMergeCache\n\
         /// NMP-owned rev-aware projection cache (ADR-0055 R3-S3).\n\
         ///\n\
         /// Lives in `KernelHandle` (one instance per kernel app). Fed each\n\
         /// FlatBuffers frame before the TypedXDecoder family runs. Implements\n\
         /// the D3-3 merge algorithm exactly so app code is oblivious to delta\n\
         /// mechanics.\n\
         ///\n\
         /// Thread-safety: called only from the NMP update callback\n\
         /// (`nmpUpdateCallback`), which fires on the Rust actor thread and is\n\
         /// always dispatched to `@MainActor` before `KernelModel.apply`. The\n\
         /// cache is NOT shared across threads.\n\
         final class ProjectionMergeCache {\n\
         \x20\x20\x20\x20private var cache: [String: CacheEntry] = [:]\n\
         \x20\x20\x20\x20private var appliedSession: UInt64 = 0\n\
         \x20\x20\x20\x20private var appliedEpoch: UInt64 = 0\n\
         \x20\x20\x20\x20/// D3-5: false until the first post-baseline frame is applied.\n\
         \x20\x20\x20\x20/// UI should be gated on this being true.\n\
         \x20\x20\x20\x20private(set) var baselined: Bool = false\n\
         \x20\x20\x20\x20/// D3-4: latches true on any decode-before-commit failure.\n\
         \x20\x20\x20\x20/// Rung 4 drains it via `nmp_app_request_full_snapshot`.\n\
         \x20\x20\x20\x20private(set) var needsResync: Bool = false\n\n",
    );

    // reset() method
    out.push_str(
        "    /// Hard-reset the cache (called from `KernelHandle.reset()` /\n\
         \x20\x20\x20\x20/// `resetAndRestart()` so the next frame is treated as a full baseline).\n\
         \x20\x20\x20\x20func reset() {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20cache.removeAll()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20appliedSession = 0\n\
         \x20\x20\x20\x20\x20\x20\x20\x20appliedEpoch = 0\n\
         \x20\x20\x20\x20\x20\x20\x20\x20baselined = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20needsResync = false\n\
         \x20\x20\x20\x20}\n\n",
    );

    // tryDecode static helper — decode-before-commit dispatch table
    out.push_str(
        "    // MARK: - Decode-before-commit dispatch\n\
         \x20\x20\x20\x20//\n\
         \x20\x20\x20\x20// Each projection key that has a Swift reader type gets a decode probe\n\
         \x20\x20\x20\x20// here. Returns `true` iff the bytes decode successfully. We call the\n\
         \x20\x20\x20\x20// `decode(bytes:)` entry point on the generated TypedXDecoder enum —\n\
         \x20\x20\x20\x20// a non-nil result means the bytes are well-formed. On failure we keep\n\
         \x20\x20\x20\x20// the prior cache entry rather than clobbering it with corrupt bytes.\n\
         \x20\x20\x20\x20private static func decodeSucceeds(key: String, bytes: Data) -> Bool {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20guard !bytes.isEmpty else { return false }\n\
         \x20\x20\x20\x20\x20\x20\x20\x20switch key {\n",
    );
    for entry in &typed_entries {
        let decoder = decoder_enum_name(entry.swift_field);
        out.push_str(&format!(
            "        case {:?}: return {decoder}.decode(bytes: bytes) != nil\n",
            entry.key
        ));
    }
    out.push_str(
        "        default:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Tier-1 host projections (feed, follow_list, etc.) that have no\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// swift_reader_type in the registry are always-Changed: we cannot\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// run a typed preflight, so accept them unconditionally.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20return true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20}\n\n",
    );

    // merge(frame:) — the D3-3 algorithm
    out.push_str(
        "    // MARK: - merge\n\
         \x20\x20\x20\x20/// Run the D3-3 merge algorithm for one incoming frame.\n\
         \x20\x20\x20\x20///\n\
         \x20\x20\x20\x20/// - If `sessionId` or `snapshotEpoch` changed ⇒ mandatory full-cache\n\
         \x20\x20\x20\x20///   reset (D4). The kernel guarantees the first post-change frame is a\n\
         \x20\x20\x20\x20///   full baseline.\n\
         \x20\x20\x20\x20/// - `sessionId == 0` ⇒ no incremental contract (full frame anyway);\n\
         \x20\x20\x20\x20///   pass envelopes through unchanged without trusting omission (D3-5).\n\
         \x20\x20\x20\x20/// - Changed row, rev > cached.rev ⇒ decode-before-commit; on success\n\
         \x20\x20\x20\x20///   overwrite cache; on failure keep prior + latch `needsResync`.\n\
         \x20\x20\x20\x20/// - Cleared row ⇒ remove from cache; add key to changedKeys.\n\
         \x20\x20\x20\x20/// - Omitted row (Unchanged) ⇒ retain cached value (no-op).\n\
         \x20\x20\x20\x20///\n\
         \x20\x20\x20\x20/// Returns the fully-merged envelope set + changed-key set.\n\
         \x20\x20\x20\x20func merge(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20envelopes: [TypedProjectionEnvelope],\n\
         \x20\x20\x20\x20\x20\x20\x20\x20sessionId: UInt64,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20snapshotEpoch: UInt64\n\
         \x20\x20\x20\x20) -> MergeResult {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// D3-5: session_id == 0 means no incremental contract.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Pass envelopes through as-is; do not trust omission.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// changedKeys = all keys present (conservative: treat as fully changed).\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if sessionId == 0 {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20let keys = Set(envelopes.map(\\.key))\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20return MergeResult(mergedEnvelopes: envelopes, changedKeys: keys, needsResync: needsResync)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// D4: mandatory reset on session or epoch change.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if sessionId != appliedSession || snapshotEpoch != appliedEpoch {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache.removeAll()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20appliedSession = sessionId\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20appliedEpoch = snapshotEpoch\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20baselined = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20var changedKeys = Set<String>()\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Run the merge algorithm over incoming rows.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20for envelope in envelopes {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20switch envelope.state {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20case .cleared:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Explicit clear: remove from cache, mark as changed\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// so the caller nils the @Published slot.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache.removeValue(forKey: envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys.insert(envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20case .changed:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20let incomingRev = envelope.projectionRev\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// D3 reorder guard: under synchronous in-process delivery this\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// never fires, but belt-and-braces for future async transport.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if let cached = cache[envelope.key], incomingRev <= cached.rev {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20continue\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Decode-before-commit (D3-4): run the typed decoder as a\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// preflight. On success: overwrite cache + advance rev.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// On failure: keep prior entry + latch needsResync.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if Self.decodeSucceeds(key: envelope.key, bytes: envelope.payload) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache[envelope.key] = CacheEntry(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20rev: incomingRev,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaId: envelope.schemaId,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaVersion: envelope.schemaVersion,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20fileIdentifier: envelope.fileIdentifier,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20payload: envelope.payload\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys.insert(envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20} else {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync = true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20pcLog.error(\"decode-before-commit failed for key=\\(envelope.key, privacy: .public) rev=\\(incomingRev, privacy: .public) — keeping prior cache entry, needsResync latched\")\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Reconstruct the full merged envelope set from the cache.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Cleared keys are absent from the cache (already removed above),\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// so they are correctly absent from the merged set too.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20let mergedEnvelopes: [TypedProjectionEnvelope] = cache.map { key, entry in\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20TypedProjectionEnvelope(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20key: key,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaId: entry.schemaId,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaVersion: entry.schemaVersion,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20fileIdentifier: entry.fileIdentifier,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20payload: entry.payload,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20projectionRev: entry.rev,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20state: .changed\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20baselined = true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return MergeResult(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20mergedEnvelopes: mergedEnvelopes,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys: changedKeys,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync: needsResync\n\
         \x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20}\n\
         }\n",
    );

    out
}

/// The Swift enum name for a projection's generated decoder. `accounts` →
/// `TypedAccountsDecoder`. Mirrors [`crate::swift_typed_decoders::decoder_enum_name`].
fn decoder_enum_name(swift_field: &str) -> String {
    let mut chars = swift_field.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("Typed{capitalized}Decoder")
}

/// Write the generated `ProjectionCache.generated.swift` to `out_path`.
pub fn generate_projection_cache(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_projection_cache(SNAPSHOT_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
pub fn check_projection_cache(out_path: &Path) -> std::io::Result<ProjectionCacheCheckOutcome> {
    let rendered = render_projection_cache(SNAPSHOT_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectionCacheCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(ProjectionCacheCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(ProjectionCacheCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}
