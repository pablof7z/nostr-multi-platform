---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - projection-cache
  - incremental-apply
  - decode-before-commit
  - codegen
supersedes:
  - 2026-06-14-2-adr-0055-r3-s3-ios-projectioncache
related_claims: []
source_lines:
  - 8988-9010
  - 9023-9105
  - 9125-9153
  - 9195-9231
  - 9248-9347
  - 9358-9380
captured_at: 2026-06-14T10:45:38Z
---

# Episode: ProjectionCache codegen-generated interposer enables incremental emission on both mobile hosts

## Prior State

Every frame was fully decoded and applied on both iOS and Android — all projection payloads delivered to the host every tick regardless of whether they changed. No incremental-apply capability existed on any host; ~81% of Tier-2 wire bytes were waste (unchanged re-emissions). Hosts had no cache-merge layer and no mechanism to handle omitted keys or Cleared signals.

## Trigger

ADR-0055 Rung 3 required a host-side cache-merge interposer before `nmp_app_declare_incremental_apply` could be safely enabled. iOS was the first host (R3-S3), Android the second (R3-S4). A build-gate failure on iOS (KernelBridge.swift missing `import FlatBuffers` from a second buffer re-parse) forced the architectural decision to thread session_id/snapshot_epoch from the single existing decode rather than re-parsing.

## Decision

A codegen-generated `ProjectionMergeCache` interposer (Swift for iOS, Kotlin for Android) runs before the existing typed decoders. It implements: (1) decode-before-commit — `decodeSucceeds` guards cache mutation, failed decode sets `needsResync` without advancing rev or blanking the slot; (2) rebaseline on session/epoch change is atomic (clear cache before row loop); (3) session_id==0 pass-through without trusting omission; (4) `changedKeys` tracks only committed-Changed ∪ Cleared keys. On iOS, `KernelModel.apply` updates only slots in `changedKeys`; on Android, the merged envelope set is re-decoded whole and StateFlow value-equality handles dedup. `decodeSucceeds` uses `isNotEmpty()` on Android (acceptable because iOS's per-key typed-decoder preflight is equally theater for non-empty corrupt payloads — both platforms use unchecked FlatBuffers `getRoot`). `nmp_app_declare_incremental_apply` is called before `nmp_app_start` on both platforms.

## Consequences

- Both mobile platforms now realize incremental-emission savings (previously ~81% Tier-2 waste)
- D3-4 no-corrupt-UI guarantee honored to the same degree on both platforms: empty-payload Changed rows are caught; non-empty corrupt bytes fail-closed on re-decode (identifier check + try/catch) and self-heal on next good rev
- iOS `KernelBridge.swift` has no FlatBuffers dependency — session/epoch are threaded from the single existing decode pass, no per-tick O(buffer) re-parse
- Android error path for `declare_incremental_apply` failure frees the `app` pointer (NIT-1 fix); corrupt-non-empty regression test added (NIT-2)
- The interposer is codegen-maintained: `cargo run -p nmp-codegen -- gen projection-cache --check` must exit 0 for both platforms

## Open Tail

- R3-S5 (S6 capstone) still needed to empirically prove 81%→<5% waste reduction via ffi-stress harness
- R3-S6 documentation step remaining

## Evidence

- transcript lines 8988-9010
- transcript lines 9023-9105
- transcript lines 9125-9153
- transcript lines 9195-9231
- transcript lines 9248-9347
- transcript lines 9358-9380

