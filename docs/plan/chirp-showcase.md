# Chirp Showcase Goal

> Part of the [Build & Validation Plan](../plan.md).

Chirp is NMP's reference Nostr client and full-stack showcase. The M1
Twitter-like timeline was the first social baseline, not the product ceiling.

The goal is a fully featured client that demonstrates every reusable feature
NMP ships. A feature is not product-ready just because a crate exists; it must
either surface in Chirp through a real user workflow, or document why that
capability cannot or should not appear on iOS.

## Product Rule

When NMP ships a reusable capability, Chirp should show it. That includes
protocol modules, identity and signer paths, routing behavior, diagnostics,
capabilities, persistence, recovery, failure states, and performance proof.

This does not make Chirp a dumping ground for app logic. Feature behavior stays
in Rust modules under `crates/` when it is reusable. Native code renders Rust
snapshots and executes platform capabilities. The app FFI crate wires the
surface together; it does not decide policy.

## Coverage Areas

| Area | Chirp should demonstrate |
|---|---|
| Social core | Timeline, profile, threads, compose, replies, reactions, follows, lists, multi-account, deletes, and expiration handling. |
| Network and sync | Relay management, NIP-42 auth, NIP-65 outbox routing, NIP-77 negentropy, reconnect/replay, provenance, and per-relay diagnostics. |
| Identity and signers | Generated keys, nsec import, NIP-49 storage, NIP-46 bunker, platform signers where available, account switching, and signer failure states. |
| Content and media | Rich content rendering, long-form notes, comments, media previews, Blossom upload/download, attachment progress, and recovery. |
| Communities and private flows | NIP-29 groups, encrypted groups, DMs, gift-wrap primitives, membership state, and private-relay safety when those modules ship. |
| Wallet and value flows | NWC, zaps, zap receipts, balances, Cashu, nutzaps, and clear failure/retry states as those modules ship. |
| Trust and safety | Web-of-Trust scoring, moderation controls, relay/source provenance, and explicit opt-outs. |
| Developer proof | Diagnostics, smoke scenarios, UI tests, perf gates, error-shape coverage, and screenshots for every user-visible feature. |

## Acceptance Rule

A reusable NMP feature is considered shipped when all of these are true:

- The reusable Rust owner exists in `crates/` or another documented NMP module.
- Chirp exposes the feature in a real workflow, or the docs record a specific
  platform exception.
- Diagnostics or status surfaces make the feature debuggable.
- Automated coverage proves the happy path and at least one meaningful failure
  path through the real FFI surface.
- The relevant docs point from the feature back to Chirp's user-visible proof.

Milestone deferrals only affect timing. Once a feature ships in NMP, Chirp is
expected to become the place where that feature is visible, testable, and
understandable as part of a complete client.
