---
title: Codebase Patterns
slug: codebase-patterns
topic: codebase-patterns
summary: The file-size gate enforces a 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across th
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Codebase Patterns

## File-Size Constraints

The file-size gate enforces a 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across the campaign. Files over cap must be split into cohesive same-module files rather than having their baseline bumped. Files already over the cap are baselined and must not grow further, and new glue must be extracted into cohesive sibling modules rather than bolted on. No baseline bumps are allowed. New tests that push files past the 500 LOC cap require a test-file split round-trip before the PR can merge. For example, KernelModel.kt must stay under the 500-LOC hard ceiling; the Marmot section is extracted into MarmotActions.kt and MarmotActionEnvelopes.kt, and the relay_diagnostics_info mapper was extracted from TypedProjectionGlue.swift (881 lines) into TypedProjectionGlue+RelayDiagnostics.swift (117-line new sibling) rather than growing the over-baseline file. Similarly, the embed sidecar's install function was extracted from lib.rs into embed_sidecar.rs to keep nmp-ffi/lib.rs exactly at its 2976 LOC baseline, avoiding the file-size gate; any PR touching it must keep net growth at zero or extract code to a `#[path]` submodule. The push event file-size workflow should fall back to merge-base when the before SHA is unfetchable after a rebase, instead of failing closed with a permanent red. The zero-debt doctrine is effective: 10 of 11 needs-decision issues were determined by documented product direction (aim.md D0, thin-shell, plan.md v1=native-only, zero-debt doctrine) without requiring owner judgment: #1291 (full parity via thin-shell), #1283 (nmp-ffi resolution), #1250 (park dead crates), #1202 (honest wasm disable), #1090 (floor-coherent eviction + re-enable ceiling), #1008 (post-v1, stale label), #999 (post-v1), #980 (in-scope v1-dx, blocked on #1283), #967 (post-v1), #920 (envelope-cut, not naive move — cycle-blocked). Only #1281 (backfill semantics) genuinely needed the owner.

The marmot_snapshot.fbs FlatBuffers schema must have an automated codegen-drift CI gate (matching the existing nmp_update.fbs gate) to prevent hand-edited bindings from silently drifting from the .fbs source. The Swift flatc drift gate uses a pure byte-diff with zero fuzzing, consistent with the Rust/Kotlin/TS gates; .gitattributes marks `ios/Chirp/Chirp/Bridge/Generated/**` as `-text linguist-generated=true` and .editorconfig disables trailing-whitespace trimming for that path.

<!-- citations: [^78c8e-83] [^78c8e-47] [^da6b1-45] [^78c8e-64] [^02745-82] [^da6b1-66] [^02745-98] [^02745-116] [^da6b1-81] [^02745-128] [^78c8e-99] -->

## API Compatibility

No compat aliases are introduced; old APIs are hard-removed, not shimmed (the repo's no-compat-aliases rule applies across all merged PRs). <!-- [^78c8e-100] -->
