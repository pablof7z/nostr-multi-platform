---
title: NMP Relay Settings View & Role Configuration
slug: nmp-relay-settings-view
summary: Relay settings is presented as its own dedicated view accessed from the Settings hub
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-31
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:87fd49fb-4869-4c40-9a6a-96545bd2313d
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
  - session:ec0e64f8-3ef7-4983-933a-f5a3e672998a
---

# NMP Relay Settings View & Role Configuration

## Relay Settings View

Relay settings is presented as its own dedicated view accessed from the Settings hub. The view has a + button to add new relays. The relay list row displays a separate colored badge for each assigned role capability. The Settings > Relays pane uses a three-tier fallback: `state.features.configured_relays` (primary), `state.relays` from diagnostics (fallback), and `relay_lines(state)` (final fallback). The Settings > Relays panel must use the real connection state from relay diagnostics (looked up by URL) when rendering relay status dots, not the role label string.

<!-- citations: [^87fd4-1] [^6e4c3-7] [^fbebb-2] [^ec0e6-3] -->
## Relay Roles

Relay roles are additive — a single relay can have any combination of read, write, indexer, and wallet roles simultaneously. Indexer and app/wallet are both configurable relay roles. The relay add/edit sheet presents four independent toggles for Read, Write, Indexer, and Wallet roles instead of a single-choice picker. The edit relay sheet pre-populates all role toggles to match the relay's existing role assignments. relay.primal.net is registered as both indexer, causing it to appear in both Content and Indexer relay roles. Apps specify their desired relays and roles in their own Rust code via a builder API (`NmpAppBuilder::with_relay()`). Apps must be able to change relay configuration at runtime and persist those changes.

<!-- citations: [^87fd4-2] [^855be-7] [^ec0e6-4] -->
## Role Data Model

The kernel must not contain UI-framed naming; `relay_edit_rows` and `RelayEditRow` are renamed to `configured_relays` and `AppRelay` respectively. The JSON projection key changes from `relay_edit_rows` to `configured_relays` across all layers (nmp-ffi, iOS Swift, web TS, chirp-tui, chirp-desktop, nmp-codegen, doctrine-lint). Rust normalize_roles() parses, deduplicates, and sorts role tokens, storing role as a space-separated capability list (e.g. "indexer read write"). Rust has_role() checks for role containment with backward compatibility for legacy "both" entries. Relay configuration persistence uses a JSON sidecar file (`{storage_dir}/.nmp-relay-config.json`) written at first start, updated on add/remove actions, and read at subsequent starts, without LMDB schema changes. The relay configuration refactor is implemented in three phases: PR 1 is a mechanical rename only; PR 2 adds the builder API, JSON sidecar persistence, and moves hardcoded defaults; PR 3 writes updated config back to the sidecar on add/remove relay dispatch.

<!-- citations: [^87fd4-3] [^ec0e6-5] -->
## See Also
- [[nmp-app-relay-configuration|NMP App Relay Configuration]] — related guide

