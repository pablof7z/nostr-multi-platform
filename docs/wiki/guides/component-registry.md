---
title: Component Registry Unification
slug: component-registry
topic: component-registry
summary: "The project maintains two component registries that have historically existed as separate artifacts: the gallery showcase catalog (`apps/nmp-gallery/registry.js"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# Component Registry Unification

## Unified Component Registry

The project maintains two component registries that have historically existed as separate artifacts: the gallery showcase catalog (`apps/nmp-gallery/registry.json`) and the CLI install registry (`crates/nmp-cli/registry/*.toml`). The gallery showcase catalog is the SSOT consumed by all four platform hosts — TUI via `registry().sections`, iOS via C-ABI JSON, Android via JNI `registryJson()` — and was shipped in commit #2258. The CLI install registry (`BUILTIN_REGISTRY_SECTIONS` in `nmp-cli`, registry_id = `nmp-local`, split per-platform for the 500-LOC gate) is a separate registry containing per-platform install manifests for `nmp add`, not a duplicate of the gallery showcase catalog. Because these two registries share overlapping concepts but can drift independently, they are to be unified into one canonical source of truth for component definitions. The two registries use different identity schemes — the showcase catalog uses `user-avatar` while the CLI registry uses `swiftui/user-avatar` — with nothing enforcing they refer to the same component. The registries have drifted: five showcase components (`content-quote-card`, `embed-article`, `embed-highlight`, `embed-note`, `embed-profile`) exist in the gallery showcase catalog but are not installable via `nmp add`; eight installable components are not showcased; and there is no enforced shared identity between the showcase `user-avatar` and CLI `swiftui/user-avatar`. The gallery showcase catalog and CLI install registry must be unified to a single canonical source with a CI drift-gate so showcase and install can never silently diverge again.

<!-- citations: [^3c942-6065d] [^3c942-c115b] [^3c942-2a799] -->
