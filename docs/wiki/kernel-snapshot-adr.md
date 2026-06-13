---
title: Kernel Snapshot ADR
slug: kernel-snapshot-adr
topic: kernel-snapshot
summary: The full-kernel-snapshot emission model re-encodes every projection every dirty tick (O(state) per tick); the ADR-0037 typed sidecar made each re-encode cheaper
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Kernel Snapshot ADR

## Kernel Snapshot Architecture

The full-kernel-snapshot emission model re-encodes every projection every dirty tick (O(state) per tick); the ADR-0037 typed sidecar made each re-encode cheaper but did not make it incremental. This is a deliberate documented architectural bet (ADR-0037) and the highest-risk performance decision; it is not diffed or gated per-projection by design. This simplicity collapses an entire desync bug class, but per-projection revision gating is the minimum acceptable improvement and the correct middle path: re-emit only changed projections while keeping the snapshot/rev correctness invariant. The false binary of full-snapshots versus fragile hand-written deltas ignores this option. Existing generic-JSON projections already have a change-gate (snapshot_registry.rs); typed sidecars and kernel built-ins lack any unchanged-reuse-prior-buffer mechanism.

The KCEV FlatBuffer is deliberately protocol-agnostic — kind rides as an opaque uint with no protocol branching in the kernel-owned buffer, as documented in claimed_events.fbs:31-34.

ADR-0036 is superseded to document that the kernel owns follow-to-interest expansion (not the composition root), replacing the misleading accepted version. WireDelta (delta protocol) is not rebuilt; the snapshot=broadcast model is empirically validated and retained.

ADR-0039's rejection of host-declared projection subscriptions is a category error: declaring which projections a host consumes is static resource ownership (the output-side sibling of push_interest), not view-state leakage; relay_diagnostics shipping 4×/sec to every host is unjustified permanent waste.

Relay diagnostics projections must ship raw timestamps over the wire; shells format relative-time strings at render time (aim.md §62 forbids format_ago_* inside projection builders).

The Android KernelProfileHost uses remember(model, profiles) where profiles is a new Map object on every snapshot tick, causing the host to be recreated every tick and triggering a claim/release churn loop in DisposableEffect (same bug class as chirp-web commit 4d1888f9a). The fix for Android profile-claim churn is to remove profileHost from the DisposableEffect key in NostrAvatar and NostrProfileName, and stabilize KernelProfileHost by keying remember on model only with rememberUpdatedState for the profiles map.

The DmConversationListScreen double-collects model.state independently of its parent, causing profiles and conversations to potentially reflect different snapshot generations.

<!-- citations: [^02745-86] [^02745-87] [^78c8e-21] [^2e544-29] [^78c8e-49] [^78c8e-66] [^02745-103] [^78c8e-84] [^78c8e-102] -->
