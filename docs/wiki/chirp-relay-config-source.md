---
title: Chirp Relay Config Source — nmp-chirp-config Not Hardcoded URLs
slug: chirp-relay-config-source
summary: Desktop and iOS must consume relay defaults from `nmp-chirp-config` rather than hardcoding `primal.net` URLs.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:ec0e64f8-3ef7-4983-933a-f5a3e672998a
---

# Chirp Relay Config Source — nmp-chirp-config Not Hardcoded URLs

## Relay Configuration Source

Desktop and iOS must consume relay defaults from `nmp-chirp-config` rather than hardcoding `primal.net` URLs. The hardcoded default relay URLs (`RELAY_BOOTSTRAP_DEFAULTS`, `BootstrapRelayEntry`, `default_relay_bootstrap()`) must be removed from nmp-core production code and moved to nmp-app-template; they should only remain in `cfg(test)` fallback constants.

<!-- citations: [^f3d8d-10] [^ec0e6-1] [^ec0e6-6] -->
