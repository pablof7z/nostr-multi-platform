---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - profiles-capability-seam
  - kind0-parser
  - profile-lookup-trait
supersedes:
  - 2026-06-15-4-profiles-extracted-from-kernel-to-capability
related_claims: []
source_lines:
  - 3202-3231
captured_at: 2026-06-15T15:14:58Z
---

# Episode: Profiles migrated out of kernel to capability/parser-owned cache

## Prior State

Kernel directly owned kind:0 processing — profiles: HashMap field, Profile struct in types.rs, ingest_profile in ingest/profile.rs, parse_profile + ProfileContent in nostr.rs

## Trigger

ADR-0057 unified chokepoint design requires each kind's processing to be owned by its capability/parser, not the kernel; profiles must become a capability seam mirroring the DM-inbox-relay pattern

## Decision

ProfileLookup narrow kernel-read trait + protocol-neutral ProfileView; nmp-nip01::Kind0Parser registered on kind:0 via EventIngestDispatcher owns parse/supersession/eviction; kernel reads via Arc<dyn ProfileLookup>; kernel kind:0 ingest arm deleted along with the profiles HashMap field, Profile struct, and parse_profile; chokepoint detects cache transition via before/after profile_lookup().profile() snapshot (mirrors mailbox pattern)

## Consequences

- Ingest path no longer names kind:0 (partial D0 advance for profiles)
- Future kind:0 format changes live in nmp-nip01, not the kernel
- Test-support TestProfileCache/TestKind0Parser for in-crate testing (mirrors TestDmInboxRelayCache precedent)
- estimated_bytes() added to the ProfileLookup trait for diagnostic accounting (kernel can no longer iterate cache values)

## Open Tail

- Contacts (kind:3) deliberately untouched — that is PR 3

## Evidence

- transcript lines 3202-3231
