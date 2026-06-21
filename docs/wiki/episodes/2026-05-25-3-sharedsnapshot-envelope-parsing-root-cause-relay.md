---
type: episode-card
date: 2026-05-25
session: 86221d39-67d3-484d-8979-b91cf75a5a72
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/86221d39-67d3-484d-8979-b91cf75a5a72.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-tui-snapshot
  - relay-diagnostics
  - envelope-parsing
supersedes: []
related_claims: []
source_lines:
  - 4382-4403
  - 5665-5670
captured_at: 2026-06-18T05:26:10Z
---

# Episode: SharedSnapshot envelope parsing root cause — relay diagnostics always empty

## Prior State

SharedSnapshot::from_json_value only handled the {t: snapshot, v: {…}} outer envelope in one code path. When the actual envelope format was {t:snapshot,v:...}, the unwrap failed silently, causing 'no relay diagnostics yet' on every TUI tick.

## Trigger

Agent investigation of the chirp-tui relay diagnostics panel found that the SharedSnapshot struct could not parse the enveloped format, producing empty diagnostics on every render cycle.

## Decision

Fixed SharedSnapshot to handle {t:snapshot,v:...} envelope format properly, so relay status data flows through to the diagnostics panel.

## Consequences

- chirp-tui now renders actual relay connection diagnostics instead of a perpetual empty placeholder
- Future envelope types must account for the wrapped {t:..., v:...} format, not just bare payloads

## Open Tail

*(none)*

## Evidence

- transcript lines 4382-4403
- transcript lines 5665-5670

