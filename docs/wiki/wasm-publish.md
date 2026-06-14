---
title: WASM Publish
slug: wasm-publish
topic: ci-gates
summary: Issue #1202 (wasm silent publish failure) is resolved by replacing the silent NoTargets with an honest CapabilityFailure (`publish_not_supported_in_web_preview_
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
---

# WASM Publish

## Silent Publish Failure

Issue #1202 (wasm silent publish failure) is resolved by replacing the silent NoTargets with an honest CapabilityFailure (`publish_not_supported_in_web_preview_reason`). The real composition root is deferred to #1007. PR #1325 (#1202 wasm honesty) implements this change.

The deployed Chirp Web app must build and serve the real current wasm per deploy (not a stale pre-built artifact). Chirp Web deploys to chirp-nmp.f7z.io on Vercel, with the real wasm built per deploy.

The TypeScript FlatBuffers bindings for the snapshot must be regenerated when the FlatBuffers schema changes, and a drift guard must exist to catch stale bindings (the web was blind to real data because bindings had drifted). <!-- [^bf035-183] -->

<!-- citations: [^02745-93] [^02745-133] [^bf035-182] -->
## Park Dead Islands

Issue #1250 (park dead islands) is resolved by excluding nmp-blossom and nmp-nip60 from the workspace and removing them from the release manifest. <!-- [^02745-134] -->


web-cli install registration for web components must be coordinated with the peer (who owns the registry) rather than added unilaterally — premature web/ installIds in content.ts broke the CLI manifest mirror test. <!-- [^bf035-184] -->
## Platform Contract

The v1 platform contract is iOS + Android + desktop only; web/wasm is post-v1. <!-- [^02745-135] -->

## WASM Runtime Execution

The wasm runtime had never actually executed in a browser before this effort — CI only cargo-checked wasm32, and the browser loaded a 3-week-old stale artifact. <!-- [^bf035-178] -->

std::time::Instant and SystemTime panic on wasm32-unknown-unknown; the fix is a web-time shim behind crates/nmp-core/src/time.rs, with native staying byte-identical. <!-- [^bf035-179] -->

Every PR must be reviewed by an independent agent before merging, with the review gate verifying wasm32 builds and real browser execution, not just native compilation. <!-- [^bf035-180] -->

Playwright end-to-end tests must serve genuinely signed events from a fixture relay and assert real rendered data (not stubs or mocks), running in CI on every push. <!-- [^bf035-181] -->
