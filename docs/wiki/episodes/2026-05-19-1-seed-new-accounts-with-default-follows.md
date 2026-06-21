---
type: episode-card
date: 2026-05-19
session: f22be978-ccc6-42dd-bad0-2b2d5aba2999
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f22be978-ccc6-42dd-bad0-2b2d5aba2999.jsonl
salience: product
status: superseded
subjects:
  - chirp-onboarding
  - create-account
  - default-follows
supersedes: []
related_claims: []
source_lines:
  - 1-342
captured_at: 2026-06-18T04:28:09Z
---

# Episode: Seed new accounts with default follows on creation

## Prior State

New accounts were created with an empty contact list — no automatic follows were applied during onboarding.

## Trigger

User directive: 'when a new account is generated on chirp, make it follow npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft and fiatjaf's key'

## Decision

Added a `DEFAULT_FOLLOWS` constant (two hex pubkeys) and a `publish_initial_follows()` helper that builds and publishes a kind:3 contact-list event; wired into `create_account()` so every fresh account automatically follows both pubkeys.

## Consequences

- Every new Chirp account starts with two pre-seeded follows in its kind:3 contact list
- The follow list is broadcast immediately on account creation via the existing outbound pipeline
- Adding or removing default follows requires editing the `DEFAULT_FOLLOWS` constant in identity.rs

## Open Tail

*(none)*

## Evidence

- transcript lines 1-342

