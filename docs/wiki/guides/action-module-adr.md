---
title: Action Module ADR
slug: action-module-adr
topic: ffi-runtime
summary: "ADR-0027 prescribes a staged unified ActionModule trait: Stage 1 (trait), Stage 2Aâ2F (per-crate), Stage 3 (delete dual seam)"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:019edc3e-b4a1-72a0-b791-9dcfdd615785
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Action Module ADR

## Implementation Staging

ADR-0027 prescribes a staged unified ActionModule trait: Stage 1 (trait), Stage 2A–2F (per-crate), Stage 3 (delete dual seam). The ADR-0027 implementation must be staged rather than attempted as a single-PR refactor. ADR-0027 is written and merged (PR #196), but no implementation branch exists yet; the dual-action seam (register_action_module + register_action_executor) remains on master. The dormant PublishKeyPackageAction (B3) is deleted via PR #238. ADR-0029 documents the bounded channel + shed-load design (C4); the actual bounded-channel implementation code is deferred. ADR-0032 (raw-data projection doctrine) was created to document that backend sends raw data and presentation layers own all formatting decisions. ADR-0032 must be amended with a dated note recording the #1099 precompute regression, its #1580 removal, the miscitation correction, and the shared-shell-mapping rationale. Issues #1556, #1559, and #1561 deserve ADR-level treatment. Every external effect must be represented as typed data crossing the Rust/native boundary; new nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams, and reducers must remain replayable from message history.

<!-- citations: [^86221-1] [^47203-1] [^156aa-1] [^95d02-1] [^019ed-82] [^019ed-116] [^11850-249] -->
