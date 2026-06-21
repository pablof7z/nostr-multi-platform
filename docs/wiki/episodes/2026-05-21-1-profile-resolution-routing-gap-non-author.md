---
type: episode-card
date: 2026-05-21
session: 7b4ae585-801c-441f-811d-5308e1002f08
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7b4ae585-801c-441f-811d-5308e1002f08.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - kernel-discovery
  - snapshot-projection
  - claim-lifecycle
supersedes: []
related_claims: []
source_lines:
  - 139-139
  - 504-547
  - 592-684
  - 697-777
  - 803-879
  - 885-962
captured_at: 2026-06-18T04:44:20Z
---

# Episode: Profile resolution routing gap — non-author pubkeys never rendered

## Prior State

Mention p-tagged pubkeys were routed exclusively through the capped discovery path (MAX_DISCOVERY_CONCURRENCY=2, DISCOVERY_BATCH=50) — separate from the uncapped batched author claim path. KernelSnapshot projected only per-item author profiles (TimelineItem.authorDisplay/authorPictureUrl) with no pubkey→profile map for non-authors. ingest_contacts extracted follow p-tags but never claimed their kind:0. kernel.profiles was never rehydrated from LMDB on cold start. The FFI seam claimProfile existed but had zero Swift callers.

## Trigger

User reported that mentions and pubkeys throughout Chirp render as truncated hex (e.g. @dd664d5e…d319) instead of resolved kind:0 display names, even when the kernel has the profile cached — 'as if we never attempted to retrieve it'

## Decision

Three-PR fix plan: (1) Route mention p-tags through the uncapped batched claim path via request_profile_for_rendered_note alongside authors, lift discovery cap for profiles arm only; (2) Add profiles_by_pubkey HashMap to snapshot projections + wire Swift .task(id:) claim/release lifecycle per visible pubkey; (3) Batch-claim kind:0 for the entire follow set during kind:3 ingest. Agent B PoC committed at 7320f0ba adding MentionProfileWire, mention_profiles_projection, and NoteContentView claim wiring.

## Consequences

- Mention p-tags now flow through pending_profile_claim_requests (one batched REQ per relay, no concurrency cap) instead of the 2-slot discovery queue
- Swift views can resolve non-author profiles via the new profiles_by_pubkey projection; mention pills, embedded cards, DM peers all benefit
- Follow-set profiles are bulk-fetched on contacts ingest, fixing the cold-start 'every author stays hex until they happen to post' symptom
- No 100ms coalescing window needed — drain-per-Message path naturally coalesces inside BTreeSet between drains
- discovery_tests::many_unknown_ids_collapse_to_few_batch_reqs and quoted_note_missing_id_is_discovered_and_resolvable_via_oneshot require test updates after routing change
- PR-1 is M2-aligned: compiler.md §3.5 retires pending_profile_claim_requests into LogicalInterest, so routing more demand through claims now makes that future cut cleaner
- Cold-start still only queries purplepag.es as indexer; multi-indexer fanout (relay.nostr.band, kindpag.es) deferred to follow-up

## Open Tail

- nostr:npub1… content mentions without p-tags are never claimed — kernel doesn't tokenize content for discovery
- Kind:7 reactions fall through to default ingest arm with no profile claim
- NIP-17 DM senders and NIP-29 group chat senders render raw hex with no mentionProfiles lookup in those views
- EOSE-never-arrives pins both discovery slots forever with no timeout eviction
- profile.rs:293-296 unconditionally marks pending authors as requested even when by_relay is empty (dormant but latent bug)
- Profile struct lacks lud16/banner/website fields

## Evidence

- transcript lines 139-139
- transcript lines 504-547
- transcript lines 592-684
- transcript lines 697-777
- transcript lines 803-879
- transcript lines 885-962

