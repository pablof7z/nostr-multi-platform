---
title: Triage Workflow and Agent Dispatch
slug: triage-workflow
topic: agent-coordination
summary: "Issue triage uses a two-tier agent strategy: sonnet agents PR + land straightforward slam-dunks, while opus agents review subjective issues with authority to cl"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Triage Workflow and Agent Dispatch

## Triage Workflow

Issue triage uses a two-tier agent strategy: sonnet agents PR + land straightforward slam-dunks, while opus agents review subjective issues with authority to close or defer. An issue existing does not mean it should be done — agents must not follow issues blindly and must apply subjective judgment before implementing. Triage must look beyond the issue's own scope estimate when classifying: for example, issue #2928 (content-kind-39000 NIP-29 group card) was re-triaged from slam-dunk to post-v1 after investigation showed it requires ~15-20 files across nmp-content/nmp-nip29/4 platforms/nmp-gallery, not the ~5 files the issue implied. All dispatched agents run in the background, worktree-isolated, following repo conventions. Agent fleet dispatch must use isolation: "worktree" param rather than prompt-instructed worktree paths, which are prone to contention when multiple agents share the same directory. Agents are parallelized whenever possible to advance work, and a Fable subagent is used periodically to direct and double-check the assistant's thinking. A 30-minute heartbeat cadence verifies real evidence (created issues, per-scenario screenshots with perf metrics and relay provenance, filed-and-fixed-and-reverified bugs) rather than just agents running. An Opus agent reviews every screenshot before any scenario is called done; anything short of perfect (padding, margins, data, relay diagnostics, perf) goes back to fix and reverify.

<!-- citations: [^d8bc6-e7f0e] [^d8bc6-07ed2] [^d8bc6-4da68] [^d8bc6-e8533] [^dcc80-609ad] -->
