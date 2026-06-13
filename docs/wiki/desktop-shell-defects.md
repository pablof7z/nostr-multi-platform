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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Desktop Shell Defects

## Desktop Shell Defects

The desktop shell had four shipped-but-inert bugs: per-frame double-render (app.rs:1054/1059), bunker handshake projections never decoded, action_stages never acked causing unbounded growth, and keyring nsec un-zeroized.

<!-- citations: [^02745-117] [^02745-129] -->
