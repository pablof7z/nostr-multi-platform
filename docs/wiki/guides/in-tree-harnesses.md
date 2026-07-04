---
title: In-Tree Conformance Harnesses and Extracted Apps
slug: in-tree-harnesses
topic: project-status
summary: Gallery stays in-tree as a cross-platform conformance and regression harness â a storybook proving every NMP component decodes and renders on every platform a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# In-Tree Conformance Harnesses and Extracted Apps

## In-Tree Harnesses

Gallery stays in-tree as a cross-platform conformance and regression harness — a storybook proving every NMP component decodes and renders on every platform at HEAD — not a shippable product. Extraction to an external pinned repo was explicitly rejected; Gallery must remain in-tree permanently and must not be extracted before #2256 (the clean-room DX gate). Chirp is fully extracted to a standalone repo at `github.com/pablof7z/chirp`; in-tree NMP app code is deleted. The governing rule: products live outside the framework repo, conformance harnesses live in-tree.

All in-repo shells (SwiftUI, Compose, desktop, TUI, web) render AdCandidateUrl as a plain-link baseline — no regression from the wire variant change.

The nutsack PoC repo is a consumer test harness that drives exactly what the NMP wallet integration needs to expose, rather than autonomously building the internal wallet product. It is scaffolded at `/Users/pablofernandez/Work/nutsack` as a cargo workspace: `nutsack-core` for app-core with typed action builders and projection mirror, `nutsack-tui` for a ratatui shell skeleton, and `tests/` for a headless acceptance harness. It builds green with the `nmp-backend` feature OFF until NMP is consumable.

The nutsack `SPEC.md` covers product scope + non-goals, 6 user flows, a TUI screen map, the thin-shell architecture mapped 1:1 to the design doc's Product Surface (8 MVP actions + bounded wallet projection), the testnut default config, the relay-acquisition boundary (NMP owns the relay set via kind:10019/NIP-65, not app-provided), security invariants, and the NMP gap list.

The nutsack acceptance test is a headless two-party nutzap round-trip: two fresh nsecs → each `cashu.create` + `nutzap.publish_info` → each deposit value-less ecash from testnut.cashu.space (deposit_quote → complete_deposit, auto-settle, no Lightning) → A nutzaps B, B nutzaps A → each redeems → assert both balances/history via the projection only, with an ephemeral relay via `nak serve`.

<!-- citations: [^898a4-ca491] [^3c942-c551d] [^91a86-d8b97] [^fb992-5d7bf] -->
