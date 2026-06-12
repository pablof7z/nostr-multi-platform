---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: root-cause
status: active
subjects:
  - nmp-ffi
  - ffi-surface
  - technical-debt
supersedes: []
related_claims: []
source_lines:
  - 1234-1378
captured_at: 2026-06-12T06:21:33Z
---

# Episode: FFI surface debt corrected from '48 bespoke symbols' to three specific items

## Prior State

A prior Opus review (2026-05-23) characterized the FFI surface as '48 bespoke symbols' of structural debt — one hand-rolled C symbol per app feature.

## Trigger

User questioned whether the debt still exists; investigation of current nmp-ffi showed the old pattern (bespoke per-feature symbols like publish_note) has been dismantled. The 55 current symbols are overwhelmingly generic framework surface (lifecycle, data plane, observer registration).

## Decision

The debt class described in the review is retired. What remains is three specific items: (1) nmp_app_open_timeline still hardcodes kinds {1,6} — tracked as #911/M2 migration, (2) raw-event tap is a documented and policed escape hatch, (3) add_relay/remove_relay send ActorCommand directly and are unadjudicated.

## Consequences

- The '48 bespoke symbols' characterization must not be cited as current state
- Only open_timeline (#911) is genuine debt needing migration
- add_relay/remove_relay deserve explicit adjudication on whether they belong in dispatch_action or stay as config-plane symbols

## Open Tail

- add_relay/remove_relay adjudication not yet recorded

## Evidence

- transcript lines 1234-1378

