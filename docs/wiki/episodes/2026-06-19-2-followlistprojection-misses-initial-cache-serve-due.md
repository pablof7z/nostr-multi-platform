---
type: episode-card
date: 2026-06-19
session: e6b44a84-8cfc-48b2-863a-58382398b5df
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e6b44a84-8cfc-48b2-863a-58382398b5df.jsonl
salience: root-cause
status: active
subjects:
  - follow-list-projection
  - store-first-interest-registration
  - observer-ordering
supersedes:
  - 2026-06-19-1-follow-list-stale-ui-and-broken
related_claims: []
source_lines:
  - 904-904
  - 1131-1132
  - 1352-1371
captured_at: 2026-06-19T12:38:13Z
---

# Episode: FollowListProjection misses initial cache-serve due to registration ordering

## Prior State

FollowListProjection relied on `startup.rs`'s combined kind:0+3+10002 interest for kind:3 delivery, with an explicit comment: 'no separate interest push is needed — events arrive through the standing subscription.' The observer was assumed to receive events through the standing subscription alone.

## Trigger

User testing revealed two UI bugs: (1) already-followed profiles display 'Follow' instead of 'Following', and (2) tapping Follow produces haptic feedback but the button stays 'Follow'. User explicitly rejected explicit cache-warming as a hack, citing the previously established store-first interest-registration doctrine (ADR-0045).

## Decision

The comment 'no separate interest push is needed' is an architectural mistake — the same bug class as the profile-claim interest gap documented in `store-first-layering-investigation.md`. FollowListProjection must own its own interest registration: after registering the observer, push an `EnsureInterest`/`PushInterest` for `{authors: [active_pubkey], kinds: [3]}` through the `ensure_interest_and_serve`/`push_interest_and_serve` front-door. This triggers cache-serve with the observer already wired, so the initial kind:3 state hydrates the projection correctly. The sequence bug: `nmp_app_start` → cache-serve runs → no observer present → later `FollowListStore.init` → observer registered but too late.

## Consequences

- Each projection that consumes kernel data must own its own interest rather than relying on a coarser combined interest from startup
- Follow/unfollow actions DO correctly update via read-your-writes (the `local_kind3_publish_fans_out_to_event_observers` test proves this) — only the cold-start state is wrong
- The `feed_served_event` path DOES call `notify_event_observers`, refuting the earlier (incorrect) episode card claim that KernelEventObserver is 'live-only'

## Open Tail

- Fix not yet implemented — user requested discussion before code changes; assistant proposed the PushInterest-after-observer-registration approach and awaits confirmation

## Evidence

- transcript lines 904-904
- transcript lines 1131-1132
- transcript lines 1352-1371

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-followlistprojection-misses-initial-cache-serve-due.json`](transcripts/2026-06-19-2-followlistprojection-misses-initial-cache-serve-due.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-followlistprojection-misses-initial-cache-serve-due.json`](transcripts/raw/2026-06-19-2-followlistprojection-misses-initial-cache-serve-due.json)
