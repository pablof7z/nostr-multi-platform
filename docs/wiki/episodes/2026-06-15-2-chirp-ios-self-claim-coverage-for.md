---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - chirp-ios
  - claim-profile
  - ios-ui
  - profile-liveness
supersedes: []
related_claims: []
source_lines:
  - 41-42
  - 2582-2632
captured_at: 2026-06-15T09:21:42Z
---

# Episode: Chirp iOS self-claim coverage for mentions/attributions/names

## Prior State

Many Chirp iOS UI surfaces (mention pills, reply attributions, standalone name displays, reaction/repost authors) never called claim_profile at all, so those pubkeys never entered the kernel's resolution pipeline and their names/avatars remained permanently blank.

## Trigger

Investigation found the iOS UI only claimed profiles for some surfaces (feed note authors), missing mentions, attributions, and standalone names — a separate contributor to the ~50% unresolved rate beyond the kernel-side outbox gap.

## Decision

All inline/list self-claiming surfaces (feed avatars, mention pills, reply attribution, standalone names) now call claim_profile with liveness=.cacheOk. Profile screen calls with liveness=.live. KernelBridge and NmpCore.h updated to the 5-arg signature matching the kernel.

## Consequences

- More pubkeys enter the resolution pipeline on iOS, reducing blank avatars/names
- FFI header consistency enforced (5-arg on both Rust and Swift sides)
- iOS unit tests (ProfileClaimSurfaceTests) added to prevent regression
- Registry export JSON needed regeneration for new Swift components

## Open Tail

*(none)*

## Evidence

- transcript lines 41-42
- transcript lines 2582-2632
