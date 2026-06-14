---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: active
subjects:
  - k3-coverage-ledger
  - presence-heuristic-removal
  - since-floor
supersedes:
  - 2026-06-14-1-presence-heuristic-replaced-by-per-filter
related_claims: []
source_lines:
  - 6578-6632
captured_at: 2026-06-14T17:27:33Z
---

# Episode: Coverage ledger replaces presence heuristic as sole floor source

## Prior State

The presence heuristic (checking whether a relay had been seen) was used alongside the coverage ledger for determining since-floor boundaries in REQ queries. Presence-based flooring could suppress H1 follow-after-stray-reply.

## Trigger

K3 keystone Stage E directive: delete the presence heuristic entirely, making the coverage ledger the sole since-floor source.

## Decision

Presence heuristic fully deleted (PR #1421). The AtomicBool default was flipped on (PR #1419). The coverage ledger is now the only floor mechanism — presence-is-not-coverage is structurally closed and the production behavior.

## Consequences

- Un-synced (filter_hash, relay) shapes will request full history (un-floored REQ / full-window negentropy) until the ledger records completed coverage, causing more initial relay traffic on cold shapes.
- H1 follow-after-stray-reply suppression is now fixed by design.
- Breaking release nmp-v0.7.0 cut; consumers must adopt the new floor semantics.

## Open Tail

*(none)*

## Evidence

- transcript lines 6578-6632

