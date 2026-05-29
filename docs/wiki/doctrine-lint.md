---
title: Doctrine Lint Tool — D0–D16 Rules and Missing D17
slug: doctrine-lint
summary: doctrine-lint enforces D0–D16 code-pattern rules in CI; D17 (dependency-direction layer enforcement) does not yet exist and is needed to catch Cargo.toml layer violations.
tags:
  - doctrine
  - lint
  - ci
  - d0
  - architecture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Doctrine Lint Tool — D0–D16 Rules and Missing D17

> doctrine-lint enforces D0–D16 code-pattern rules in CI; D17 (dependency-direction layer enforcement) does not yet exist and is needed to catch Cargo.toml layer violations.

## Current State of Doctrine Lint

The doctrine-lint tool (at `crates/nmp-testing/bin/doctrine-lint/`) enforces rules D0–D16 via grep-based static analysis against code-pattern fixtures. All existing rules are pattern-based, targeting:
- **D0**: App nouns in substrate
- **D6**: Panic/unwrap outside tests
- **D7**: Policy-decision verbs in capability traits
- **D8**: Hot-path allocations and polling
- **D9–D16**: Protocol purity, DM security, snapshot keys, etc.

The smoke test runs 42 tests and is part of CI. [^42908-24]

## D17: Missing Dependency-Direction Lint (V-57-P6)

No tool currently enforces layer invariants in the dependency graph:
- `cargo deny` enforces license whitelist, advisory scanning, and known-safe registry only — it has zero crate-layer rules
- No `cargo-vet` or layer-lint config detects `nmp-router → nmp-ffi` or other layer-violation edges at build time

Example violation: `crates/nmp-nip29/Cargo.toml` imports both `nmp-core` and `nmp-ffi` (layers apart) with no lint catching it.

Required: a D17 rule that parses `Cargo.toml` dependency graphs and enforces layer invariants (nmp-app-* → nmp-protocol-* → nmp-core; only nmp-core may import nmp-ffi), with an explicit allowlist for sanctioned adapter cases. [^42908-25]

## See Also
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide

