---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - ram-eviction
  - open-view-pins
  - gc-eviction
supersedes: []
related_claims: []
source_lines:
  - 3378-3399
  - 3446-3481
  - 3533-3548
captured_at: 2026-06-12T00:59:06Z
---

# Episode: Open-view RAM eviction would silently blank live threads

## Prior State

The initial pin-set design in #1096 pinned `events ← timeline ∪ event_claims.keys()`, assuming that open-thread/open-author views would populate `event_claims` for their focused entries

## Trigger

Opus reviewer traced the code and proved that `open_thread`/`open_author` set `thread_view.selected_thread` / `author_view.selected_author` (ViewInterest refcounts) but write nothing to `event_claims` — that map is only populated by the separate `claim_event` embed mechanism. Replies arriving via thread subs land in `self.events` unpinned, and once the map exceeds the 1000 HWM they are evicted → silent blank rows while the user is viewing the thread. Recovery is also broken: evicted ids stay in `requested_ids`/`requested_reply_targets` (dedup blocks re-fetch), and the read paths never fall back to LMDB.

## Decision

Add `open_view_pins()` that derives pin sets from live view state: thread view pins focused id + derived root + `referenced_event_ids(focused)` + all four hydration bookkeeping sets (closing the recovery-dedup hole) + every cached event matching the `thread_items()` membership predicate; author view pins every cached event whose author matches. Pins are computed once per GC pass before any eviction.

## Consequences

- Open threads and non-followed author views survive eviction — no more silent blank rows
- Pinning `requested_*` sets also fixes the broken-recovery hole (dedup-blocked ids are never evicted while view is open)
- Profile pins extended to open-view authors and thread-participant authors
- One O(events) scan per open view per 60s GC pass (sub-millisecond at 1000-entry HWM)
- The `thread_items()` membership predicate is copied (not shared) into ram_eviction.rs — future broadening of `thread_items` won't auto-propagate to pin derivation, flagged as follow-up

## Open Tail

- Extract shared membership predicate or fold into #957 when it retires the stack

## Evidence

- transcript lines 3378-3399
- transcript lines 3446-3481
- transcript lines 3533-3548

