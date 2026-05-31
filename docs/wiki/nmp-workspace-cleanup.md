---
title: NMP Workspace Cleanup & Artifact Removals
slug: nmp-workspace-cleanup
summary: The `apps/longform` and `apps/notes` artifacts are deleted from the `Cargo.toml` workspace members.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:f8eb6e59-19f0-4591-a9b4-47453c051d45
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
---

# NMP Workspace Cleanup & Artifact Removals

## Workspace Members

The `apps/longform` and `apps/notes` artifacts are deleted from the `Cargo.toml` workspace members. The release manifest (`release/nmp-release.toml`) must not reference the deleted packages `apps/notes` and `apps/longform`. The workspace Cargo.toml members list must switch from explicit per-crate paths to globs (e.g. `apps/chirp/crates/*`) when Rust crates move into `apps/<app>/crates/`. Migrations to new paths must be driven to completion with the old path fully removed, not left half-landed as `#[deprecated]`. The `nmp_app_gallery_snapshot` pull chain (Rust symbol, Kotlin wrapper, Swift wrapper) is dead code with zero call sites and is safe to remove. Implemented milestone plans must be deleted from `docs/plan/` (specifically `docs/plan/m0–m10*` files). The `docs/perf/codex-reviews/` directory must be deleted entirely, and review, audit, and brainstorm files in `docs/perf/` must be deleted. Benchmark data in `docs/perf/` (firehose-bench, reactivity-bench, m10.5/S*, real-relay, pulse, screenshots, marmot, pending-user-decisions) must be kept.

<!-- citations: [^f8eb6-4] [^f5503-5] [^16ca6-8] [^d0690-8] [^44c6c-2] -->
## See Also

