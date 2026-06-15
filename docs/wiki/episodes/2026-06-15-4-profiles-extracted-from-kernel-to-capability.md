---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - profiles-capability-seam
  - profile-lookup-trait
  - kind0-parser
supersedes:
  - 2026-06-15-2-profiles-and-contacts-caches-stay-kernel
related_claims: []
source_lines:
  - 3202-3232
captured_at: 2026-06-15T15:03:56Z
---

# Episode: Profiles extracted from kernel to capability/parser-owned cache

## Prior State

Profile data (kind:0) owned by the kernel: profiles: HashMap<Pubkey, Profile> field, kernel-owned Profile struct, parse_profile + ProfileContent in nostr.rs, ingest_profile in kernel/ingest/profile.rs. ~10 synchronous readers reached self.profiles directly.

## Trigger

ADR-0057 direction to move protocol-specific logic out of the kernel — kind:0 handling violates D0 (the ingest path names kind:0 explicitly). Profiles are the next extraction after DM-inbox-relay.

## Decision

ProfileLookup narrow kernel-read trait + protocol-neutral ProfileView in substrate. nmp_nip01::Kind0Parser registered IngestParser owns parse, supersession (newest-wins + event-id tiebreak), and RAM eviction. Kernel profiles field + Profile struct + parse_profile deleted (no compat shims). Chokepoint detects cache transition via before/after profile_lookup().profile() snapshot. All ~10 readers migrated to trait.

## Consequences

- Ingest path no longer names kind:0 (D0 for profiles achieved)
- RAM eviction now cache-owned with kernel pin-set as input
- estimated_bytes() added to trait since kernel can no longer iterate cache values
- Test-support TestProfileCache + TestKind0Parser needed for in-crate tests (mirrors TestDmInboxRelayCache precedent)
- Contacts (kind:3) deliberately untouched — PR 3

## Open Tail

- Codex review of PR 2 still in flight
- PR 3 (contacts extraction) queued after PR 2 lands

## Evidence

- transcript lines 3202-3232
