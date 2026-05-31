---
title: Home-Timeline Seed Content & Bootstrap
slug: home-timeline-seed-content
summary: Home-timeline seed content (seed-bootstrap, seed-contacts, seed-profiles, seed-relays) still uses hardcoded fiatjaf/jb55/pablof7z pubkeys and requires a separat
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
---

# Home-Timeline Seed Content & Bootstrap

## Seed Content Bootstrap

The timeline displays only the logged-in user's feed, with no seed timeline for cold starts. Apps must be able to request a timeline using a semantic filter like "authors:[current-users-follows]" without needing to manually fetch kind:3, kind:10002, connect to relays, or add the current user's pubkey to the authors filter. The `SeedAccount` type and `seed_accounts()` function are removed from the codebase. The `startup_requests` function fetches the active user's own kind:3, profile, and relay list (returning empty if no account is signed in), instead of emitting seed-bootstrap, seed-contacts, seed-profiles, or seed-relays for a hardcoded trio. The timeline authors set includes the active user's own pubkey so they see their own posts, and excludes hardcoded seed pubkeys. The `maybe_open_timeline` function builds the author set solely from the active account's contacts. The `should_open_timeline` function gates timeline opening on the active account's kind:3 contacts having arrived or a 3-second deadline elapsing, not on hardcoded seed contact lists. The `retarget_timeline` function emits a self-contacts REQ on sign-in or account switch so the timeline can open even when sign-in happens after startup. The `sign_in_nsec`, `create_account`, and `switch_active` flows reconcile the M2 follow-feed and emit bootstrap REQs for the new active account so the follow feed works when login happens after cold-start. The subscription ID prefix for the follow timeline is `follow-timeline-` (the legacy `seed-timeline-` prefix is kept alive in the EOSE handler for in-flight subscriptions). The `status.rs` diagnostic label for the follow timeline is `FollowTimeline` instead of `SeedTimeline(fiatjaf,jb55,pablof7z)`. The hardcoded seed timeline removal is committed in master.

<!-- citations: [^09da8-2] [^fc128-1] [^5d180-1] -->
## See Also

