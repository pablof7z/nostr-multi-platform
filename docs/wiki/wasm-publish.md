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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# WASM Publish

## Silent Publish Failure

Issue #1202 (wasm silent publish failure) is resolved by replacing the silent NoTargets with an honest CapabilityFailure (`publish_not_supported_in_web_preview_reason`). The real composition root is deferred to #1007. PR #1325 (#1202 wasm honesty) implements this change.

<!-- citations: [^02745-93] [^02745-133] -->

## Park Dead Islands

Issue #1250 (park dead islands) is resolved by excluding nmp-blossom and nmp-nip60 from the workspace and removing them from the release manifest. <!-- [^02745-134] -->

## Platform Contract

The v1 platform contract is iOS + Android + desktop only; web/wasm is post-v1. <!-- [^02745-135] -->
