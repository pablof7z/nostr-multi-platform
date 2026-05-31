---
title: V-90 — ADR-0040 Capability-Worker Seam (HIGH · D8, ADR-Gated)
slug: v-90-adr-0040-capability-worker-seam
summary: "V-90 ADR-0040 capability-worker seam (HIGH D8): ADR-gated design to offload actor-thread blocking in dm_send and Keychain dispatch."
tags:
  - backlog
  - V-90
  - ADR-0040
  - D8
  - actor
  - blocking
  - capability
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-90 — ADR-0040 Capability-Worker Seam (HIGH · D8, ADR-Gated)

> V-90 ADR-0040 capability-worker seam (HIGH D8): ADR-gated design to offload actor-thread blocking in dm_send and Keychain dispatch.

## Overview

V-90 is a HIGH-priority D8 backlog item: move the two actor-thread blocking sites off-thread to prevent visible device freezes. The blocker is a design decision — the capability-worker seam needs an ADR ratified before any code is written. The user chose option 1: draft ADR-0040 (capability-worker seam) so the highest-priority blocked HIGH item becomes actionable. [^4edd4-160]

## Design Scope

ADR-0040 addresses two actor-blocking sites: (1) dm_send.rs:221 — op.wait(12s) for NIP-46 remote-signer gift-wrap; (2) capability.rs:62 — dispatch_capability for iOS Keychain blocking. The design grounds itself in the existing signer-broker offload precedent and specifically solves the account-switch race (the hard part the BACKLOG flagged) plus D8 compliance: blocking-recv worker, never polling; results re-enter via ActorCommand to preserve the actor single-writer invariant. [^4edd4-161]


The user chose to merge ADR-0040 as Proposed (option 3), deferring implementation. The ADR is on master as a durable design record. Three independently-shippable implementation PRs are planned: DM off-actor (reuse existing nmp-nip57 lnurl worker pattern), cold-start PendingSign (reuse existing park/settle path), and the capability-worker seam last under fullest test coverage. The capability-worker is a single serialized thread using FIFO mpsc, blocking recv (D8-compliant, never polls) that re-enters the actor via typed ActorCommand::CapabilityResultReady. The design explicitly rejects per-op thread spawn because two threads racing the Keychain can reorder persist/forget and corrupt at-rest secrets. The single FIFO worker makes per-account ordering correct by construction. [^4edd4-196]

V-14 requires a host-visible `BunkerConnectionState` projection so relay-flap session drops are visible, making NIP-46 viable as a first-class v1 sign-in method. [^4edd4-214]
## Deliverable

An Opus architect is drafting ADR-0040 as a ratifiable ADR PR with a 5-line executive summary for quick approval. The ADR itself contains no code or behavior changes — the implementation follows as a separate gated PR after ratification. The ADR number is 0040, following the existing ADR convention (next after 0037/0039). [^4edd4-162]

## See Also
- [[adr-0040-capability-worker-seam-full-design|ADR-0040 — Capability-Worker Seam Full Design and Ratification]] — related guide
- [[session-decision-tree-v90-adr-resolution|Session Decision Tree — V-90 ADR Resolution and User Choices]] — related guide

