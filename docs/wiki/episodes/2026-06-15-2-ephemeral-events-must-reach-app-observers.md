---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - ephemeral-delivery
  - notify-event-observers
  - kernel-event-observer
supersedes: []
related_claims: []
source_lines:
  - 1428-1493
captured_at: 2026-06-15T10:51:19Z
---

# Episode: Ephemeral events must reach app observers (latent bug discovered)

## Prior State

Ephemeral events (kind 20000-29999) reach NIP parsers (via `verify_and_persist` dispatch on `Inserted|Replaced|Ephemeral`) but do NOT reach app-facing `KernelEventObserver` because `notify_event_observers` fires only on `Inserted|Replaced`, excluding `Ephemeral`. NWC happens to work because it rides the parser/capability path, not the generic observer seam.

## Trigger

User correction during admission-model discussion: 'non-ephemeral is just for the cache part, but applications must (obviously) be able to use ephemeral events' — which revealed that the current observer gate excludes ephemerals from app delivery, not just from persistence.

## Decision

Dispatch and notify every valid event including ephemeral. The observer gate becomes `Inserted|Replaced|Ephemeral`. 'Ephemeral' qualifies only the persist step (don't durably store), not delivery (always show the app). The invariant: deliver everything valid to parsers and app observers; durably store only non-ephemeral.

## Consequences

- Latent bug fix added to PR 1 step 1: `notify_event_observers` gate widened to include `Ephemeral`
- The admission/persistence/delivery split is now explicit everywhere: admission = valid sig, persist = non-ephemeral only, deliver = all valid including ephemeral
- §7 verification oracle updated: ephemeral events must reach NIP parsers AND app observers but must NOT persist in the store

## Open Tail

- Other app-facing seams that might similarly exclude ephemerals should be audited — the observer seam is the one found, but there may be others

## Evidence

- transcript lines 1428-1493
