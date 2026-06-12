---
title: NMP Scope and Roadmap Decisions
slug: scope-and-roadmap
topic: project-governance
summary: NMP's v1 scope excludes web (IndexedDB, Worker port), per the owner's Decision A
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:bbd5fe79-cd71-4de0-ba9f-f3684520a03f
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# NMP Scope and Roadmap Decisions

## v1 Scope

NMP's v1 scope excludes web (IndexedDB, Worker port); aim.md, plan.md, and README are reworded to reflect web as a preview, not a v1 exit criterion. WASM-related backlog items (F-01 IndexedDB store, F-06 wasm cross-platform) are demoted to post-v1 and stripped from other backlog entries (V-51 Phase 3, F-09 chirp-web, F-10 acceptance); Section 5 of BACKLOG.md is the post-v1 bucket rather than a separate file. Zap receipt verification (nostrPubkey extraction) is deferred to post-v1 per owner decision; shipped zap capability is sufficient for v1. F-05 typed coverage is a v1-blocker with the strict 28-binding + PR-B scope. F-00 app directory unification must not block v1 work. F-10 is DONE with measured frame-size reduction numbers; #979 and #991 are closed and removed from the v1-blocker list. The `phase:v1-blocker` label is applied to issues #977, #978, #979, and #1069 so the gate is machine-visible to the agent swarm. PD-033-A (#975) is closed, with external consumers (podcast-player, hl, win-the-day) documented as the second-app evidence, per owner's Decision B. BACKLOG.md Section 4 lists F-02, F-04, F-05, F-08, F-09, F-10 after the rebase resolution. #968 (V-51 routing observability) carries a p1+post-v1 contradiction; the wasm debug-toolbar portion is clearly post-v1 but the iOS Phase 3 inspector leg may need to cross v1, awaiting owner decision. The nmp-wasm facade is not a working browser runtime: OpenView logs 'interest not compiled', open_interest is not exposed to wasm, the write path only handles PublishNote and rejects everything else, the web worker protocol has no SetSigner, and the committed wasm artifact does not export dispatch_app_action_async. The podcast player (Pod0) is a separate repo that consumes NMP as a pinned git dependency, not a workspace member inside the NMP repo. The podcast player's Rust architecture is exemplary for an NMP consumer: single register_defaults composition root, domain crates, a clean nmp_app_podcast_* FFI surface, ActionModule implementations for writes, and kernel projections for reads. The podcast player's iOS shell is mid-migration: a legacy Swift-native NostrRelayService (own WebSocket, own relay selection, own REQ/EVENT framing) runs in parallel with the correct kernel-routed NostrRelayCapability. Every deviation in the podcast player's iOS shell (Swift relay service, hardcoded relay URLs, Swift-only state mutations, NIP-46 bunker signing) is already tracked in BACKLOG.md or the feature-parity migration plan, behind a deletion gate. The highest-leverage alignment work for the podcast player is porting the NostrAgentResponder LLM/relay autopilot into the kernel, which would eliminate the second Swift Nostr stack.

<!-- citations: [^b4fe9-8] [^b4fe9-9] [^b4fe9-10] [^b4fe9-11] [^b4fe9-12] [^da6b1-15] [^da6b1-16] [^da6b1-17] [^bbd5f-8] [^f1b74-7] [^da6b1-33] [^da6b1-80] -->
## Doctrine-Lint D9 Scope Heuristic

The `/crates/nmp-` substring no longer distinguishes protocol crates from app crates; doctrine-lint D9 scope heuristic must exclude `apps/` first. <!-- [^bbd5f-9] -->

## Process Decisions

NMP should incorporate a comparative-research step into its ADR process before design decisions, surveying how multiple implementations handle the topic. <!-- [^954c5-6] -->
