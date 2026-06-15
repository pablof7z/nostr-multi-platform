---
title: Codebase Patterns
slug: codebase-patterns
topic: codebase-patterns
summary: Hand-authored source and documentation files must be kept under 300 lines of code where practical; 500 lines is a hard ceiling
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
  - session:fabf8ca3-e1b9-4a7c-bcd5-bf5731fb571d
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Codebase Patterns

## File-Size Constraints

Hand-authored source and documentation files must be kept under 300 lines of code where practical; 500 lines is a hard ceiling. Generated, vendored, lockfile, binary, and benchmark-output artifacts are exempt from the LOC ceiling, but their producers must be kept small and documented. Modules must be organized by cohesive feature/page/view/protocol/domain-type owner, not by technical role buckets like model/update/view/state/actions. When a cohesive owner module approaches the LOC limit, it must be split under the same owner namespace by concrete sub-type or sub-protocol, not by recreating global Model/Update/View layers. The file-size gate enforces the 500-line hard cap with an anti-cheat rule that blocks raising a file's baseline in a PR; zero baseline bumps were merged across the campaign. Files over cap must be split into cohesive same-module files rather than having their baseline bumped. Files already over the cap are baselined and must not grow further, and new glue must be extracted into cohesive sibling modules rather than bolted on. Per repo doctrine, no baseline-bump exceptions are allowed; oversized files must be split. New tests that push files past the 500 LOC cap require a test-file split round-trip before the PR can merge. Pre-existing violations like docs/decisions/0055-rung3.md (918 LOC) will keep failing the file-size check on PRs until they are split, baselined, or exempted. For example, KernelModel.kt must stay under the 500-LOC hard ceiling by extracting projection-cache wiring and decode-frame logic into a separate Android update helper; the Marmot section is extracted into MarmotActions.kt and MarmotActionEnvelopes.kt, and the relay_diagnostics_info mapper was extracted from TypedProjectionGlue.swift (881 lines) into TypedProjectionGlue+RelayDiagnostics.swift (117-line new sibling) rather than growing the over-baseline file. Similarly, the embed sidecar's install function was extracted from lib.rs into embed_sidecar.rs to keep nmp-ffi/lib.rs exactly at its 2976 LOC baseline, avoiding the file-size gate; any PR touching it must keep net growth at zero or extract code to a `#[path]` submodule. Gallery App.tsx and other hand-authored files approaching the hard line-count ceiling must be split by extracting support functions, render guards, and component-specific CSS into sibling modules rather than allowing oversize files. The push event file-size workflow should fall back to merge-base when the before SHA is unfetchable after a rebase, instead of failing closed with a permanent red. The zero-debt doctrine is effective: 10 of 11 needs-decision issues were determined by documented product direction (aim.md D0, thin-shell, plan.md v1=native-only, zero-debt doctrine) without requiring owner judgment: #1291 (full parity via thin-shell), #1283 (nmp-ffi resolution), #1250 (park dead crates), #1202 (honest wasm disable), #1090 (floor-coherent eviction + re-enable ceiling), #1008 (post-v1, stale label), #999 (post-v1), #980 (in-scope v1-dx, blocked on #1283), #967 (post-v1), #920 (envelope-cut, not naive move — cycle-blocked). Only #1281 (backfill semantics) genuinely needed the owner. The marmot_snapshot.fbs FlatBuffers schema must have an automated codegen-drift CI gate (matching the existing nmp_update.fbs gate) to prevent hand-edited bindings from silently drifting from the .fbs source. The Swift flatc drift gate uses a pure byte-diff with zero fuzzing, consistent with the Rust/Kotlin/TS gates; .gitattributes marks `ios/Chirp/Chirp/Bridge/Generated/**` as `-text linguist-generated=true` and .editorconfig disables trailing-whitespace trimming for that path. A mechanism_census test must assert per-capability mechanism counts and fail CI when a second generation appears. A Linear ActionTicket with #[must_use] and a Drop bomb recording Failed{dropped} eliminates the ~15 hand-patch sites and collapses three correlation-id regimes into one identity. The three doctrine-lint rules to add are D20 (no ambient authority: ban OnceLock/lazy_static/static mutable holding non-const state), D21 (correlation linearity: ban correlation_id: Option<String> outside ActionTicket), and D22 (presence-floor ban: after K3, ban event-store newest-match queries from the floor path). D21 doctrine-lint bans process-global mutable state (OnceLock/lazy_static/static Mutex/RwLock/AtomicPtr holding non-const state) in production crates, with a burning allowlist.

