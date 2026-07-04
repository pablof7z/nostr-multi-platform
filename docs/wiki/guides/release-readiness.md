---
title: Release Readiness Pipeline
slug: release-readiness
topic: ci-gates
summary: The `release-readiness.yml` workflow is the exit criterion for the release pipeline
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:04745411-a0c1-4523-ac83-71dc983f410b
---

# Release Readiness Pipeline

## Release Readiness Workflow

The `release-readiness.yml` workflow is the exit criterion for the release pipeline. It validates release gates on both PR events and tag pushes. The tag-triggered release rehearsal (`push: tags: nmp-v*`) is an exit criterion that must fire and pass before a real publish; pushing the `nmp-v1.0.0-rc.1` tag kicks off a rehearsal run that validates the full release-readiness path. The workflow contains zero actual publish steps — it only runs gates, making it safe to rehearse against. A migration-note file is hard-required to exist for the tagged version before a tag-triggered rehearsal can pass.

The project uses rc releases for the crate/npm publish, starting with crates.io. The NMP release workspace version for the rc is `1.0.0-rc.1`.

The release manifest specifies `npm_publish_mode = "trusted-publishing"` (OIDC) for CI-based publishing without long-lived tokens.

<!-- citations: [^04745-3b5dc] [^04745-71abc] [^04745-cb2cd] -->

## Internal Dependency Pinning

All internal `nmp-*` path dependencies in `[dependencies]` and `[build-dependencies]` sections carry a `version =` constraint so crates.io can resolve them. Unversioned `dev-dependencies` with a bare `path` are left untouched by the version-pinning script because cargo auto-strips them during packaging. `cargo publish --dry-run` validates against the live crates.io index, so any crate with an internal dependency only passes once that dependency is actually live on the registry. <!-- [^04745-997af] -->
