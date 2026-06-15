---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: product
status: active
subjects:
  - lmdb-backend
  - store-init
  - fail-loud-misconfiguration
supersedes:
  - 2026-06-15-4-store-init-fail-loud-when-storage
  - 2026-06-15-4-silent-memeventstore-fallback-footgun-diagnosed-and
related_claims: []
source_lines:
  - 4094-4248
captured_at: 2026-06-15T18:08:06Z
---

# Episode: LMDB silent fallback eliminated: fail-loud when storage_path set without lmdb-backend

## Prior State

store_init.rs silently fell back to MemEventStore when storage_path was supplied but the lmdb-backend cfg feature was off. No warning, no error — cold-restart would silently lose the entire event store.

## Trigger

Stress harness discovered the footgun: A7.1 cold-restart test initially FAILed because the harness depended on nmp-ffi directly (bypassing the Chirp app crate's default features), causing silent MemEventStore fallback. Production (Chirp) was safe (default includes lmdb-backend), but the silent fallback was a real misconfiguration trap.

## Decision

Emit a V-67 diagnostic (storage_path_non_persistent) when storage_path is set but lmdb-backend is off, reusing the existing diagnostic channel. No silent misconfiguration.

## Consequences

- Any consumer supplying storage_path without lmdb-backend will see an explicit diagnostic instead of silent data loss
- Production Chirp builds confirmed unaffected (default = ["wallet", "lmdb-backend"])

## Open Tail

*(none)*

## Evidence

- transcript lines 4094-4248
