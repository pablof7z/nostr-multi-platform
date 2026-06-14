---
title: CLI Registry Manifest
slug: cli-registry-manifest
topic: nmp-app-integration
summary: The CLI registry manifest must mirror all component ids that appear in the web registry, including web-targeted components such as web/login-block, web/relay-li
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:019ec57a-fb01-7081-80c8-d7107f302049
---

# CLI Registry Manifest

## Component Coverage

The CLI registry manifest must mirror all component ids that appear in the web registry, including web-targeted components such as web/login-block, web/relay-list, and web/render-identity. <!-- [^019ec-5] -->

## Vendor Structure

The CLI registry vendor structure uses one file per platform target (e.g. registry.web.toml for web components), matching the existing split pattern used for other platforms. <!-- [^019ec-6] -->

## Aggregation Delegation

Registry content.ts and user.ts aggregate files must delegate their web metadata to dedicated platform modules (contentWeb.ts, userWeb.ts) rather than inlining web object literals. <!-- [^019ec-7] -->
