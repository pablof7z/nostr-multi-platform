---
type: episode-card
date: 2026-05-19
session: 5d893073-9635-450b-b8e9-50648bc1a4e7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5d893073-9635-450b-b8e9-50648bc1a4e7.jsonl
salience: architecture
status: superseded
subjects:
  - kernel-bootstrap
  - relay-configuration
  - ffi-surface
supersedes: []
related_claims: []
source_lines:
  - 3734-3753
captured_at: 2026-06-18T04:20:28Z
---

# Episode: Eliminate hardcoded relay URLs and pubkeys from production kernel code

## Prior State

BOOTSTRAP_DISCOVERY_RELAYS, CONTENT_RELAY_URL, INDEXER_RELAY_URL, and hardcoded test pubkey constants (TEST_NPUB, TEST_PUBKEY, FIATJAF_PUBKEY, JB55_PUBKEY) were available in production code paths. RelayRole::bootstrap_url() and url() were production-callable. The kernel used these constants as fallback configuration.

## Trigger

Continuation of the seed-timeline removal — same doctrine applied to relay configuration: no hardcoded bootstrap data in production code

## Decision

Moved all relay URL constants and hardcoded pubkeys into #[cfg(test)] blocks. Production code now reads bootstrap URLs from app-provided relay_edit_rows via new Kernel::bootstrap_urls_for_role() and bootstrap_discovery_relays() methods. actor/relay_mgmt.rs spawn_missing_relays and kernel/outbox.rs cold-start fallbacks use kernel config, not constants.

## Consequences

- Production kernel has zero hardcoded relay URLs or pubkeys — all configuration flows from the FFI surface
- relay_edit_rows handle must survive Kernel::Reset (dispatch.rs now preserves it alongside drops_handle)
- New FFI methods bootstrap_urls_for_role() and bootstrap_discovery_relays() are the sole production entry points for relay configuration
- Test code retains access to the constants via #[cfg(test)]

## Open Tail

*(none)*

## Evidence

- transcript lines 3734-3753

