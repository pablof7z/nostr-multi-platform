---
title: NMP Scope and Roadmap Decisions
slug: project-governance
topic: project-governance
summary: All work must flow through isolated worktrees; committing directly on the main checkout is forbidden.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-12
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:89070aba-0e77-4da3-99e1-322addb1c747
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
  - session:bbd5fe79-cd71-4de0-ba9f-f3684520a03f
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
  - session:65edf39e-4cfd-4bfc-9b65-ec4dc1944b1e
  - session:bc280895-beb9-4575-a06e-027987d2a4a8
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# NMP Scope and Roadmap Decisions

## Repo Discipline

NMP apps are located in ~/src and ~/Work. All work must flow through isolated worktrees; committing directly on the main checkout is forbidden. Agent worktrees are reused for existing branch checkouts rather than creating duplicate checkouts in the main working tree. Commits are scoped to specific pathspecs (e.g., `deny.toml` only) rather than whole-tree commits. The `.claire/` directory is added to `.gitignore`. Git's `merge.renameLimit` and `diff.renameLimit` must be raised before rebasing directory-move branches to let git's rename detection auto-carry edits into relocated files. A comparative-research step surveying how other implementations handle a design decision should be added to NMP's ADR process before the pending M2 open_interest ADR. An ADR is warranted for the M2 surface evolution (5-for-2 symbol swap) rather than the one-line PR note path, since it completes a scheduled migration.

The builder guide/conformance story should be a single ordered narrative document rather than scattered reference docs. NMP should not adopt literate tangling (markdown as source of truth) because a monorepo with 30+ crates and parallel agent worktrees would conflict with it.

<!-- citations: [^954c5-5] [^89070-1] [^37035-17] [^bbd5f-6] [^65edf-3] [^bc280-1] [^954c5-23] -->
## Housekeeping

The dead `build-android-gallery-bundle.rs` file and obsolete BACKLOG entries F-CR-02/F-CR-07 should be deleted. PR #899 must be merged before PR #903 to avoid textual conflicts in builder-guide and Chirp Swift. PR #903 (F-00 directory layout) is closed as superseded by master commits 47add568, 295d49f9, and e17b6983. Superseded PRs are closed rather than force-pushed with conflict resolutions that would duplicate already-shipped features or produce non-compiling states. PR #940 is closed as superseded by master commit 50041a87. PR #941 is closed as superseded by master commits 50041a87 and 2b82591d. PR #910 was rebased to drop the dead calendar-file hunk (the file PR #913 deleted), resolving the modify/delete conflict, and is now mergeable. PD-033-A (#975) is closed with external consumers (podcast-player, win-the-day, hl) documented in docs/architecture/external-consumers.md. F-05 (#979) and F-10 (#991) are promoted to phase:v1-blocker with the 28-binding + PR-B scope, Android first. The phase:v1-blocker label was applied to #977, #978, #979, #991, and #1069 so the gate is machine-visible to the agent swarm. Four would-be regressions were caught pre-merge by Opus review: silent open-view-pin breakage from conflict-free merge, pin-set gap blanking live threads, perf-gate flake calibration, and example-compile break from untested gap class.

<!-- citations: [^bbd5f-7] [^b4fe9-4] [^65edf-4] [^da6b1-55] [^da6b1-89] -->
