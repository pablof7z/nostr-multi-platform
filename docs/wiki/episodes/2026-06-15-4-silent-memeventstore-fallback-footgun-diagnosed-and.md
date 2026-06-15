---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - store-init-lmdb-gate
  - storage-path-footgun
  - mem-event-store-silent-fallback
supersedes: []
related_claims: []
source_lines:
  - 4094-4227
captured_at: 2026-06-15T17:37:54Z
---

# Episode: Silent MemEventStore fallback footgun diagnosed and hardened

## Prior State

When a storage_path is supplied but the lmdb-backend feature is not compiled in, store_init.rs silently falls back to MemEventStore and ignores the path — no warning, no error. Cold restart silently loses the entire event store.

## Trigger

Stress harness scenario A7.1 (cold-restart rebuild from LMDB) initially FAILed because the harness depended on nmp-ffi directly (bypassing the Chirp app crate's default features), so nmp-core's lmdb-backend cfg was off → storage_path silently ignored → MemEventStore used → data lost on restart.

## Decision

Add fail-loud diagnostic via V-67 channel when storage_path is supplied but lmdb-backend is not compiled in. Production is safe (Chirp's default features include nmp-core/lmdb-backend), but the footgun is real for any consumer that sets storage_path without explicitly enabling the feature.

## Consequences

- No more silent misconfiguration — a storage_path without lmdb-backend now surfaces a diagnostic instead of silently losing data
- Production Chirp builds confirmed safe: default = ["wallet", "lmdb-backend"] → nmp-core/lmdb-backend
- Diagnostic uses existing V-67 store_open_failure channel — no new stderr/log infrastructure needed

## Open Tail

*(none)*

## Evidence

- transcript lines 4094-4227
