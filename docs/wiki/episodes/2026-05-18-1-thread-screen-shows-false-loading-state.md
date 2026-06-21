---
type: episode-card
date: 2026-05-18
session: cc7dc68a-1fcd-49fe-98be-198f17b6d59e
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cc7dc68a-1fcd-49fe-98be-198f17b6d59e.jsonl
salience: root-cause
status: active
subjects:
  - chirp-thread-screen
  - chirp-route
  - nmp-kernel-snapshot-gap
supersedes: []
related_claims: []
source_lines:
  - 127-664
captured_at: 2026-06-18T04:17:24Z
---

# Episode: Thread screen shows false loading state — route discards cached item

## Prior State

ThreadScreen received only eventID via ChirpRoute.thread(eventID:). On navigation, threadView was nil until the next kernel snapshot tick (up to 250ms at 4Hz), so the UI showed a 'Fetching events from relays' placeholder even though the tapped event was already in the local kernel cache.

## Trigger

User reported seeing 'fetching events from relays' after tapping an already-downloaded event. Investigation revealed two issues: (1) the async snapshot gap — threadView is nil for up to one emit tick after openThread, and the route threw away the TimelineItem already in scope; (2) the kernel fires thread hydration REQs unconditionally (no cache-hit guard) even for events already in self.events.

## Decision

Embed TimelineItem as an optional initialItem parameter in ChirpRoute.thread. ThreadScreen renders the focused note immediately from initialItem when threadView is nil, replacing the false 'fetching' placeholder. All call sites that have a TimelineItem in scope pass it; SearchView and 'Show this thread' pill pass nil (no item available).

## Consequences

- The tapped note is visible instantly on thread open; only replies/context below it wait for the kernel snapshot.
- Kernel thread hydration REQs are unchanged — they still fire unconditionally for root + replies (correct for replies which need fresh data, overhead for the already-cached focused event).
- ThreadScreen has two fallback paths: initialItem → immediate render → snapshot overlay; nil → old spinner behavior (SearchView).
- The unconditional REQ behavior for already-cached thread events is now an explicitly noted optimization gap.

## Open Tail

- Kernel enqueue_thread_id could skip IDs already in self.events to avoid re-fetching the focused event and root — not yet implemented.
- M2 subscription-compiler migration (compiler.md §3.5) will replace the hand-maintained thread REQ builders with view-module interest registration.

## Evidence

- transcript lines 127-664

