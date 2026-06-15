---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - profiles-capability
  - profile-lookup-trait
  - kind0-parser
  - d0-profiles
supersedes:
  - 2026-06-15-3-profiles-migrated-out-of-kernel-to
related_claims: []
source_lines:
  - 3202-3233
captured_at: 2026-06-15T15:26:03Z
---

# Episode: Profiles migrated out of kernel to capability-owned cache

## Prior State

Profile data (kind:0) was kernel-owned: kernel `profiles: HashMap` field, kernel `Profile` struct, kernel `parse_profile`/`ProfileContent`, kernel `ingest_profile` arm. Kernel directly named and handled kind:0.

## Trigger

ADR-0057 PR 2 implementation — profiles are the second data type to migrate out of the kernel (after DM-inbox relays), following the capability-seam pattern.

## Decision

New narrow `ProfileLookup` trait + protocol-neutral `ProfileView` in kernel; `nmp_nip01::Kind0Parser` registered on kind:0 via EventIngestDispatcher owns parse, supersession (newest-wins + lexicographic event-id tiebreak), and RAM eviction. Kernel `profiles` field, `Profile` struct, `ingest_profile` arm, and `parse_profile` all deleted with no compat shims. Chokepoint detects cache transition via before/after `profile_lookup().profile()` snapshot and bumps `profiles_ver`. The kernel ingest path no longer names kind:0.

## Consequences

- Kernel cannot iterate cache values directly — `estimated_bytes()` added to trait for diagnostic byte accounting
- In-crate unit tests use test-support `TestProfileCache` + `TestKind0Parser` (mirrors existing `TestDmInboxRelayCache` precedent) since nmp-core can't depend on nmp-nip01
- Contacts (kind:3) deliberately untouched — that is PR 3
- Cache-serve replay must also handle profiles_ver transition (found by codex review, folded into fan-out unification)

## Open Tail

- PR 2 under rework for cache-serve fan-out unification before landing

## Evidence

- transcript lines 3202-3233
