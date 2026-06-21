---
type: episode-card
date: 2026-05-25
session: 86221d39-67d3-484d-8979-b91cf75a5a72
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/86221d39-67d3-484d-8979-b91cf75a5a72.jsonl
salience: product
status: active
subjects:
  - kind0-fetch
  - profile-resolution
  - nostr-mention-rendering
supersedes: []
related_claims: []
source_lines:
  - 1-7
  - 57-87
captured_at: 2026-06-18T05:26:10Z
---

# Episode: Kind:0 profile fetch gap — content-embedded nostr: URIs never trigger fetches

## Prior State

NMP had five distinct kind:0 fetch paths (startup bootstrap, author-view open, UI-claimed profile, p-tag mention, and relay-list-driven). Content-embedded nostr:npub1…/nostr:nprofile1… URIs were tokenized for rendering but never triggered a kind:0 fetch. iOS Chirp had the FFI surface for claimProfile wired but no UI call site invoked it.

## Trigger

User observed that a large volume of pubkeys rendered in chirp (both iOS and TUI) never attempted kind:0 retrieval from indexer relays or the pubkey's own relays.

## Decision

Confirmed the architectural gap: content-embedded mention URIs must trigger kind:0 + kind:10002 fetches from indexer relays and (when known) from the pubkey's own relays. Profile-fetch-plan.md was written specifying the fix.

## Consequences

- Any rendered pubkey (including inline nostr: mentions) must produce a kind:0 + kind:10002 subscription
- claimProfile on iOS needs a UI call site — currently FFI-only with no consumer
- The five existing fetch paths remain; a sixth path for content-embedded mentions must be added
- Indexer relay selection follows existing route_outbox_subscription_relays logic

## Open Tail

- Implementation of the content-embedded mention fetch path not yet started
- TUI claim_profile seam may be missing entirely

## Evidence

- transcript lines 1-7
- transcript lines 57-87

