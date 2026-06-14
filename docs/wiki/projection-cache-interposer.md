---
title: Projection Cache Interposer
slug: projection-cache-interposer
topic: kernel-snapshot
summary: The iOS ProjectionCache interposer keeps a keyâ(rev, bytes) cache, merges each frame (Changed overwrites, Cleared drops, omitted retained), and hands the exis
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Projection Cache Interposer

## Key→(Rev, Bytes) Cache and Frame Merge Semantics

The iOS ProjectionCache interposer keeps a key→(rev, bytes) cache, merges each frame (Changed overwrites, Cleared drops, omitted retained), and hands the existing decoders a fully-reconstituted set so app code never touches delta mechanics. The R3-S3 generated interposer is codegen-produced and runs before existing typed decoders, so app code (KernelModel accessors) stays byte-identical and developers never touch incremental-apply mechanics. The producer omits Unchanged Tier-2 rows entirely from the wire, keeps an explicit payload-less Cleared marker, and keeps full Changed rows. The NMP-owned generated ProjectionCache interposer re-feeds existing per-key decoders a merged full envelope set, so app code calls model.wallet exactly as before and structurally cannot get incremental-apply wrong; Android uses an identical merge algorithm. The Cleared-signal inverse pass synthesizes a payload-less TypedProjectionData row for manifest-Cleared keys absent from the typed vector, so the host ProjectionCache can remove them. A projection manifest enumerates the full 18-key builtin universe (not just rows present in typed), so the omit_unchanged consumer can synthesize Cleared rows for manifest-Cleared keys absent from the typed vector.

<!-- citations: [^78c8e-418] [^78c8e-431] [^78c8e-445] [^78c8e-457] [^78c8e-474] -->
## Session and Epoch Handling

The iOS ProjectionCache rebaseline (session_id or snapshot_epoch change) atomically clears all cached entries before processing the new frame's rows, rebaselining in lockstep with the producer's FrameIdentity reset. session_id and snapshot_epoch are threaded out of the single existing KernelUpdateFrameDecoder decode pass (not a second buffer re-parse), eliminating a per-tick O(buffer) waste and a spurious FlatBuffers import. The feed emission state rebaselines on FrameIdentity(session_id, snapshot_epoch) — the same two-axis signal the host ProjectionCache resets on — so a Reset/session change forces a baseline in both producer and host simultaneously, preventing a frozen timeline on app reset. The nmp_app_reset / resetAndRestart path rebuilds the kernel (changing session_id, which clears the host cache) while preserving the feed engine Arc and FeedEmissionState; the fixed FrameIdentity read ensures the surviving closure detects the new session_id and emits a baseline rather than omitting into an empty cache. The producer and host reset on the same FrameIdentity(session_id, snapshot_epoch) signal; the bespoke account-switch-only emission_epoch was deleted as subsumed. The Reset freeze test c10 fails against the pre-fix epoch-only logic and passes with the FrameIdentity fix, proving it guards the actual freeze path.

<!-- citations: [^78c8e-419] [^78c8e-432] [^78c8e-446] [^78c8e-458] -->
## Decode-Before-Commit Failure Policy

The ProjectionCache decode-before-commit guarantee prevents blanking: on a .changed row, the typed decoder runs as preflight; cache and rev advance only inside the success branch; on failure, needsResync is latched and the prior entry is untouched. Decode-before-commit plus a sticky needsResync latch is the sufficient self-healing floor given synchronous in-process delivery; Rung 3 does not need a per-frame full manifest.

<!-- citations: [^78c8e-420] [^78c8e-447] [^78c8e-459] [^78c8e-476] -->
## Android Decode-Success Floor Equivalence

Android's decodeSucceeds uses bytes.isNotEmpty() which is semantically equal to iOS's effective floor because FlatBuffers getRoot is unchecked on both platforms; non-empty corrupt payloads are caught by try/catch + file-identifier checks in the typed decoder re-decode path on Android. Android re-decodes from the merged envelope set, so omitted projections keep their prior value — the finding-4 regression is avoided.

<!-- citations: [^78c8e-421] [^78c8e-433] [^78c8e-448] [^78c8e-475] [^78c8e-487] -->
## FlatBuffers Schema Pinned Versions

The FlatBuffers schema uses pinned versions: Rust+Swift 25.12.19, Kotlin 25.2.10, TypeScript 25.9.23. <!-- [^78c8e-434] -->

## Host Swift Decode Overhead

The host Swift decode (TypedHomeFeedDecoder.decode) runs every tick over the cache-reconstructed feed regardless of changedKeys; R6-S1's idle win covers FFI bytes + @Published invalidation + Array allocation, but NOT the per-tick O(80-card) decode. Gating TypedHomeFeedDecoder.decode on changedKeys would be a contained follow-up with no protocol surface change, but it may conflict with the bounded-native-state doctrine (D5 forbids native caches of derived values).

On iOS, the home-feed timeline body does not re-evaluate on idle ticks in either incremental-ON or OFF arm because .equatable() at HomeFeedView.swift:147 short-circuits when roots is unchanged, making it the load-bearing idle-render shield. <!-- [^78c8e-488] -->

<!-- citations: [^78c8e-460] [^78c8e-478] -->
## Oracle Model and Correctness Scope

The MiniProjectionCache in the S7 oracle models only the steady-state Changed/Cleared/retain subset (not session/epoch rebaseline, sessionId==0 pass-through, rev-monotonicity, or decode-before-commit); session/epoch correctness is proven by R6-S1's FrameIdentity tests. <!-- [^78c8e-477] -->
