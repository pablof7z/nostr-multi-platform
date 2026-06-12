---
type: episode-card
date: 2026-06-03
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
salience: architecture
status: active
subjects:
  - d0-violation
  - timeline-item
  - crate-boundaries
  - nmp-core
supersedes: []
related_claims: []
source_lines:
  - 1076-1135
  - 1155-1297
captured_at: 2026-06-11T22:58:15Z
---

# Episode: TimelineItem in nmp-core identified as D0 violation — social concepts must not live in substrate

## Prior State

`TimelineItem` (a social feed projection row with `is_repost`, `nav_target_id`, `repost_inner_content`, `author_lnurl`) lived in `nmp-core/src/kernel/types.rs` — the pure substrate kernel that must have zero NIP/kind-specific knowledge.

## Trigger

User questioned why "timeline" concepts live in nmp-core instead of `nmp-nip01`. The assistant confirmed `nmp-nip01` already owns `TimelineEventCard`, `TimelineBlock`, and `ModularTimelineSnapshot` — that is where timeline row types belong.

## Decision

`TimelineItem` must move out of `nmp-core` to `nmp-nip01`. A full D0 audit was launched and found 12 violations across the kernel. Issue #920 tracks the cleanup with a three-tier priority order.

## Consequences

- 12 D0 violations catalogued in `nmp-core/src/kernel/`: `TimelineItem`, `ingest/profile.rs` (kind:0), `ingest/contacts.rs` (kind:3), `ingest/timeline.rs` (kind:1 metric), `update/views.rs` (NIP-18 repost parsing), `update/helpers.rs` (`parse_repost_inner`), `local_publish_intent.rs` (kind:3), `publish_outbox.rs` (kind dispatch for display), `nostr.rs` (kind:0), `auth.rs` (kind:22242), `reply.rs` (kind:1 parent check), `ingest/mod.rs` (hardcoded kind dispatch)
- Reply-ref projection cannot simply be added to `TimelineItem` in-place — the struct itself is misplaced and deepens the D0 violation
- `reply.rs` kind:1 check is blocked by a dependency cycle (`nmp-nip01 → nmp-core`), requiring cycle-breaking

## Open Tail

- Issue #920: Tier 1 moves (TimelineItem, parse_repost_inner, ingest/profile, ingest/contacts) before Tier 2 cascades
- Adding reply tags to the projection requires solving where the projection type lives first

## Evidence

- transcript lines 1076-1135
- transcript lines 1155-1297

