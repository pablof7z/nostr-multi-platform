---
title: Developer Workflow Standards
slug: developer-workflow-standards
topic: developer-workflow
summary: "No temporary hacks, 'for now' workarounds, or stubs that stay in production code are permitted; a staged fix is allowed only when a GitHub issue labeled status:"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:019edc10-1fb3-7752-ab3e-7f5b969da686
  - session:019edc16-8e40-7a92-9ea1-7405af0d34f3
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:019edc63-ed50-7dc0-9f1a-38e311efc3b4
  - session:019edc84-6e5c-74a2-9ed9-57938dae31a1
  - session:019edc94-e2f8-76e3-8cdc-a6d8f6bba72a
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Developer Workflow Standards

## Architectural Standards

No temporary hacks, 'for now' workarounds, or stubs that stay in production code are permitted; a staged fix is allowed only when a GitHub issue labeled status:staged documents every stage with a completion deadline. Every concept must have exactly one canonical representation and one code path; if two paths exist for the same concern, one must be deleted before the PR merges. Every change must seek the long-term correct architecture, not the shortest path to a green CI; if the correct fix requires touching 10 files or creating a new crate, that must be done instead of papering over a structural problem with a local patch. The acceptance criterion is 'it works and is architecturally correct', not merely 'it works'. A D4 doctrine lint does not exist yet; it is identified as an actionable item but not created in this campaign.

<!-- citations: [^11850-205] [^95d02-2] [^45258-10] [^45258-11] [^7b06d-1] [^019ed-5] [^019ed-42] [^019ed-57] [^019ed-69] [^019ed-98] [^019ed-153] -->
## Planning and Backlog Tracking

The repository has exactly one canonical tactical queue: GitHub Issues; scattered notes, ad-hoc TODO.md/NOTES.md/ROADMAP.md/PLAN-foo.md files, parallel planning docs, and inline // TODO: annotations used as substitutes for tracking are forbidden. The project must not duplicate plan files or scatter planning notes around the repository; strict repository discipline is required. A violation or feature tracked in GitHub Issues must not be restated in any other file. Issue labels define priority order: work priority:p0 through priority:p4 in order; within a bucket, prefer category:violation before category:feature, then category:test, then category:decision, unless the user explicitly directs otherwise. GitHub search is the backlog view; use gh issue list --state open --label priority:p0 --limit 50 (repeating for each priority level). A // TODO: comment in code is not a plan; if it represents work to be done, it belongs in a GitHub issue; if it represents a known limitation or durable decision, it belongs in an ADR, doctrine clarification, architecture/design doc, builder-guide page, or wiki article. Single source of truth per fact (D4 applied to docs): tactical state in GitHub Issues, durable facts in durable docs or code. AI code review output, direction reviews, codex review dumps, and post-merge review notes must not be committed to the repository; actionable findings must be promoted to a GitHub issue or durable doc and then discarded. Existing GitHub issues must be edited in place rather than appending parallel ones; if queued work changes, update the existing issue body/labels/title rather than creating duplicates. An implemented plan must be retired: close the issue, delete the temporal detail, or replace it with the smallest remaining live follow-up issue; durable lessons are preserved in the durable doc that owns that concept. Plans must not survive as reference documentation after being implemented, executed, or invalidated; lasting knowledge belongs in durable documentation (docs/aim.md, docs/product-spec/, docs/architecture/, docs/design/, docs/decisions/, builder guide, wiki/). No new top-level planning files (PLAN.md, TODO.md, ROADMAP.md, NEXT.md, STATUS.md, or per-feature plan files) may be created at the repo root or directly under docs/; new tactical detail belongs in a GitHub issue, and short-lived migration plans may live in docs/architecture-audit/ only when they gate a specific active milestone and link back to the owning issue. AGENTS.md must contain a planning-discipline section stating the canonical files, their roles, update cadences, and strict rules: no new top-level plan files, no duplicated state, single source of truth per fact (D4 applied to docs), edit-in-place rather than append-parallel, and fewer files when in doubt. A PR that introduces a duplicate planning file, a scattered todo list, or a parallel roadmap is rejected and the entries are folded back into GitHub Issues or durable docs. CLAUDE.md must serve as a thin pointer to AGENTS.md, providing a cold-start reading order and a TL;DR of the planning discipline, without duplicating content from AGENTS.md.

<!-- citations: [^9fc44-2] [^9fc44-3] [^95d02-3] [^9fc44-1] [^f2605-7] [^019ed-7] [^019ed-40] [^019ed-96] -->
## Open Items

PD-033 Finding B flags `wallet_status` as a D0 violation still needing a formal decision. P4 finding 4 (ExternalSignerCapabilityBridge transport selection and concurrent-Intent rejection) is not a violation — it is mechanical from Rust-set fields and an OS capacity constraint. #1516 and #1518 can proceed in parallel after #1522 + #1517 merge, as long as their write sets stay separate. The #1524 final acceptance gate is implemented in two passes: Pass 1 (early-stop gate + DM-ciphertext fixture + docs) now, Pass 2 (un-ignore gate, add remaining replay fixtures, extend cache-baseline binary) at epic close after #1516–#1522 merge. Web client.ts ProjectionMergeCache (Finding 5) and chirpConfig.ts relay-role drift (Finding 6) are deferred to a post-v1 follow-up issue (#1546).

<!-- citations: [^95d02-4] [^129d2-38] [^11850-116] [^11850-134] -->
## Workflow

