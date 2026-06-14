---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - d0-thin-shell
  - business-logic-in-ui
  - embed-kind-projection
  - profile-projection
supersedes: []
related_claims: []
source_lines:
  - 8625-8626
  - 8650-8654
  - 8745-8746
captured_at: 2026-06-13T18:49:50Z
---

# Episode: Thin-shell D0 doctrine: 9 confirmed UI-logic violations cataloged for Wave 2 fix

## Prior State

D0 thin-shell doctrine existed in docs but 9 specific protocol-logic violations lived unaddressed in Swift/Kotlin shells — embed-kind dispatch, repost detection, relay-role labeling, NIP-29 default URLs, profile attribution, bunker-handshake completion, and more were each re-derived in platform code rather than consumed as Rust projections.

## Trigger

Deep-dive triage of all 88 open issues systematically identified 9 confirmed business-logic-in-UI violations (#1283, #980, #984, #989, #981, #920, #996, #626, #611) with specific Rust-side prescriptions for each.

## Decision

All 9 violations are queued for Wave 2 fix: each must be resolved by moving the logic into Rust projections (e.g., nmp-content EmbedKindProjection, nmp-core resolved_profiles, nmp-nip29 typed snapshot for defaults) so shells become decode-only renderers. No logic moves into a new crate — it already exists in the correct Rust crate; shells must stop re-shaping it.

## Consequences

- Wave 2 will target the embed/profile/timeline projection cluster as the core of 'no logic in UI'
- Swift EmbedHost resolver helpers, NostrKindRegistry kind switch, ModularBlockView.syntheticItem() shim, roleLabel/roleTint duplication, and NIP-29 hardcoded default-relay URL are all slated for deletion
- Each fix must pass doctrine grep gates (D0/D6/D7/D8) before merge

## Open Tail

- Wave 2 has not yet launched — the projection cluster fix is queued behind Wave 1b debt-fix
- #611 (bunker-dismiss) is already in Wave 1 (merged); the remaining 8 are Wave 2 scope

## Evidence

- transcript lines 8625-8626
- transcript lines 8650-8654
- transcript lines 8745-8746

