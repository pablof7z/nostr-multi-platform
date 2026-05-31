---
title: Builder Guide Writing Conventions
slug: builder-guide-conventions
summary: "The documentation build session is file-disjoint from the primary orchestrator session: it creates only new files matching docs/builder-guide/NN-*.md and must n"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-30
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
---

# Builder Guide Writing Conventions

## File Scope and Boundaries

The documentation build session is file-disjoint from the primary orchestrator session: it creates only new files matching docs/builder-guide/NN-*.md and must not touch crates/, apps/, ios/, Cargo.*, docs/builder-guide/PLAN.md, or any docs/ path outside docs/builder-guide/NN-*.md. All docs in docs/design/, docs/decisions/, and docs/builder-guide/ must be kept. Writer agents must not edit PLAN.md, even if they encounter a genuine ambiguity PLAN does not resolve; instead they must make a call, document it inline, and note it in section 27. Builder guide sections must not reference the removed v2 traits: DomainModule, ViewModule, IdentityModule, and ModuleRegistry. Stale link texts referencing '5 trait families' or '05-substrate-traits.md' across the builder guide must be updated to reflect the current architecture.

<!-- citations: [^7f0f0-2] [^7f0f0-3] [^44c6c-1] [^c3f75-1] -->
## Section Size and Structure

Each builder guide section file must not exceed 300 lines of code, splitting into sub-files (e.g., 05a/05b) if the budget is exceeded. Every builder guide section must include at least 3 anti-patterns and at least 2 concrete deliverables. Each builder guide file must end with a 'See also:' line listing cross-references in the format `[NN — title](NN-name.md)`. The fixture-todo-core walkthrough (§05b) must use real codebase patterns including OnceLock, Arc<Mutex<>>, and codegen exports (pub const ACTION_NAMESPACE, pub type Store). The microblog walkthrough (§19a/b) must use compiling APIs including ActorCommand::PublishNote and the correct KernelEventObserver signature.

<!-- citations: [^7f0f0-4] [^c3f75-2] -->
## Citation Integrity

Writer agents must verify every cited `path:line` by reading the file at the current master tip. If a citation is wrong or has drifted, the writer must fix it in place within its section file and add a row to section 27's discrepancy register. [^7f0f0-5]

## Prose Quality and Honesty

Builder guide documentation must not contain TODO, FIXME, or placeholder prose. Honest status flags are mandatory; M14 UniFFI, M16 nmp init, and NmpPodcast-NmpHighlighter scaffolds must not be documented as if shipped. Builder docs must not mark sections teaching removed architecture as SHIPS status.

<!-- citations: [^7f0f0-6] [^c3f75-3] -->
## Section 27 and Discrepancy Handling

Section 27 (discrepancy register) must be written last, after all other sections land, because it aggregates drift that earlier sections discovered. Section §27 must contain entries for the removal of ViewModule, IdentityModule, ModuleRegistry, and the ActionModule step-machine redesign.

<!-- citations: [^7f0f0-7] [^c3f75-7] -->
## Git Workflow

Writer agents self-push their files via `git fetch origin master && git rebase origin/master && git push origin HEAD:master`, retrying once on non-fast-forward, and never force-pushing. [^7f0f0-8]

## How to Add a Projection

Builder guide documentation must include a positive 'How to add a projection' section teaching the `register_snapshot_projection` seam, replacing the current gap that causes builders to copy the bespoke pull-symbol anti-pattern. [^d0690-1]

## Codegen Section Requirements

The codegen section (§15) must document 9 generated files including envelope.rs, not 7. The codegen section (§15) must describe FfiApp as a live entry-point, not a stub. [^c3f75-4]

## Glossary Requirements

The glossary (§23) must mark ViewModule, ModuleRegistry, DomainModule, and IdentityModule as [removed] and must include entries for KernelEventObserver, ActionModule, and snapshot projection. [^c3f75-5]

## Reference Card Requirements

Section §24 reference cards must display the correct trait table reflecting the current NmpApp seams rather than removed types like ViewModule. [^c3f75-6]
## See Also

