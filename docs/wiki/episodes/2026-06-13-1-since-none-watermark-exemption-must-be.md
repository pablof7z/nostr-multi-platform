---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: superseded
subjects:
  - nmp-core-subs-watermark
  - since-none-backfill
  - tailing-vs-oneshot
supersedes:
  - 2026-06-13-1-since-none-watermark-rewrite-exemption-refined
related_claims: []
source_lines:
  - 8345-8349
  - 8806-8839
  - 8839-8860
captured_at: 2026-06-13T20:56:22Z
---

# Episode: since=None watermark exemption must be lifecycle-aware (backfill exempt, Tailing narrowed)

## Prior State

T129 watermark rewrite applied uniformly to all subscription shapes: any interest had its since floor raised to watermark+1 on reconnect/recompile, including since=None (unbounded) interests — causing full-history backfill requests to be narrowed to watermark+1 instead of fetching all events.

## Trigger

Owner decision (#1281): since=None interests should backfill full history, not be narrowed by watermark rewriting. Initial implementation exempted since=None uniformly, which regressed Tailing live feeds (negentropy_skips_redundant_req e2e test failed) — Tailing interests also default to since=None but must still narrow to skip already-cached events.

## Decision

Watermark rewrite exemption is now lifecycle-aware: non-Tailing (OneShot/backfill) interests with since=None skip watermark narrowing (fetch full history); Tailing interests with since=None still narrow to since=watermark+1. Applied in both recompile.rs apply_watermark_rewrite and handlers.rs handle_reconnect.

## Consequences

- Backfill/OneShot interests now correctly request full history from relays
- Tailing/live-feed interests continue to skip already-cached events via watermark narrowing
- Initial uniform exemption broke master (negentropy e2e regression), requiring the lifecycle-aware correction in PR #1337
- Two code paths in subs must now correctly distinguish Tailing vs non-Tailing: recompile and reconnect-replay

## Open Tail

- Chirp and Android consumers must handle the new lifecycle-aware since behavior

## Evidence

- transcript lines 8345-8349
- transcript lines 8806-8839
- transcript lines 8839-8860

