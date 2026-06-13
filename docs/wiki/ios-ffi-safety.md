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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# iOS FFI Safety

## KernelHandle Update Sink

iOS KernelBridge.listen() uses passUnretained(sink) creating a fragile ARC teardown contract; passRetained(sink) with takeRetainedValue in the callback is safer given Rust's quiescence guarantee.

The FlatBuffers Verifier is dropped on the trusted in-process kernel snapshot decode path; all 35 iOS decoder sites use unchecked getRoot instead of getCheckedRoot. The buffer is produced microseconds earlier by the same in-process Rust kernel and crosses a trusted FFI boundary; routing-by-schemaId is the real type guard.

Issue #1289 was filed for wiring the Swift consumer of RelayDiagnosticsInfo (the regenerated binding) separately from the drift-gate PR.

iOS forward-compat fallback derivedLabel/derivedTone duplicates Rust business logic and silently falls through to default on unknown tokens without compile-time or runtime warning.

<!-- citations: [^02745-102] [^78c8e-65] [^02745-85] [^02745-34] [^02745-57] [^02745-101] [^78c8e-101] -->
