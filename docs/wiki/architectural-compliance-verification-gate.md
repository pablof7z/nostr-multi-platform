---
title: Architectural Compliance Verification Gate — Verify Before Implementing
slug: architectural-compliance-verification-gate
summary: Before implementing in Chirp iOS, every plan must be explicitly verified against all project doctrines (D8, ADR-0025, one-way principle, ADR-0037, component-owned reactivity) and the user must sign off before agents are dispatched.
tags:
  - ios
  - chirp
  - workflow
  - architecture
  - doctrine
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Architectural Compliance Verification Gate — Verify Before Implementing

> Before implementing in Chirp iOS, every plan must be explicitly verified against all project doctrines (D8, ADR-0025, one-way principle, ADR-0037, component-owned reactivity) and the user must sign off before agents are dispatched.

## Overview

Before any implementation work begins on Chirp iOS, the proposed plan must be explicitly verified against all relevant project doctrines. The user must confirm the architecture is compliant before agents are dispatched. Skipping this gate risks implementing work that must later be unwound. [^38935-35]

## Required Verifications

Every plan must be checked against: D8 (no polling) — the implementation must consume kernel-pushed snapshots reactively, never via sleep+check loops; ADR-0025 (no bespoke pull symbols) — the implementation must use the existing projection registry seam, minting zero new FFI symbols; One-way principle — one mechanism for projections, the plan must not introduce a second path; ADR-0037 (typed transport) — the implementation must read typed Swift structs from SnapshotProjections, not raw JSON; Component-owned reactivity — components must signal their own data requirements via claim/release, the kernel must never pre-fetch. Additional doctrines may apply depending on the specific plan. [^38935-36]

## User Sign-Off

The user must explicitly approve the plan after architectural compliance is verified. In the embed system case, the user asked 'is that architecture in line with how we want to do things per all the rules of the project?' and only after the assistant verified every doctrine did the user say 'ok, go ahead then.' Do not dispatch agents until this explicit sign-off is received. [^38935-37]


When an implementation effort has lost focus or produced surface-level work that misses the architectural goal, the user may gate further work behind a high-level goal statement. The user asks for a "high-level goal so that I can gate your understanding, very high-level, ambitious and, most importantly, RIGHT (no hacks)." The assistant must articulate the thesis in positive, ambitious terms — what the system should be, not what went wrong. Only after the user confirms the goal is correct does work proceed. This gate ensures the assistant has internalized the architectural vision before touching code, preventing the pattern where agents produce superficially correct work (catalog parity, visual polish) that fails to address the core architectural problem (business logic duplication, component-owned reactivity, single source of truth). [^6a951-5]
## Verification Before Blocked Claims

Before claiming a feature is blocked on upstream changes (e.g., Rust codegen), verify that the C FFI symbols exist in NmpCore.h, that the kernel already emits the relevant projection data, and that KernelTypes.generated.swift can accept manual field additions. The embed system was initially claimed to be 'blocked on Rust' — this claim was incorrect and was disproven when the user challenged it. The C FFI symbols were already present and the work was pure Swift. [^38935-38]

## See Also
- [[chirp-ios-embed-system-implementation|Chirp iOS Embed System — Implementation and Architecture]] — related guide
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[one-way-principle|One-Way Principle — Avoid Multiple Mechanisms for the Same Concern]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide

