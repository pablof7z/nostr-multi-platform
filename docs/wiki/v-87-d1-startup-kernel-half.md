---
title: V-87 — D1 Startup Ordering Kernel Half (HIGH · D1)
slug: v-87-d1-startup-kernel-half
summary: "V-87 D1 startup ordering kernel half (HIGH D1): pre-flight snapshot with resume_rev_after_preflight to fix rev-collision, PR #835."
tags:
  - backlog
  - V-87
  - D1
  - startup
  - offline-first
  - rev-collision
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-87 — D1 Startup Ordering Kernel Half (HIGH · D1)

> V-87 D1 startup ordering kernel half (HIGH D1): pre-flight snapshot with resume_rev_after_preflight to fix rev-collision, PR #835.

## Overview

V-87 is a HIGH-priority D1 offline-first backlog item: fix the kernel startup ordering so the first snapshot is emitted before any relay I/O. The agent's analysis found that the blocking recv() (#601) exists to win a storage-path init race — nmp_app_set_storage_path must arrive before the real kernel builds, or LMDB is silently ignored. The fix emits a pre-flight snapshot from a temporary bare kernel before the recv, then builds the real kernel post-recv with the correct storage path. [^4edd4-149]

## Review — BLOCKED on Rev-Collision Defect

The first review BLOCKED the PR with a superb catch that 1034 passing tests missed: a rev-collision protocol defect. The throwaway pre_kernel emits rev=1, and the real kernel's first Start-arm emit also produces rev=1. The shipping iOS host's guard update.rev > rev (KernelModel.swift:643) silently drops the real running=true frame — and offline (no relays/events), changed_since_emit is false afterward, so the host stays stuck on the running=false pre-flight frame forever. The tests passed because they used raw channel receives without the rev-guard the live host applies. [^4edd4-150]

## Fix — resume_rev_after_preflight

The fix agent resolved the rev-collision cleanly: resume_rev_after_preflight(floor) reads preflight_rev from live state and advances the real kernel so the Start frame carries rev=2 > 1, passing the host's guard update.rev > rev — a by-construction invariant. This prevents the host from silently dropping the real running=true frame. Storage-path race preserved (resume called post-recv), strengthened test fails-without/passes-with, and the bunker test was confirmed the known flake (2/2 pass, scheduling-neutral change).

<!-- citations: [^4edd4-151] [^4edd4-238] -->
## Re-Review and Merge

The re-review APPROVED with all 5 points verified: rev invariant guaranteed by construction, storage-path race intact, test genuinely discriminates the failure, and the pre-flight frame is startup-only so Reset is correctly orthogonal. Cargo test passed, and V-87 landed as PR #835 (merged at master 3831ae75). [^4edd4-152]

## See Also

