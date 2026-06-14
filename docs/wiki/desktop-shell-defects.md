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
---

# Desktop Shell Defects

## Desktop Shell Defects

The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never acked causing unbounded growth, and keyring nsec un-zeroized. Additionally, the render-churn feedback loop (Posts remounting every frame because snapshot() always produces a new reference, causing a claim/release storm where profile names never stabilize) must be fixed with stable sub-memos, not by increasing test timeouts or adding retries. When collapsed, the NMP Inspector must render near-zero per-frame work — full decode happens lazily when the dock opens, not eagerly on every snapshot frame.

<!-- citations: [^02745-117] [^02745-129] [^bf035-164] -->
