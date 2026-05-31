---
title: Consolidated Backlog Document (docs/BACKLOG.md)
slug: backlog-document
summary: A single consolidated backlog document at docs/BACKLOG.md is the authoritative source for all pending work, violations, and execution plans in the project
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-29
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Consolidated Backlog Document (docs/BACKLOG.md)

## Overview

A single consolidated backlog document at docs/BACKLOG.md is the canonical violation and backlog tracker, containing Active Violations (Section 1), Pending User Decisions (Section 3), V1 Feature Backlog (Section 4), and Post-V1 items (Section 5). Completed items are removed from the document upon completion, not marked DONE, to keep the backlog genuinely empty of completed work; verbose implementation notes are not dumped there. It serves as the tactical queue tracking active violations, pending user decisions, and ordered V1 features. Any mock, stub, or 'for now' hack that deviates from PERFECT architectural execution is COMPLETELY FORBIDDEN and must be FIXED RIGHT AWAY. The document includes a hard invariant FUNDAMENTAL RULE with an explicit staged-fix corollary for multi-week work. It provides an agent entry point instructing agents to pick the topmost Section 4 item not in Section 2. Review agent findings are placed directly into BACKLOG.md subsections rather than creating scattered docs/perf/reviews/ files. Newly discovered codebase gaps are evaluated against the existing BACKLOG.md to identify genuinely new items before adding them. Backlog prioritization follows the backlog's own stated priority labels (Section 1 HIGH first), with parallel-safety only as a tiebreaker, not as the primary filter.

<!-- citations: [^95d02-1] [^9fc44-1] [^f2605-2] [^cd2b6-3] [^4edd4-2] -->
## Section 1 — Active Violations

Section 1 of BACKLOG.md lists Active Violations: V-01 (nmp-wasm stub, critical), V-02 (nmp-marmot misplaced), V-03 (wallet_status D0 violation, user-gated), V-04 (two subscription systems coexisting, user-gated). NIP-17 namespace violations (nmp.dm.* → nmp.nip17.*) and D0 violations (chirp.follow/unfollow → nmp.follow/nmp.unfollow) are already fixed on current master HEAD and are not listed as active violations. All violation claims in BACKLOG.md are code-verified against current master HEAD with file:line citations. [^95d02-2]

## Section 2 — In Flight

Section 2 of BACKLOG.md lists In Flight work: 3 real open branches verified by git log, plus a note that WIP.md's 4 entries have zero commits ahead of master (merged or abandoned). [^95d02-3]

## Section 3 — Pending User Decisions

Section 3 of BACKLOG.md lists Pending User Decisions: PD-033-A/B/C and PD-37 requiring human choice. [^95d02-4]

## Section 4 — Feature Backlog

Section 4 of BACKLOG.md lists Feature Backlog items F-01 through F-07 ordered by V1 blocking priority. It also captures untracked V1 exit items: an honest cross-platform claim (either wire wasm or rewrite 'cross-platform' in aim.md) and a bespoke-FFI deprecation calendar addressing 48 nmp_app_* symbols vs dispatch_action. Components that would likely work well as primitives or complex components that most applications would recreate must be identified as gallery extraction opportunities and added to BACKLOG.md rather than extracted immediately.

<!-- citations: [^95d02-5] [^9fc44-2] [^9a2c7-1] -->
## Section 5 — Post-V1 Deferrals

Section 5 of BACKLOG.md contains a Post-V1 explicit deferral table. Deferred items include: Nutzap support (NIP-60/61) with no nmp-nip60/61 crates existing on master; Android parity with iOS Chirp, blocked on UniFFI (M14) to avoid hand-maintaining two FFI surfaces; a Nostr-aware UI component registry (NDK svelte/registry-style curated primitives), blocked on stable snapshot projection contracts and a target-platform decision; Chirp TUI unfinished interactions (repost/group-discover/DM-open/add-relay); nmp-content Phase-2 claim dependency channel; wasm32 test infrastructure; and web/registry CodeBlock placeholder.

<!-- citations: [^95d02-6] [^9fc44-3] [^cd2b6-2] -->
## Appendix — Verified-Fixed Items

BACKLOG.md includes an Appendix of verified-fixed items to prevent Opus reviews from re-flagging them. [^95d02-7]

## Superseded Documents

docs/perf/pending-user-decisions.md is superseded by BACKLOG.md Section 3 and has a deprecation banner redirecting to it. docs/arch-review-queue.md is superseded by BACKLOG.md Sections 1 and 4 and has a deprecation banner redirecting to them. WIP.md is superseded by BACKLOG.md Section 2 and has a deprecation banner marking prior entries as merged or abandoned. [^95d02-8]
## See Also

