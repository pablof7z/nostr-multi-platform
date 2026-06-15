---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: product
status: superseded
subjects:
  - nmp-core-store-init
  - lmdb-backend-fail-loud
supersedes: []
related_claims: []
source_lines:
  - 4096-4247
captured_at: 2026-06-15T17:44:41Z
---

# Episode: store_init fail-loud when storage_path set but lmdb-backend feature off

## Prior State

When storage_path was supplied but the lmdb-backend feature was not compiled in, store_init.rs silently fell back to MemEventStore and ignored the path — causing silent data loss on restart for misconfigured builds.

## Trigger

Stress harness A7.1 cold-restart scenario initially failed because the harness depended on nmp-ffi directly (bypassing the app crate's default features). Investigation confirmed production Chirp builds are safe (default = ['wallet', 'lmdb-backend']), but the silent fallback footgun remained for any consumer that sets storage_path without enabling lmdb-backend.

## Decision

Emit a loud diagnostic via the V-67 snapshot channel when storage_path is set but lmdb-backend is off, instead of silently falling back to MemEventStore.

## Consequences

- Misconfigured builds no longer silently lose data on restart — the diagnostic surfaces through the existing V-67 channel (no stderr, consistent with D6)
- Production Chirp builds unaffected (already enable lmdb-backend by default)
- The stress harness must explicitly add nmp-core/lmdb-backend to its dependencies for LMDB scenarios

## Open Tail

*(none)*

## Evidence

- transcript lines 4096-4247
