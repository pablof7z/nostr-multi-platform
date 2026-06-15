---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-kernel-profile-claims
  - nmp-subscription-lifecycle
  - nip65-outbox-resolution
supersedes: []
related_claims: []
source_lines:
  - 69-88
  - 713-785
  - 886-989
captured_at: 2026-06-14T21:37:23Z
---

# Episode: M2 migration: profile claims onto generic subscription chokepoint with probe-epoch retry

## Prior State

claim_profile used a bespoke relay-aware path (profile_claim_request, pending_profile_claim_requests, ProfileRequestState) that built kind:0 REQs directly via route_outbox_subscription_relays, bypassing the subscription registry and recompile_and_diff_with_lookup chokepoint. Strangers' NIP-65 relay lists were never fetched (the generic D3 10002 probe only fires for feed-registered authors). Additionally, probed_mailboxes was insert-only with no TTL or retry — authors whose 10002 wasn't on the indexer at probe time were permanently abandoned.

## Trigger

Investigation revealed (1) the generic 10002 probe in recompile.rs:141-193 only fires for feed-registered authors, not claimed strangers; (2) probed_mailboxes is never cleared on indexer reconnect; (3) user layering correction: higher-order features (follows feed, profile claim) must NOT be relay-aware — they inherit routing from underlying infrastructure, not call ensure_relay_lists_for_authors explicitly

## Decision

Migrate profile claims onto the registry as LogicalInterest{kinds:[0], authors:[P], bootstrap_fallback: IndexerOnly} so they inherit 10002 discovery, set-cover minimization, progressive re-route, and nprofile hint seeding from the single infrastructure chokepoint. Three component fixes: (1) BootstrapFallback::IndexerOnly extension to InterestShape/case_a_authors preserves the kind:0-no-leak-to-content-relay contract; (2) probe-epoch retry — epoch-gate probed_mailboxes so missed 10002 probes re-fire on indexer reconnect or new-relay-added; (3) thread nprofile TLV relay hints into claim interest.hints. Delete bespoke profile_claim_request, ProfileRequestState, pending_profile_claim_requests, and refresh_profile_after_mailbox — all subsumed by registry + Nip65Arrived recompile.

## Consequences

- Strangers' profiles resolve via their own NIP-65 write relays (not just indexers) — fixes the random-profile-visit case
- Missed 10002 probes automatically retry on any relay-availability change (indexer reconnect, new relay added)
- nprofile-embedded relay hints seed resolution for authors not on any indexer
- Bespoke relay-aware code in profile.rs deleted — feature code is now genuinely relay-oblivious
- Two compiler extensions required: BootstrapFallback enum on InterestShape, and limit handling (recommended limit:None for replaceable kind:0 to avoid Rule 5 merge-block)
- Set-cover relay minimization (selection.rs greedy_select) confirmed to already exist — no artificial connection cap needed
- F-TTL re-verify and refcounted claims preserved through independent mechanisms (claim_replaceable + registry Slot owners)

## Open Tail

- Extension 2 form: limit:None simplification (recommended, no merge-lattice change) vs limit_semantics: PerAuthor (explicit, touches Rule 5)
- claimed_profiles projection: keep profile_claims map as projection SoT and drive registry off it (recommended) vs derive from registry owner set
- Claim interest lifecycle: OneShot (matches today) vs Tailing (stays live for reactive kind:0 replacements while avatar is on-screen)
- No code until owner approves design and resolves three open decisions

## Evidence

- transcript lines 69-88
- transcript lines 713-785
- transcript lines 886-989
