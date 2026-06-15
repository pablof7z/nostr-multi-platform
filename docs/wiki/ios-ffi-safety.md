---
title: iOS FFI Safety
slug: ios-ffi-safety
topic: nmp-ffi-surface
summary: iOS KernelBridge.listen() uses passUnretained(sink) creating a fragile ARC teardown contract; passRetained(sink) with takeRetainedValue in the callback is safer
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
---

# iOS FFI Safety

## KernelHandle Update Sink

iOS KernelBridge.listen() uses passUnretained(sink) creating a fragile ARC teardown contract; passRetained(sink) with takeRetainedValue in the callback is safer given Rust's quiescence guarantee.

The R3-S3 iOS review caught a missing import FlatBuffers in KernelBridge.swift that caused a build break, and identified the root cause as a double-decode of the same buffer for session_id/snapshot_epoch; the fix threads those scalars out of the single existing decode pass (added to the .snapshot case tuple) rather than re-parsing the buffer a second time, eliminating both the break and the per-tick O(buffer) waste.

The FlatBuffers Verifier is dropped on the trusted in-process kernel snapshot decode path; all 35 iOS decoder sites use unchecked getRoot instead of getCheckedRoot. The buffer is produced microseconds earlier by the same in-process Rust kernel and crosses a trusted FFI boundary; routing-by-schemaId is the real type guard.

Issue #1289 was filed for wiring the Swift consumer of RelayDiagnosticsInfo (the regenerated binding) separately from the drift-gate PR.

iOS forward-compat fallback derivedLabel/derivedTone duplicates Rust business logic and silently falls through to default on unknown tokens without compile-time or runtime warning.

On a Release iOS build, the idle home-feed timeline body does not re-evaluate — the load-bearing shield is the .equatable() boundary at HomeFeedView.swift:147, not the changedKeys omission; even with incremental OFF (feed reassigned every tick), body-evals remain 0.

The iOS Fault-A fix adds claim/release calls to NoteContentView (mention pubkeys), HomeFeedView ReplyAttributionLine, and NostrProfileName (self-claim), using the same refcounted lifecycle as NostrAvatar — no new FFI needed beyond the liveness parameter. <!-- [^ab806-79] -->

<!-- citations: [^78c8e-492] [^02745-102] [^78c8e-65] [^02745-85] [^02745-34] [^02745-57] [^02745-101] [^78c8e-101] [^78c8e-470] [^78c8e-493] -->