<!-- citations: [^78c8e-83] [^78c8e-47] [^da6b1-45] [^78c8e-64] [^02745-82] [^da6b1-66] [^02745-98] [^02745-116] [^da6b1-81] [^02745-128] [^78c8e-99] [^019ec-8] [^2e544-363] [^2e544-408] [^418d5-11] [^fabf8-1] [^019ec-3] -->
## API Compatibility

No compat aliases are introduced; old APIs are hard-removed, not shimmed (the repo's no-compat-aliases rule applies across all merged PRs). <!-- [^78c8e-100] -->

A PR that is patch-equivalent to current master (all commits already applied via earlier squash-merged work) should be closed as superseded rather than merged as an empty or redundant change. A PR whose intended code and docs are already present on master (often with newer wording or architecture) should be closed as superseded rather than reintroducing its older version. <!-- [^019ec-9] -->

## Tactical Queue Discipline

GitHub Issues are the one canonical tactical queue for the repository; scattered notes, ad-hoc TODO.md/NOTES.md/ROADMAP.md/PLAN-foo.md files, and inline // TODO: annotations used as tracking substitutes are forbidden. New top-level planning files (PLAN.md, TODO.md, ROADMAP.md, NEXT.md, STATUS.md, or per-feature plan files at the repo root or directly under docs/) must not be created. State must not be duplicated across files; a violation or feature tracked in GitHub Issues must not also be restated as a queue row in WIP.md or docs/plan.md. A PR that introduces a duplicate planning file, a scattered todo list, or a parallel roadmap is rejected and the entries are folded back into GitHub Issues or durable docs. When queued work changes, the existing issue body/labels/title must be updated in place rather than appending parallel issues. Executed plans must be retired: close the issue, delete the temporal detail, or replace it with the smallest remaining live follow-up issue; durable lessons go in durable docs. Auto-generated knowledge-capture wiki .md files must be archived but not committed, as they violate the no-AI-dump rule. Issue labels define priority order: priority:p0 first, then p1 through p4; within a bucket, prefer category:violation before category:feature, then category:test, then category:decision. A // TODO: comment in code representing work to be done belongs in a GitHub issue; if it represents a known limitation or durable decision, it belongs in an ADR, doctrine clarification, architecture/design doc, builder-guide page, or wiki article. AI code review output, direction reviews, codex review dumps, and post-merge review notes must not be committed to the repository; actionable findings must be promoted to a GitHub issue or durable doc, then the review itself discarded. Each fact has a single source of truth (D4 applied to docs): tactical state in GitHub Issues, in-flight branch ownership in WIP.md, release-plan checkpoints in docs/plan.md, and durable facts in durable docs or code.

<!-- citations: [^019ec-10] [^fabf8-2] [^019ec-4] -->
## Planning Doc Lifecycle

Plans must not survive as reference documentation after they have been implemented, executed, or invalidated; lasting knowledge from completed work belongs in durable documentation instead.

<!-- citations: [^019ec-11] [^019ec-5] -->
## Debug and History Surfaces

Debug and history surfaces must use log-safe action tags and correlation ids; they must never record secrets, raw nsecs, plaintext DMs, or bearer tokens.

<!-- citations: [^019ec-12] [^019ec-6] -->
## No Temporary Hacks

No temporary hacks are allowed: no for-now workarounds, no stubs that stay, and no TODO-fix-this-properly comments left in production code. A staged fix is allowed only when a GitHub issue labeled status:staged documents every stage with a completion deadline. <!-- [^019ec-13] -->

## Architectural Correctness

Every concept must have exactly one canonical representation and one code path; if two paths exist for the same concern, one must be deleted before the PR merges. Every change must be done by the long-term correct architecture, not the shortest path to a green CI; if the correct fix requires touching many files or creating a new crate, that is what must be done. 'It works' is not an acceptance criterion; 'It works and is architecturally correct' is. TDD style is preferred whenever possible for all implementation work. Cross-module contract comments must cite a test; a claim like 'flush_due already enforces the rate limit' with no test citation is review-rejectable.

<!-- citations: [^019ec-14] [^2e544-429] [^019ec-9] -->
## Rust/Native Boundary

Native code (Swift, Kotlin, TypeScript, etc.) is allowed only to render Rust-produced state snapshots into UI and to execute capabilities by calling OS APIs and reporting raw results back to Rust; it must never decide policy, retry, cache, or contain business logic. Every external effect must be represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state. New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams; reducers must remain replayable from message history. <!-- [^019ec-7] -->

## Crate Placement

A feature belongs in an NMP crate (crates/) when it is a general building block that any Nostr app could use directly; app-specific proprietary domain logic belongs in app Rust crates (apps/<app>/). <!-- [^019ec-8] -->
