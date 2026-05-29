---
title: D1 Doctrine — First Snapshot Must Precede Relay I/O
slug: d1-snapshot-before-relay-io
summary: D1 requires the kernel to emit the first snapshot before dialing any relays; the Reset arm had this reversed and was fixed in PR #763.
tags:
  - d1
  - doctrine
  - snapshot
  - relay
  - actor
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# D1 Doctrine — First Snapshot Must Precede Relay I/O

> D1 requires the kernel to emit the first snapshot before dialing any relays; the Reset arm had this reversed and was fixed in PR #763.

## The Problem

Profile mentions embedded in note content (`nostr:npub1...` / `nostr:nprofile1...` in the text field) are correctly tokenized for rendering by `crates/nmp-content/src/tokenizer.rs`. However, the ingest path (`crates/nmp-core/src/kernel/ingest/timeline.rs`) only calls `collect_unknown_refs(&event.tags)`, visiting tags but never content. So content-only mentions never trigger relay discovery REQs. [^42908-29]

## D1 Doctrine: Snapshot Before Relay I/O

D1 requires that the first rendered frame is independent of relay I/O — the kernel must emit the initial snapshot before dialing any relays. The `Start` arm of `dispatch_command` already had this correct ordering. The `Reset` arm had `spawn_missing_relays` before `emit_now` (a violation), which was fixed in PR #763 by moving `emit_now` before `spawn_missing_relays` in the `Reset` arm of `crates/nmp-core/src/actor/dispatch.rs`. [^42908-30]

## See Also
- [[kernel-boot-initial-emit-guarantee|Kernel Boot Initial Emit — Guaranteed Post-Start Snapshot Frame]] — related guide
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide

