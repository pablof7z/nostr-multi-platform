---
type: episode-card
date: 2026-05-21
session: 4f37753c-0654-4478-9c19-e799f1b10d39
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/4f37753c-0654-4478-9c19-e799f1b10d39.jsonl
salience: architecture
status: active
subjects:
  - chirp-tui-event-loop
  - nmp-core-update-callback
supersedes: []
related_claims: []
source_lines:
  - 652-767
  - 849-856
captured_at: 2026-06-18T05:00:59Z
---

# Episode: Push-update replaces REPL polling for TUI data flow

## Prior State

chirp-repl polls snapshots on demand via chirp_snapshot() — a synchronous pull model with no push notification

## Trigger

Research agent discovered nmp_app_set_update_callback() and nmp_app_register_event_observer() in nmp-core FFI, providing push-based snapshot delivery at emit_hz rate with kernel.changed_since_emit() gating (doctrine D8)

## Decision

TUI registers update callback before nmp_app_start() → callback sends to bounded mpsc channel → ratatui event loop receives Custom events, no polling or sleep() loops

## Consequences

- TUI must never call chirp_snapshot() in a poll loop; data arrives via callback wake
- Callbacks must be non-blocking (doctrine D8) — send to mpsc, do not render in callback
- Per-ingested-event observer available for fine-grained reactions (e.g. new DM notification)
- emit_hz=4 (250ms) from chirp-repl becomes the snapshot delivery cadence; TUI render tick is decoupled at 30 FPS

## Open Tail

- Whether snapshot versioning enables delta-only redraws or full-buffer comparison is needed
- Whether Marmot MLS group messages fire the event observer or only snapshot projections

## Evidence

- transcript lines 652-767
- transcript lines 849-856

