---
title: Excellence Program
slug: excellence-program
topic: codebase-patterns
summary: "The excellence program identifies six repo-wide patterns and defines EXCELLENT per pattern: exactly one production mechanism per capability (P1), since floors t"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Excellence Program

## Program Overview

The excellence program identifies six repo-wide patterns and defines EXCELLENT per pattern: exactly one production mechanism per capability (P1), since floors trace to recorded coverage facts not event presence (P2), invariants enforced by construction not convention (P3), every feedback input the planner declares has a production writer (P4), zero process-global mutable state in production crates and no consumer needing only a pubkey can reach secret material (P5), and a CI parity matrix runs every journey test with backend ∈ {local, bunker} (P6). A `mechanism_census` test in nmp-testing asserts per-capability mechanism counts and fails CI when a second generation appears. A dormant-surface inventory test lists intentionally-unwired public surfaces with issue links and deadlines; the test fails on unregistered additions, with the goal state being an empty inventory. Across sixteen reviews the verdict distribution was 1 SOUND, 14 SOUND-WITH-RESERVATIONS, 1 QUESTIONABLE, zero EXCELLENT, and zero WRONG-SHAPE.

<!-- citations: [^2e544-56] [^2e544-390] [^2e544-464] -->
## Regression Gate

A bunker parity matrix (every journey acceptance test × {local, bunker}) is the regression gate that makes P6 unable to silently regress. <!-- [^2e544-57] -->

## Out of Scope

The excellence program explicitly does NOT build: a saga coordinator for zaps, a delta protocol, a LateWiring runtime diagnostic for #618, per-envelope bunker unseal RPC, a full expected-coverage optimizer + hysteresis, per-projection dirty-tracking rework, or a multi-session bunker broker before the correlation token exists.

<!-- citations: [^2e544-58] [^2e544-391] -->
## Wrong-Shaped Fixes

Five queued fixes were flagged as wrong-shaped: #1090 (should be derived pin-set into gc_step plus eviction-watermark co-land, not persisted claims), V-08 Stage 3 (needs session capability or explicit policy, not per-envelope RPC), #618 (failure should be made inexpressible by spawn-at-start, not diagnosed), ADR-0036 (documents a topology never built, needs supersession), and the publish-auth M6 plan (should use existing availability-gate seam, not a separate reauth budget). <!-- [^2e544-59] -->
