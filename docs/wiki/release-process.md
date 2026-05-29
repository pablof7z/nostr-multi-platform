---
title: Release Process — Tag Pattern, Workflow, and First Release
slug: release-process
summary: Tags matching nmp-v* trigger release-readiness workflow (manifest check + cargo package dry-run, not a publish); nmp-v0.1.0 was the first coordinated baseline tagged 2026-05-29.
tags:
  - release
  - tagging
  - release-readiness
  - nmp-cli
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Release Process — Tag Pattern, Workflow, and First Release

> Tags matching nmp-v* trigger release-readiness workflow (manifest check + cargo package dry-run, not a publish); nmp-v0.1.0 was the first coordinated baseline tagged 2026-05-29.

## Release Tag Pattern

The release tag pattern is `nmp-v{version}` (defined in `release/nmp-release.toml`). The workspace version is at `root Cargo.toml:56`. Pushing a tag matching `nmp-v*` triggers the `.github/workflows/release-readiness.yml` workflow. [^42908-46]

## Release Readiness Workflow

The `release-readiness.yml` workflow runs on tag push. It performs:
- Manifest check (verifies `nmp-release.toml` consistency)
- `cargo package` dry-run (NOT a crates.io publish)

It does NOT publish to crates.io/npm unless that happens automatically via the workflow. Tagging + GitHub release is the deliverable. [^42908-47]

## Release CLI

The `nmp` CLI supports `nmp upgrade`, `nmp doctor`, and `nmp init --nmp-version <tag>`. App consumers can pin to a specific release baseline via `nmp init --nmp-version 0.1.0`. [^42908-48]

## nmp-v0.1.0 — First Release (2026-05-29)

The first coordinated release-train baseline. Tagged off the master tip after PR #779 (c13 fix). What it includes:
- OP-centric feed live (V-80)
- D5 snapshot bounding (V-46, PR #770)
- Silent-failure hardening: V-61/62 Marmot, V-67 LMDB diagnostic, V-69 LMDB orphan-index, V-70 hex Option, V-72 signer kind overflow, V-63/64 NIP-47 payments
- D0 substrate purity stage 1 (V-68, PR #773)
- Cleanups: V-71, V-74, V-77
- V-75 router lane attribution (PR #777)
- V-58 rate-limited backoff (PR #778) [^42908-49]

## See Also

