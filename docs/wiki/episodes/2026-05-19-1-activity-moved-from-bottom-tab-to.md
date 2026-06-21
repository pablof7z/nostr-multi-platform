---
type: episode-card
date: 2026-05-19
session: cb3376a7-cea1-49ac-b6dd-9251fa1af14a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cb3376a7-cea1-49ac-b6dd-9251fa1af14a.jsonl
salience: reversal
status: active
subjects:
  - chirp-navigation
  - activity-access-point
  - tab-bar-structure
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 70-70
  - 171-173
  - 260-269
captured_at: 2026-06-18T04:22:37Z
---

# Episode: Activity moved from bottom tab to top-right toolbar

## Prior State

Activity was a dedicated tab in the bottom TabView (6 tabs total). This caused a duplicate back-button bug on the Accounts settings screen, likely due to NavigationStack nesting issues with the extra tab.

## Trigger

User reported duplicate back buttons on Accounts settings (image evidence), then identified the root cause as the extra Activity tab on the bottom bar and explicitly directed moving Activity to a top-right toolbar.

## Decision

Remove Activity from the bottom TabView entirely; instead, add a bell toolbar button on HomeFeedView (top-right, left of compose) that presents NotificationsView in a NavigationStack sheet. Tab count reduced from 6 → 5.

## Consequences

- Duplicate back-button bug on Accounts is eliminated by reducing tab count from 6 to 5
- Activity is no longer a persistent tab — it is now a modal/sheet interaction pattern accessed only from HomeFeedView
- Users must be on the Home tab to access Activity; it is unreachable from other tabs via the bottom bar
- Navigation architecture simplified: fewer tab-level NavigationStacks reduces nesting conflicts

## Open Tail

- Whether the sheet-based Activity access is discoverable enough compared to a persistent tab icon
- Whether other screens need a path to Activity now that it's not globally available on the tab bar

## Evidence

- transcript lines 1-3
- transcript lines 70-70
- transcript lines 171-173
- transcript lines 260-269

