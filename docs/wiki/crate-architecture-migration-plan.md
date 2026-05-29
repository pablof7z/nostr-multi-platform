---
title: 12-Step Crate-Boundary Architecture Migration
slug: crate-architecture-migration-plan
summary: nmp-core must be a pure substrate with zero protocol knowledge; protocol code lives in nmp-nipXX crates that depend on nmp-core, never the reverse.
tags:
  - architecture
  - crate-boundaries
  - nmp-core
  - d0
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# 12-Step Crate-Boundary Architecture Migration

> nmp-core must be a pure substrate with zero protocol knowledge; protocol code lives in nmp-nipXX crates that depend on nmp-core, never the reverse.

## Goal

Make `nmp-core` a pure substrate — generic actor, store, planner — with zero protocol-specific knowledge (D0 doctrine). Protocol code moves into `nmp-nipXX` crates that depend on `nmp-core`, never the reverse. [^42908-1]

## Completed Steps

The following migration steps are confirmed complete and merged to master:

- Substrate seams injected: `OutboxRouter`, `MailboxCache`, `IngestParser`, `ProtocolCommand`
- `nmp-router` extracted
- Kernel cut-over to injected router
- DM stack → `nmp-nip17`
- LNURL → `nmp-nip57`
- Relay worker + Pool API → `nmp-network`
- NIP-42 wire/FSM split
- `nmp-store` / `nmp-planner` extracted
- `nmp-app-template` created
- `nmp-ffi` extracted
- `chirp-*` moved to `apps/`
- Step 8 Phase B (Pool API): `crates/nmp-network/src/pool/` exists with full implementation (mod.rs, inner.rs, types.rs, tests.rs); `Pool`, `PoolConfig`, `PoolEvent`, `RelayHandle`, `PoolSnapshot` are public exports
- Step 8 Phase C (BrowserRelayDriver move): `crates/nmp-network/src/browser_driver.rs` exists (398 lines)
- Step 8 Phase D (nmp-signer-broker Pool dedupe): `crates/nmp-signer-broker` declares `nmp-network` dependency; direct `tungstenite`/`mio`/`rustls` deps removed; transport now rides Pool
- Step 8 Phase F (actor-pool cutover): merged as PR #479
- Step 12 (`nmp-marmot` return to `crates/`): `crates/nmp-marmot/` is a workspace crate [^42908-2]

## Remaining Work

- **V-38 (Stage 2)**: NWC/wallet fully migrated to `crates/nmp-nip47/`; any remaining surface-area cleanup
- **V-68 (Stage 2)**: Two live author/thread sites in `nmp-core`/`nmp-planner` still hardcode kind:1/6 social policy; Stage 1 (planner constructor + inert trace) landed in PR #773. Stage 2 requires an FFI/ABI change and is tracked as a separate follow-up.
- **PD-033-A revalidation**: The substrate must demonstrate it can host a genuine second app (non-Chirp) to prove the framework thesis. Blocked on V-37 (generic snapshot seam) being resolved. [^42908-3]

## D0 Doctrine

D0 requires that `nmp-core` contains zero app-domain or protocol-specific nouns. Violations are tracked as HIGH-priority backlog items. The doctrine-lint tool (rules D0–D16) enforces code-pattern invariants via grep-based static analysis, run in CI. A dependency-direction lint (D17) to catch layer-violation edges in `Cargo.toml` does not yet exist. [^42908-4]

## See Also

