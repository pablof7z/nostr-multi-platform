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
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019ec57a-fb01-7081-80c8-d7107f302049
---

# Codebase Patterns

## File-Size Constraints

Hand-authored source and documentation files must be kept under 300 lines of code where practical, with 500 lines as a hard ceiling. When a cohesive owner file approaches the LOC soft limit, split under the same owner namespace by concrete sub-type or sub-protocol, not by recreating global Model/Update/View layers. The file-size gate enforces the 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across the campaign. Files over cap must be split into cohesive same-module files rather than having their baseline bumped. Files already over the cap are baselined and must not grow further, and new glue must be extracted into cohesive sibling modules rather than bolted on. No baseline bumps are allowed. New tests that push files past the 500 LOC cap require a test-file split round-trip before the PR can merge. For example, KernelModel.kt must stay under the 500-LOC hard ceiling by extracting projection-cache wiring and decode-frame logic into a separate Android update helper; the Marmot section is extracted into MarmotActions.kt and MarmotActionEnvelopes.kt, and the relay_diagnostics_info mapper was extracted from TypedProjectionGlue.swift (881 lines) into TypedProjectionGlue+RelayDiagnostics.swift (117-line new sibling) rather than growing the over-baseline file. Similarly, the embed sidecar's install function was extracted from lib.rs into embed_sidecar.rs to keep nmp-ffi/lib.rs exactly at its 2976 LOC baseline, avoiding the file-size gate; any PR touching it must keep net growth at zero or extract code to a `#[path]` submodule. Gallery App.tsx and other hand-authored files approaching the hard line-count ceiling must be split by extracting support functions, render guards, and component-specific CSS into sibling modules rather than allowing oversize files. The push event file-size workflow should fall back to merge-base when the before SHA is unfetchable after a rebase, instead of failing closed with a permanent red. The zero-debt doctrine is effective: 10 of 11 needs-decision issues were determined by documented product direction (aim.md D0, thin-shell, plan.md v1=native-only, zero-debt doctrine) without requiring owner judgment: #1291 (full parity via thin-shell), #1283 (nmp-ffi resolution), #1250 (park dead crates), #1202 (honest wasm disable), #1090 (floor-coherent eviction + re-enable ceiling), #1008 (post-v1, stale label), #999 (post-v1), #980 (in-scope v1-dx, blocked on #1283), #967 (post-v1), #920 (envelope-cut, not naive move — cycle-blocked). Only #1281 (backfill semantics) genuinely needed the owner. The marmot_snapshot.fbs FlatBuffers schema must have an automated codegen-drift CI gate (matching the existing nmp_update.fbs gate) to prevent hand-edited bindings from silently drifting from the .fbs source. The Swift flatc drift gate uses a pure byte-diff with zero fuzzing, consistent with the Rust/Kotlin/TS gates; .gitattributes marks `ios/Chirp/Chirp/Bridge/Generated/**` as `-text linguist-generated=true` and .editorconfig disables trailing-whitespace trimming for that path.

<!-- citations: [^78c8e-83] [^78c8e-47] [^da6b1-45] [^78c8e-64] [^02745-82] [^da6b1-66] [^02745-98] [^02745-116] [^da6b1-81] [^02745-128] [^78c8e-99] [^019ec-8] -->
## API Compatibility

No compat aliases are introduced; old APIs are hard-removed, not shimmed (the repo's no-compat-aliases rule applies across all merged PRs). <!-- [^78c8e-100] -->

A PR that is patch-equivalent to current master (all commits already applied via earlier squash-merged work) should be closed as superseded rather than merged as an empty or redundant change. A PR whose intended code and docs are already present on master (often with newer wording or architecture) should be closed as superseded rather than reintroducing its older version. <!-- [^019ec-9] -->

## Tactical Queue Discipline

The repository has exactly one canonical tactical queue: GitHub Issues; scattered notes, ad-hoc TODO.md/NOTES.md/ROADMAP.md/PLAN-foo.md files, and inline TODO comments used as tracking substitutes are forbidden. New top-level planning files are forbidden; tactical detail belongs in a GitHub issue and short-lived migration plans may live in docs/plan/m*.md or docs/architecture-audit/ only when gating an active milestone. A violation or feature tracked in GitHub Issues must not also be restated as a queue row in WIP.md or docs/plan.md. A PR that introduces a duplicate planning file, a scattered todo list, or a parallel roadmap is rejected and the entries are folded back into GitHub Issues or durable docs. Existing issues must be edited in place rather than appending parallel ones. Issue labels define priority work order: priority:p0 through priority:p4, with category:violation before category:feature before category:test before category:decision within each bucket. A TODO comment in code is not a plan; if it represents work to be done it belongs in a GitHub issue, and if it represents a known limitation or durable decision it belongs in an ADR, doctrine doc, architecture/design doc, builder-guide page, or wiki article. AI code review output, direction reviews, and post-merge review notes must not be committed to the repository; actionable findings must be promoted to a GitHub issue or durable doc and then discarded. Single source of truth per fact (D4): tactical state in GitHub Issues, in-flight branch ownership in WIP.md, release-plan checkpoints in docs/plan.md, durable facts in durable docs or code. <!-- [^019ec-10] -->

## Planning Doc Lifecycle

Plans must not survive as reference documentation after they have been implemented, executed, or invalidated; lasting knowledge belongs in durable documentation instead. <!-- [^019ec-11] -->

## Debug and History Surfaces

Debug and history surfaces must use log-safe action tags and correlation ids; never record secrets, raw nsecs, plaintext DMs, or bearer tokens. <!-- [^019ec-12] -->

## No Temporary Hacks

No temporary hacks are allowed: no for-now workarounds, no stubs that stay, and no TODO-fix-this-properly comments left in production code. A staged fix is allowed only when a GitHub issue labeled status:staged documents every stage with a completion deadline. <!-- [^019ec-13] -->

## Architectural Correctness

Every concept must have exactly one canonical representation and one code path; if two paths exist for the same concern, one must be deleted before the PR merges. Every change must be done by the book seeking the long-term correct architecture, not the shortest path to a green CI; if the correct fix requires touching 10 files or creating a new crate, that must be done. 'It works' is not an acceptance criterion; 'It works and is architecturally correct' is. <!-- [^019ec-14] -->
