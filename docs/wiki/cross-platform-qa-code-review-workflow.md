---
title: Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize
slug: cross-platform-qa-code-review-workflow
summary: "QA deployment + parallel code-review fan-out pattern: deploy Haiku agent to build/run the app, fan out 8 agents each owning a single concern, and a synthesis agent to aggregate findings into a prioritized report."
tags:
  - android
  - qa
  - code-review
  - multi-agent
  - cross-platform
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize

> QA deployment + parallel code-review fan-out pattern: deploy Haiku agent to build/run the app, fan out 8 agents each owning a single concern, and a synthesis agent to aggregate findings into a prioritized report.

Purpose

After fanning out large-scale implementation work across platforms, a QA + code-review phase validates that everything actually works end-to-end and that the code conforms to architectural doctrine. This uses a two-tier fan-out: one Haiku QA agent to build and run the app, plus multiple parallel code-review agents each owning a single concern, with a synthesis agent aggregating findings. [^f3d8d-40]

QA Agent — Build and Run

A Haiku QA agent is deployed with full access to ADB and emulator. It performs: (1) code-review of every Android screen for claim/release lifecycle, profile merge order, action JSON correctness, and registry hardcoding; (2) Gradle build of both Chirp Android and nmp-gallery Android; (3) emulator launch, APK install, screenshot every tab, and adb log inspection for errors; (4) report of what actually renders vs. what's broken, with file:line citations for any issues. [^f3d8d-41]

Code-Review Fan-Out — Single Concern Per Agent

Code review is fanned out across 8 parallel agents, each owning exactly one concern. This ensures deep focus: an agent looking only at claim/release patterns will catch issues that a generalist would miss. The 8 concerns used for the Chirp Android QA were: claim-release (DisposableEffect claim/release in every screen), action-json (KernelModel action JSON shapes vs. typed_api.rs canonical), render-model (Android feed model vs. V-80 OP-centric Rust emitter), profile-merge (Kotlin profile merge order: claimed → author_view → mention), gallery-d8-adr0034 (Gallery: D8 polling timeout + ADR-0034 embed infrastructure), registry (registry hardcoding vs. registry.json source of truth), gallery-screens (Gallery screens: feature completeness + claim/release), and data-flow (full snapshot → StateFlow → UI data chain). [^f3d8d-42]

Synthesis Agent

A 9th synthesis agent aggregates all code-review findings into a single prioritized report. This agent runs after all 8 review agents complete and produces a severity-ranked list (critical, high, medium, low) with cross-referenced findings. For the Chirp Android QA, the synthesis report found 9 critical + 10 high issues. [^f3d8d-43]

Fix Dispatch

After the synthesis report, fix agents are fanned out ordered by severity. Critical issues get fixed first. The orchestrator reads the full synthesized report before dispatching any fixes to ensure fixes don't conflict. [^f3d8d-44]

## See Also
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide

