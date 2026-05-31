---
title: ADR-0040 — Capability-Worker Seam Full Design and Ratification
slug: adr-0040-capability-worker-seam-full-design
summary: "Complete design for ADR-0040: three primitives (PendingSign reuse, lnurl worker reuse, single capability-worker thread), account-switch race solution, fact-check corrections, and user decision to merge as Proposed with implementation deferred."
tags:
  - adr-0040
  - v-90
  - d8
  - capability-worker
  - actor-thread
  - design
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# ADR-0040 — Capability-Worker Seam Full Design and Ratification

> Complete design for ADR-0040: three primitives (PendingSign reuse, lnurl worker reuse, single capability-worker thread), account-switch race solution, fact-check corrections, and user decision to merge as Proposed with implementation deferred.

## Overview

ADR-0040 is the design document for V-90, a HIGH-priority D8 backlog item: move actor-thread blocking sites off-thread. The ADR was drafted by an Opus architect (PR #842), fact-checked, corrected for off-by-one citations, and merged as Proposed per user decision — implementation is deferred. The ADR supersedes ADR-0024 for the native capability class. [^4edd4-173]

## Problem — Three Actor-Blocking Sites

Three sites block the single-writer actor thread, freezing the kernel loop (D8 violation): (1) dm_send.rs — NIP-46 DM gift-wrap op.wait (up to ~24s on a bunker), (2) capability.rs — synchronous iOS Keychain dispatch executed in-actor, (3) identity.rs — cold-start onboarding signs. All three cause visible device freezes that unit tests cannot detect. [^4edd4-174]

## Design — Three Primitives, Only One New

The design uses three primitives: (A) Cold-start signs reuse the existing PendingSign park/settle path — no new mechanism needed. (B) DM op.wait reuses the existing nmp-nip57 lnurl worker pattern: spawn off-actor, re-enter via ActorCommand::PublishSignedEvent — no new mechanism needed. (C) The only genuinely new surface is a single serialized capability-worker thread: FIFO mpsc channel, blocking recv (D8-compliant, never polls), runs the native callback off-actor, re-enters the actor via typed ActorCommand::CapabilityResultReady. Of the three actor-blocking sites, two reuse existing patterns; the new work is one primitive. [^4edd4-175]

## Account-Switch Race — The Hard Part

Per-op thread spawn is rejected because two threads racing the Keychain can reorder persist/forget and corrupt at-rest secrets. A single FIFO worker makes per-account ordering correct by construction: operations for the same account are serialized through one channel. A result for a removed account is dropped (D6 trace), never misapplied to the new account. This is the specific race condition the BACKLOG flagged as the hard part of V-90. [^4edd4-176]

## Fact-Check and Corrections

A fact-check reviewer verified the ADR's citations and reuse claims. Finding: MOSTLY ACCURATE — architecture, both reuse claims, account-switch reasoning, and supersession all sound. But the identity.rs citations were off by one: 826,864,1019 should be 825,863,1018 — they pointed to the comment or next line, not the actual sign_active call. All four occurrences were corrected. Additionally, a useful lnurl re-entry clarification was added so a future implementer isn't misled. The corrections were committed and pushed before merge. [^4edd4-177]

## User Decision — Merge as Proposed, Defer Implementation

The user was given three options for ADR-0040: (1) Ratify — flip status to Accepted, merge, and start implementation PRs; (2) Request changes; (3) Merge as Proposed, defer implementation — land the ADR for the record, implement later. The user chose option 3. This means the ADR is on master as a durable design record, V-90 moves from 'ADR-gated, no actionable path' to 'design ratifiable, implementation plan ready', and the three implementation PRs (DM off-actor, cold-start PendingSign, capability-worker seam last) are deferred until explicitly greenlit. [^4edd4-178]

## Scope and Deliverable

ADR-0040 is a design document only — zero behavior change, no code modifications. The deliverable is a ratifiable ADR PR with an executive summary. Implementation follows as three independently-shippable PRs: DM off-actor (reuse lnurl worker), cold-start PendingSign (reuse existing path), and the capability-worker seam last under fullest test coverage. [^4edd4-179]

## See Also
- [[v-90-adr-0040-capability-worker-seam|V-90 — ADR-0040 Capability-Worker Seam (HIGH · D8, ADR-Gated)]] — related guide
- [[adr-fact-check-citation-ground-truth|ADR Fact-Check — Citation Ground-Truthing Before Merge]] — related guide
- [[session-decision-tree-v90-adr-resolution|Session Decision Tree — V-90 ADR Resolution and User Choices]] — related guide

