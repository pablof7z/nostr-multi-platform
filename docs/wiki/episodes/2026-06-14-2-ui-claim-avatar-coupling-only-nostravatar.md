---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-ios-profile-resolution
  - nostr-avatar-claim
  - mention-profiles-gap
supersedes: []
related_claims: []
source_lines:
  - 289-337
  - 352-361
captured_at: 2026-06-14T21:20:46Z
---

# Episode: UI claim/avatar coupling — only NostrAvatar claims profiles

## Prior State

Documented invariant F-CR-00 states 'every author-displaying component self-claims on mount.' The mention_profiles projection was intended as a passive fallback for pubkeys fetched but not explicitly claimed.

## Trigger

iOS UI investigation revealed that only NostrAvatar (and ProfileView) call claim_profile. NostrProfileName is a pure rendering leaf with no claim logic. Mentioned pubkeys in note content (nostr:npub…), reply attributions, and repost attributions never claim. mention_profiles has been dead-empty since V-112/ADR-0042 — the projection derives exclusively from claimed_profiles.

## Decision

Confirmed as the primary ~50% driver: roughly half of displayed pubkeys in a typical feed are mentions/attributions (not avatar-bearing note authors), and these are never claimed so the kernel never surfaces their kind:0 even if it is already cached. The documented 'every component self-claims' invariant is violated. Fix dispatched to separate agent (pending design approval).

## Consequences

- ~50% of displayed pubkeys render as truncated npub/gradient+initials placeholders permanently
- A fetched-but-unclaimed kind:0 sits invisible in the kernel cache — the UI cannot see it
- The mention_profiles projection is a dead path (always empty) — all profile visibility flows through claimed_profiles only
- Fix requires every pubkey-displaying surface (NostrProfileName, mention labels, reply/repost attributions) to call claim_profile, not just NostrAvatar

## Open Tail

- Fix agent was dispatched but design not yet reviewed in this session
- Whether to add claim logic to NostrProfileName (making it non-pure) or to a parent wrapper component is an architectural choice
- Batched claim on mount for lists showing many mentioned pubkeys needs performance consideration

## Evidence

- transcript lines 289-337
- transcript lines 352-361
