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
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Desktop Shell Defects

## Desktop Shell Defects

The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never acked causing unbounded growth, and keyring nsect un-zeroized. Additionally, the render-churn feedback loop (Posts remounting every frame because snapshot() always produces a new reference, causing a claim/release storm where profile names never stabilize) must be fixed with stable sub-memos, not by increasing test timeouts or adding retries. When collapsed, the NMP Inspector must render near-zero per-frame work — full decode happens lazily when the dock opens, not eagerly on every snapshot frame. The ack contract for action_stages is dead on 3 of 4 hosts (iOS, Android, Desktop), converting action_stages from a correctness mechanism into a per-tick serialization tax. Action feedback should collapse to one mechanism (action_lifecycle) with TTL-anchored retention and ack as early-dismiss only; the action_results drain and action_stages ack-mirror should be deleted. The bounded bunker decrypt queue admits up to MAX_IN_FLIGHT_DECRYPTS=8 concurrent decrypts, with decrypt_state (ok|limited|unavailable) and undecrypted_count surfaced in the projection, so over-bound envelopes are counted and surfaced rather than silently dropped.

<!-- citations: [^02745-117] [^02745-129] [^bf035-164] [^2e544-368] [^2e544-431] -->
