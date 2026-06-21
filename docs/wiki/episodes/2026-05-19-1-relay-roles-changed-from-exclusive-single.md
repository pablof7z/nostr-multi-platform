---
type: episode-card
date: 2026-05-19
session: 87fd49fb-4869-4c40-9a6a-96545bd2313d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/87fd49fb-4869-4c40-9a6a-96545bd2313d.jsonl
salience: product
status: active
subjects:
  - relay-roles
  - relay-edit-row
  - chirp-relay-settings
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 1829-1830
  - 1849-1850
  - 2198-2213
  - 2705-2712
captured_at: 2026-06-18T04:24:13Z
---

# Episode: Relay roles changed from exclusive single-choice to additive multi-capability

## Prior State

Relay role was a single forced-choice string field: "read" | "write" | "both" (and optionally "indexer", "wallet"). The Swift picker and Rust normalize_role() mapped to one value. A relay could only serve one role at a time.

## Trigger

User explicitly requested: "when CRUDing a relay I should be able to choose what role to use the relay for (read, write, indexer, etc)" — implying multi-select additive capabilities rather than exclusive choice.

## Decision

Relay role semantics changed to an additive, space-separated capability list (e.g. "indexer read write"). normalize_roles() parses, deduplicates, and sorts tokens. has_role() checks containment with backward-compat for legacy "both" (maps to read+write). The Swift UI replaced the single-picker with four independent toggles (Read, Write, Indexer, Wallet). Relay list rows render a colored badge per capability. bootstrap_urls_for_role and author_write_relays in the Rust kernel now use has_role() instead of exact-match.

## Consequences

- Legacy "both" entries are backward-compatible: has_role("both", "read") and has_role("both", "write") both return true
- Adding the same URL twice merges roles additively rather than overwriting (second add merges with existing role set)
- RelayEditRow.role field is now a space-separated string; any consumer that did exact-match comparison must use has_role() instead
- role_for_relay_url and bootstrap_urls_for_role updated to use has_role for correct multi-capability matching
- has_role re-exported through actor::commands for kernel/outbox.rs to use without breaking D0 module boundary

## Open Tail

- Wallet role toggle exists in UI but kernel-side bootstrap_urls_for_role returns empty for Wallet (dynamic NWC URI), so the toggle is cosmetic until NIP-47 integration deepens

## Evidence

- transcript lines 1-3
- transcript lines 1829-1830
- transcript lines 1849-1850
- transcript lines 2198-2213
- transcript lines 2705-2712

