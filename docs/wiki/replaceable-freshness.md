---
title: Replaceable Event Freshness and TTL
slug: replaceable-freshness
topic: replaceable-freshness
summary: "Replaceable events use a TTL-based freshness system: each replaceable identity tracks `check_again_after`, and claims against stale entries automatically enqueu"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-03
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
---

# Replaceable Event Freshness and TTL

## Replaceable Freshness

Replaceable events use a TTL-based freshness system: each replaceable identity tracks `check_again_after`, and claims against stale entries automatically enqueue a background re-verification REQ. Default TTLs are kind:0 = 1 hour, everything else = 6 hours. The `check_again_after` timestamp is updated to `now + per-kind TTL` on EOSE receipt and on any event ingestion (new, replaced, or duplicate), preventing re-verify loops. <!-- [^37035-18] -->

## Force-Refresh

Force-refresh is implemented as a `force` parameter on `nmp_app_claim_profile` and `nmp_app_claim_event` rather than a separate `nmp_app_refresh_replaceable` C-ABI symbol. The `force` parameter applies to both `claim_profile` and `claim_event` — not just kind:0 — so any replaceable event type can be force-refreshed. When `force=true` (1=always hit relays, 0=use cache/TTL), the kernel zeros `check_again_after` before the TTL gate check, causing an immediate re-fetch regardless of staleness. The old `nmp_app_refresh_replaceable` stub is deleted. <!-- [^37035-19] -->

<!-- citations: [^37035-19] [^b4fe9-6] -->

## Correct Replaceable Ranges

NIP-01 classifies parameterized replaceable events as kind 30000..39999 only. The 20000..30000 range is ephemeral (never stored). The `is_parameterized_replaceable` function in `nmp-nostr-lmdb` previously classified the 20000..30000 range as parameterized replaceable, and a separate `is_parameterized_replaceable` in `nmp-core/src/kinds.rs` had the same wrong range, locked in by green tests asserting the bad model. PR #899 corrects this by delegating to `rust-nostr`'s `Kind::is_replaceable`/`is_addressable` (single source of truth) rather than hand-rolling corrected ranges. PR #899 should be merged and #915 closed as superseded. <!-- [^b4fe9-7] -->
