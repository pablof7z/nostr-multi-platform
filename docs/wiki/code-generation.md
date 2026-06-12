---
title: Code Generation and FFI
slug: code-generation
topic: code-generation
summary: A Rust flatc codegen-drift CI gate (ci/check-rust-flatc-drift.sh) exists and fails on synthetic drift in both directions.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Code Generation and FFI

## Rust Flatc Codegen Drift CI Gate

A Rust codegen-drift CI gate (check-rust-flatc-drift.sh) is added that version-pins flatc, regenerates bindings, rustfmts, and byte-diffs; it fails on synthetic drift in both directions. Cross-platform frame-contract drift gates currently cover Rust and Swift only; Kotlin and TypeScript bindings are ungated, and a kernel-emitted golden-frame fixture test on each native platform would have caught the Android dark-screen defect class. A real-frame golden fixture test (KernelUpdateFrameDecoderTier3GoldenTest) loads a FlatBuffers frame hex-encoded by Rust and asserts non-default values on every Tier-3 field, verified to fail on the pre-fix decoder.

<!-- citations: [^da6b1-45] [^da6b1-3] [^da6b1-25] [^da6b1-44] [^da6b1-97] -->
## Builder Guide Consumption Rules (Post-0.3.0)

The builder guide (docs/builder-guide/15-codegen-and-ffi.md) carries post-0.3.0 consumption rules: no generic payload tree, per-platform typed sidecar decoders, decode_snapshot_envelope for Tier-3 fields, register_snapshot_projection_gated with ChangeGate, and last_gc/last_gc_at_ms GC health signals. <!-- [^da6b1-4] -->

## C-ABI Surface Freeze

A C-ABI signature change (adding a parameter to an existing function) is net-zero for the surface-freeze CI gate, which only counts net-new symbol names. The C-ABI surface freeze script and process have been deleted — PR #933 removed the workflow and script per the direction that the freeze was self-imposed with no external constraint. <!-- [^37035-1] -->

## FFI Error Handling

The FFI across the C-ABI boundary is fire-and-forget returning void; failures come back as a `toast` string in the next state snapshot, never as exceptions or Result types crossing the boundary. <!-- [^37035-2] -->

NMP should not adopt UniFFI/KMP over its existing C-ABI + FlatBuffers codegen because UniFFI cannot express the FlatBuffers snapshot model. <!-- [^954c5-1] -->

Any compute-heavy API exposed over FFI should follow a batch-based shape (start/count) so the caller owns threading, matching coracle-rust's mine_pow pattern. <!-- [^954c5-10] -->
