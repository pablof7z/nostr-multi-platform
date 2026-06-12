---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: architecture
status: active
subjects:
  - open-timeline-retirement
  - contact-feed-ffi-seam
  - interest-scope-boundary
  - adr-0042-amendment
supersedes: []
related_claims: []
source_lines:
  - 1380-1382
  - 1754-1772
  - 2037-2056
captured_at: 2026-06-12T06:38:44Z
---

# Episode: Retire open_timeline via dedicated contact-feed verb, not open_interest scope overload

## Prior State

`nmp_app_open_timeline` hardcoded Chirp's `{1,6}` social kinds inside the generic `nmp-ffi` crate — the last remaining bespoke feed-open verb and a D0 violation tracked as #911 (V-68 Stage 2). No `close` counterpart existed. The tempting fix was to add a `scope=2` (FollowsOfActiveAccount) to the existing `open_interest` seam.

## Trigger

User directive to fix `nmp_app_open_timeline`. Design exploration revealed that `InterestScope` in the planner governs *mailbox/relay resolution*, not author-set expansion — the follow feed's authors are kernel-owned, kind:3-sourced, live-rebuilt per-follow, and re-routed on account switch. Forcing it through `open_interest` would make `filter_json`/`consumer_id`/`close` mean different things depending on a magic integer, violating ADR-0042 §5.1's anti-pattern.

## Decision

Adopt a dedicated generic pair: `nmp_app_open_contact_feed(kinds_json)` / `nmp_app_close_contact_feed()`. The kind set is the only app-policy input; authors/re-routing/refcount stay kernel-owned. The `{1,6}` literal lives once in a Chirp wrapper (`nmp_app_chirp_open_home_feed`), mirroring the author-feed precedent. Net +1 symbol (the close verb fixes a real gap — nothing could deactivate the contact feed before). ADR-0042 gets an amendment section (not a new ADR). Ships as one PR.

## Consequences

- ActorCommand::OpenContactListSubscription deleted; OpenContactFeed{kinds} + CloseContactFeed replace it
- D5 projection-cluster gating becomes symmetric: close re-hides the timeline cluster
- `timeline_requested` milestone flag unaffected (driven by ingest path, not the FFI verb)
- Closes #911 and #958: zero bespoke feed-open verbs remain in nmp-ffi
- Four shells (iOS, TUI, desktop, Android JNI) migrate to the Chirp wrapper
- ADR-0042 amended with the scope=2 refutation rationale

## Open Tail

- Android Kotlin caller rename decision (keep `nativeOpenTimeline` JNI name or change)

## Evidence

- transcript lines 1380-1382
- transcript lines 1754-1772
- transcript lines 2037-2056