All work follows the PR → codex exec review → fix → merge to master workflow. A pull request description must include a short TLDR summary of what changed, a detailed overview of the work performed, and any subjective decisions made including tradeoffs or assumptions. Completed work must be opened as a ready-for-review pull request, not as a draft, unless explicitly requested or intentionally incomplete. When a user gives a correction or instruction about how the NMP product should work, it must be treated as a possible product-authority update; a separate agent must research whether the correction belongs in product specs, doctrine, canonical docs, GitHub Issues, ADRs, or another authoritative document before making code changes. If documentation needs to change due to a product correction, the documentation update must be made in the same PR as the implementation unless the user explicitly scopes the work to docs only. The codex exec command must be run with a very simple prompt directing codex to read a temporary file containing the full prompt, to prevent errors when running codex. When running codex exec, append `< /dev/null` to prevent stdin hang. Codex should only be called again after all suggested fixes from the current assessment have been landed on master. Codex takes approximately 20 minutes to finish and must be allowed to finish without being killed.

The #1524 plan recommends no new GitHub Actions workflow; deterministic gates run as plain cargo test on PR, and any wall-clock latency gate follows the s3-snapshot-pressure-gate.yml nightly-only precedent.

<!-- citations: [^129d2-62] [^f2605-5] [^019ed-6] [^019ed-20] [^019ed-41] [^019ed-51] [^019ed-97] [^019ed-109] [^019ed-114] -->
## Documentation Standards

Documentation must follow Basecamp/37signals style: direct, opinionated, intent-heavy, no hedging, no internal jargon visible to builders, short sentences, strong verbs, philosophy-heavy not code-heavy. Copywriting for documentation and landing pages must use Opus agents. All 'bible'/'RMP bible' jargon must be removed from builder-facing and product-spec documentation, replaced with doctrine citations (D0–D10) or plain language. NIP numbers and kind codes must be explained inline on first use in developer-facing documentation. <!-- [^56d21-2] -->

## Doctrine Presentation

D0–D10 doctrines are constraints enforced by the type system, not guidelines or suggestions. The docs/product-spec/doctrine.md opener leads with concrete examples of what doctrines prevent (D10 stops DM routing to public relays, D1 stops spinners, D3 stops manual relay picking) before describing their structure. Each doctrine entry in the rewritten doctrine.md has: plain-English headline → plain lead paragraph → plain-English 'rules out' bullets → italic Implementation detail block for technical specifics. <!-- [^56d21-3] -->

## Builder Guide Structure

The builder guide chapter 00 (00-how-to-read.md) explicitly states the promise: 'if you follow the patterns here, the hardest classes of Nostr bugs become structurally impossible'. The docs/product-spec/overview-and-dx.md section 1 leads with the key insight: 'The framework treats common Nostr-correctness failures as product defects in the framework rather than as developer mistakes'. The builder guide chapter 01 includes a 'What you stop writing' section listing 8 specific things the framework handles so developers don't have to. <!-- [^56d21-4] -->

## Testing and Build Standards

Local cargo test runs must be scoped to the crates touched, not the whole workspace; cargo test --workspace is reserved for the merging agent and CI. cargo test -p nmp-testing --test doctrine_lint_smoke must always be run locally because D-rule gates trip silently in scoped cargo test runs. After renaming a public symbol, moving a module, changing a Cargo.toml dep path, or adding a workspace member, cargo build --workspace (compile-only) must be run locally. The `tests/helpers/mod.rs` subdirectory layout is required because a flat `tests/helpers.rs` would be compiled as its own integration test binary by Cargo. D6 doctrine prohibits `.expect()` in protocol crates — must return `Result` instead. <!-- [^019ed-8] -->

## File Size and Module Structure

Hand-authored source and documentation files must be kept under 300 lines of code where practical; 500 lines of code is a hard ceiling that must not be exceeded. When files approach the soft LOC limit, they must be split by cohesive ownership using feature modules, sibling views, or linked docs rather than large catch-all files; inline tests must be split into sibling test files to meet this constraint. The baseline must never be raised. Top-level model/, update/, view/, state/, or actions/ buckets whose only purpose is technical role separation must not be created; features must be organized by cohesive owner (feature, page, view module, protocol module, or central domain type), keeping state, messages, reducer, view, and tests near that owner. Generated, vendored, lockfile, binary, and benchmark-output artifacts are exempt from the LOC ceiling, but their producer files must remain small and documented. When a cohesive owner approaches the LOC limit, split under the same owner namespace by concrete sub-type or sub-protocol rather than by recreating global Model/Update/View layers. The top-level actor/router must remain flat until a screen or module has genuinely self-contained state; nested messages must be composed deliberately without introducing native/local component state just to avoid plumbing. When splitting is needed, new modules are declared in mod.rs (not as submodules of the file being split), and the baseline is never raised.

<!-- citations: [^129d2-61] [^019ed-9] [^019ed-43] [^019ed-70] [^129d2-101] [^019ed-99] [^129d2-108] [^019ed-110] [^11850-135] [^019ed-154] -->
## Native Boundaries

No native domain logic is permitted; if an if statement in Swift, Kotlin, or any native language decides what the app should do (not how it should look), that logic belongs in Rust. Native code has exactly three responsibilities (aim.md §2 #4): render (translate Rust-produced state snapshots into UI); execute capabilities (call OS APIs and report raw results back to Rust, never deciding policy, retrying, or caching); and hold ephemeral presentation state (spinners keyed to correlation ids, scroll position, focus, input-buffer text, animation state, per-platform icon/color choices — state that no other platform would have to reimplement to stay correct). The discriminating test: would a second platform have to reimplement this to stay correct? If yes → Rust; if only-how-it-looks → shell. Every external effect must be represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state. New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams; reducers must remain replayable from message history. Debug/history surfaces must use log-safe action tags and correlation ids and must never record secrets, raw nsecs, plaintext DMs, or bearer tokens. <!-- [^019ed-44] -->
