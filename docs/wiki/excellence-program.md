---
title: Excellence Program
slug: excellence-program
topic: codebase-patterns
summary: "The excellence program identifies six repo-wide patterns found by the reviewers: superseded generations never deleted, presence-is-not-coverage, invariants by c"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Excellence Program

## Program Overview

The excellence program identifies six repo-wide patterns found by the reviewers: superseded generations never deleted, presence-is-not-coverage, invariants by convention not construction, feedback loops built but starved, ambient authority creep, and bunker second-classness as systemic not incidental. Across sixteen reviews the verdict distribution was 1 SOUND, 14 SOUND-WITH-RESERVATIONS, 1 QUESTIONABLE, zero EXCELLENT, and zero WRONG-SHAPE. <!-- [^2e544-56] -->

## Regression Gate

A bunker parity matrix (every journey acceptance test × {local, bunker}) is the regression gate that makes P6 unable to silently regress. <!-- [^2e544-57] -->

## Out of Scope

The excellence program explicitly does NOT build: a saga coordinator for zaps, a delta protocol, a LateWiring diagnostic for #618, per-envelope bunker unseal RPC, or a big-bang expected-coverage optimizer rewrite. <!-- [^2e544-58] -->

## Wrong-Shaped Fixes

Five queued fixes were flagged as wrong-shaped: #1090 (should be derived pin-set into gc_step plus eviction-watermark co-land, not persisted claims), V-08 Stage 3 (needs session capability or explicit policy, not per-envelope RPC), #618 (failure should be made inexpressible by spawn-at-start, not diagnosed), ADR-0036 (documents a topology never built, needs supersession), and the publish-auth M6 plan (should use existing availability-gate seam, not a separate reauth budget). <!-- [^2e544-59] -->
