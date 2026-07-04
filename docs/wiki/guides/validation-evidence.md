---
title: Validation Evidence and Screenshot Policy
slug: validation-evidence
topic: dx-proof
summary: Validation is performed by driving the real apps on simulators, emulators, and Playwright â not merely compiling or running unit tests
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Validation Evidence and Screenshot Policy

## Validation Method

Validation is performed by driving the real apps on simulators, emulators, and Playwright — not merely compiling or running unit tests. This is the required validation method because it surfaces latent bugs that all unit and CI tests stay green through. App work must not be declared done from one screen or one platform; every screen on every platform must be driven with screenshot evidence.

The iOS test plan (chirp#60) contains 70 scenarios (S01–S70). Each scenario includes an acceptance list, required performance metrics (startup, CPU, memory, scroll, and render), relay-provenance checks, and offline behavior criteria. Search was deleted from Chirp (chirp#27), so no search scenarios are included in the validation test plan.

A 30-minute heartbeat cadence is enforced during the iOS validation sweep to verify that real evidence is actually landing — created issues, per-scenario screenshots with perf metrics and relay provenance, and filed-and-fixed-and-reverified bugs — rather than just agents 'running.'

Haiku tester agents are unreliable for iOS simulator validation (50% hand-wave rate: placeholder comments, no real screenshots, bailed on sign-in); Sonnet is the minimum viable tester model for real evidence.

Each test scenario must post one PASS/FAIL comment with a screenshot to chirp#60 on GitHub; bug findings are filed as separate GitHub issues, fixed, and reverified with after-screenshots.

An Opus final-review agent reviews every screenshot from the validation sweep and sends anything short of perfect (padding/margins, data, relay diagnostics, or perf) back to be fixed and reverified before any scenario is called done.

iOS validation testers are serialized (max 2 concurrent sim-drivers) because concurrent drivers corrupt each other through the shared xcode MCP session profile; each agent must re-assert its sim UDID per call.

The Chirp iOS validation sweep discovered 20 real bugs across rendering, write-publish, DMs, Groups/Marmot, accessibility, and diagnostics honesty.

<!-- citations: [^dcc80-3b8c9] [^dcc80-32ef6] [^dcc80-45081] [^dcc80-3adcd] [^dcc80-7269a] [^dcc80-b2246] [^dcc80-2057c] [^dcc80-3a2d3] [^dcc80-a24c6] -->
## Evidence Storage and Referencing

Validation screenshots are committed to the Chirp repo branch and referenced by raw GitHub URL on the issues — not hosted on claude.ai links — so they persist. Validation evidence for each platform is committed under `docs/validation/{platform}/` directories in the Chirp repo.

<!-- citations: [^dcc80-67590] [^dcc80-079e7] [^dcc80-ccbd5] -->
## Offline Scenario Setup

Offline validation scenarios put the iOS simulator offline via simctl, never the host machine.

<!-- citations: [^dcc80-45df2] [^dcc80-0ccfa] [^dcc80-2ddb7] -->
## iOS Seeded Local Relay

The iOS validation seeded local relay uses ws://127.0.0.1:10547 with 16 events: two profiles, a follow graph, all content types (link/image/video/hashtag/mention/quote/long note), and a target note with exactly 2 replies, 3 reactions, and 1 repost so testers can verify count correctness.

<!-- citations: [^dcc80-70fe0] [^dcc80-31bae] -->
## Read-Model Validation

The Chirp read-model collapse is proven sound end-to-end through the real Rust shell (C-ABI, compose, difference resolution, decode, render) at the exact pinned NMP rev (fa49d00c), including a real-shell C-ABI test that surfaces a followed non-self author's notes. <!-- [^dcc80-0ec72] -->
