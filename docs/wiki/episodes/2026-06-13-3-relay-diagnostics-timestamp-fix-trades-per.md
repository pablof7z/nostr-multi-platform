---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - relay-diagnostics
  - timestamp-churn
  - byte-stability
supersedes: []
related_claims: []
source_lines:
  - 6262-6301
captured_at: 2026-06-13T21:45:23Z
---

# Episode: Relay-diagnostics timestamp fix trades per-second churn for per-millisecond churn — requires deterministic wall-clock anchor

## Prior State

PR #1332 ships raw Unix-ms timestamps on the wire (formatting in shells), intended to eliminate §62 churn where pre-formatted relative-time strings flip every second. The implementation computes timestamps via two independent live clock reads (SystemTime::now() + Instant::now()) recomputed every snapshot.

## Trigger

Opus review (PR #1332) found: elapsed_to_unix_ms computes unix_now_ms - (now_ms - event_ms) from two non-simultaneous clock reads, causing ~1ms jitter per tick for a fixed event. The snapshot emission is gated by changed_since_emit (not byte-level), so this doesn't cause extra emissions today, but it undercuts the byte-stability goal — trading per-second churn for per-millisecond churn.

## Decision

Anchor wall-clock timestamp once at kernel start: store started_unix_ms alongside timing.started_at, compute deterministic unix_ms as started_unix_ms + event_ms for every event. Add a byte-stability oracle test asserting two consecutive relay_diagnostics_snapshot() calls (no intervening relay event) serialize to identical bytes. Also fixes: regenerate Kotlin bindings with pinned flatc 25.2.10, strip u64 suffixes from JSON test fixture, extract relay_settings format helpers under file-size baseline.

## Consequences

- Without the deterministic anchor, the PR's byte-stability goal is not met — two snapshots of the same event produce different bytes
- The byte-stability oracle test is the regression guard the whole PR needs; it would be flaky without the anchor fix
- With deterministic timestamps + per-projection rev-gating (ADR-0053), relay_diagnostics will never produce spurious Changed signals
- PR #1332 is the hard prerequisite for ADR-0053's incremental emission — relative-time strings would poison the rev gate

## Open Tail

- Sonnet agent dispatched with all six review findings; must pass CI and opus re-review before merge
- After #1332 lands, the S3 baseline measurement can be compared against incremental-emission Rung-0 instrumentation

## Evidence

- transcript lines 6262-6301

