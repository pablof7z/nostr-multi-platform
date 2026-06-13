---
title: Coverage Ledger
slug: coverage-ledger
topic: event-acquisition
summary: The coverage ledger (K3) is gated behind a full K2 landing on master, per the user's sequential ordering
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
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Coverage Ledger

## Placement and Sequencing

The coverage ledger (K3) is gated behind a full K2 landing on master, per the user's sequential ordering. K3 is the riskiest surgery because it changes what every standing REQ asks for; a bug either re-fetches the world or suppresses fetches, so it goes last behind two cheap soundness restorations (un-floor NEG-OPEN and unify the shape-to-query predicate). <!-- [^2e544-51] -->

## Oracles

The convergence property test (P2-d) and fixture-relay journey (follow-after-thread-reply backfills the author's history) are the oracles for the coverage ledger. <!-- [^2e544-52] -->

## Un-flooring NEG-OPEN (K3 Rung 2.1)

Un-flooring NEG-OPEN makes findings 1-3 self-healing for all NEG-eligible shapes without touching watermark code, because reconciliation covers the full window instead of inheriting the presence-derived floor. <!-- [^2e544-53] -->

## Sync One-Change Fix

The fix is inverting the composition order: when the NIP-77 interceptor claims a frame, reconcile the un-floored window, keeping the floor only on plain REQs. <!-- [^2e544-54] -->

## Disabled Guards

The GC honest-budget Phase-3 hourly gate and the HOT_EVENT_CEILING are disabled until store-claims are wired (tracked in #1090), with the cursor livelock edge case tracked in #1097. <!-- [^da6b1-68] -->
