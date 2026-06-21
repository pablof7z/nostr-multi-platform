---
type: episode-card
date: 2026-05-19
session: fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53.jsonl
salience: architecture
status: superseded
subjects:
  - relay-bootstrap
  - nmp-core-relay-rs
  - kernel-bootstrap
  - relay-edit-rows
supersedes:
  - 2026-05-19-4-eliminate-hardcoded-relay-urls-and-pubkeys
related_claims: []
source_lines:
  - 76-81
  - 733-755
  - 776-806
  - 837-898
  - 900-912
  - 913-935
  - 936-960
  - 985-1005
  - 1007-1033
  - 1238-1250
  - 1491-1505
captured_at: 2026-06-18T04:25:20Z
---

# Episode: Relay source-of-truth moved from hardcoded kernel constants to app-provided config

## Prior State

The kernel hardcoded relay URLs (wss://relay.damus.io, wss://purplepag.es) and pubkeys (TEST_NPUB, TEST_PUBKEY, FIATJAF_PUBKEY, JB55_PUBKEY) as production constants. The Swift side also had hardcoded relay arrays in MarmotBridge and a hardcoded default in KernelBridge.nostrConnectURI. All cold-start, outbox fallback, profile discovery, and startup REQ paths read from these constants.

## Trigger

User emphatically corrected that no relay should be hardcoded in the kernel — 'damus shouldn't be hardcoded ANYWHERE' — and specified that Chirp defaults to wss://r.f7z.io (both) + wss://purplepag.es (indexer), provided by the app side.

## Decision

All relay URLs and pubkeys were removed from production code paths. BOOTSTRAP_DISCOVERY_RELAYS, CONTENT_RELAY_URL, INDEXER_RELAY_URL, and all hardcoded pubkeys are now gated behind #[cfg(test)] only. A new Kernel::bootstrap_urls_for_role(role) method reads from app-provided relay_edit_rows (empty in production if unconfigured). Kernel::bootstrap_discovery_relays() unions indexer + content URLs from the same source. Swift KernelModel.addDefaultRelaysIfNeeded() seeds the two Chirp-default relays before kernel.start() if relayEditRows is empty. MarmotBridge reads relays from a provider closure backed by the app's relay config. KernelBridge.nostrConnectURI no longer has a default relay parameter.

## Consequences

- The kernel has zero hardcoded relay URLs or pubkeys in production code — #[cfg(test)] fallbacks preserve test behavior with damus/purplepag.es
- Production code returns an empty vec from bootstrap_urls_for_role when no app relays are configured, meaning the kernel will not connect anywhere unless the app seeds relays first
- All former call sites (relay_mgmt spawn_missing_relays, outbox cold-start fallback, profile REQs, startup REQs, status projection, subscription lifecycle) now route through the app-provided config
- Swift must call addDefaultRelaysIfNeeded() before kernel.start() or the kernel will have no bootstrap relays
- role_for_relay_url() changed from constant-indexed lookup to relay_edit_rows iteration
- req() changed from returning a single OutboundMessage to returning Vec<OutboundMessage> (one per configured bootstrap URL per role)
- Startup REQ emission changed from .push() to .extend() to accommodate multiple indexer URLs

## Open Tail

- The Wallet RelayRole has no role-matching logic in bootstrap_urls_for_role yet (returns empty) — may need app-side wallet relay configuration later
- Minor unused_mut warning on bootstrap_urls_for_role return value (#[cfg(test)] guard)

## Evidence

- transcript lines 76-81
- transcript lines 733-755
- transcript lines 776-806
- transcript lines 837-898
- transcript lines 900-912
- transcript lines 913-935
- transcript lines 936-960
- transcript lines 985-1005
- transcript lines 1007-1033
- transcript lines 1238-1250
- transcript lines 1491-1505

