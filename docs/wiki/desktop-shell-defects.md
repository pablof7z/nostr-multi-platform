---
title: Desktop Shell Defects
slug: desktop-shell-defects
topic: shell-defects
summary: "The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never a"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Desktop Shell Defects

## Desktop Shell Defects

The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never acked causing unbounded growth, and keyring nsect un-zeroized. Additionally, the render-churn feedback loop (Posts remounting every frame because snapshot() always produces a new reference, causing a claim/release storm where profile names never stabilize) must be fixed with stable sub-memos, not by increasing test timeouts or adding retries. When collapsed, the NMP Inspector must render near-zero per-frame work — full decode happens lazily when the dock opens, not eagerly on every snapshot frame. The ack contract for action_stages is dead on 3 of 4 hosts (iOS, Android, Desktop), converting action_stages from a correctness mechanism into a per-tick serialization tax. Action feedback should collapse to one mechanism (action_lifecycle) with TTL-anchored retention and ack as early-dismiss only; the action_results drain and action_stages ack-mirror should be deleted. The bounded bunker decrypt queue admits up to MAX_IN_FLIGHT_DECRYPTS=8 concurrent decrypts, with decrypt_state (ok|limited|unavailable) and undecrypted_count surfaced in the projection, so over-bound envelopes are counted and surfaced rather than silently dropped. The chirp-tui freeze is caused by an ADR-0055 projection-rev oracle violation where the claimed_events projection's cache unit changes without its revision number advancing (StaleStamp), triggering a panic at kernel_oracle.rs:33 that crashes the app and leaves the terminal frozen at the last drawn state; the oracle assertion detecting this violation should not be compiled into the chirp-tui binary because it is a test-only assertion. Shell code must not keep all-kinds raw event caches or construct protocol filters; those belong in Rust projections or diagnostics. Shells must render typed projections only; no raw event store/history crosses FFI; no raw kind switches in shell for protocol meaning; Rust emits typed embed keys, repost flags, relay diagnostic facts, and action-stage state. Desktop must consume the same typed signer/action projections as mobile; action stages must be acked and bounded; secret handling must match the platform capability/keyring model; no duplicate rendering/parsing of kernel-owned content; NIP-55 restore must be silent when persisted signer state is valid. Higher-layer boundary fixes (desktop shell defects, TUI shell boundary leaks, NIP-55 signer lifecycle bugs, selected projection/action lifecycle fixes) can be done in parallel and land as independent PRs immediately without waiting for the lower-layer persistence/acceptance architectural refactor.

<!-- citations: [^02745-117] [^02745-129] [^bf035-164] [^2e544-368] [^2e544-431] [^418d5-1] [^019ec-44] -->
