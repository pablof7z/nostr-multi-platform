---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-kernel-profile-resolution
  - outbox-model-kind10002
  - claim-profile-liveness
supersedes:
  - 2026-06-15-1-third-party-profile-outbox-resolution-kind
related_claims: []
source_lines:
  - 48-83
  - 3160-3177
captured_at: 2026-06-15T10:01:35Z
---

# Episode: Outbox model activated for third-party profile resolution

## Prior State

The outbox router machinery existed but was inert for third-party profiles: kind:10002 (NIP-65 relay lists) were only fetched for the self/active account at startup. Profile claims used a bespoke bypass path (profile_claim_request → route_outbox_subscription_relays) that only queried indexer-role relays (primal, purplepag.es). There was no liveness distinction (CacheOk vs Live/Tailing), and probed_mailboxes was cleared on every relay connect, causing infinite re-probes.

## Trigger

User reported ~50% of pubkeys never resolve. Investigation revealed the 'fatal gap': for an arbitrary author X whose avatar is claimed, MailboxCache has no entry → Lane 1 empty → query only hits operator/indexer relays. purplepag.es was AUTH-walled anonymously, so anyone publishing kind:0 only to their own relays was silently unreachable.

## Decision

Migrated all profile claims through the outbox registry chokepoint (register_profile_claim_interest → recompile_and_diff_with_lookup), enabling kind:10002 discovery for third-party authors before issuing kind:0 queries. Added liveness hint parameter (CacheOk=one-shot for feed avatars, Live=Tailing for open profile screens) via 5-arg FFI (breaking 4→5). Fixed probed_mailboxes re-arm to only fire on genuine reconnect (indexer_socket_was_down). Exempted wasm/web from snapshot-on-claim (SolidJS <For> remount loop).

## Consequences

- kind:0 resolution measured 10.2% → 50.0% → 60.3% (5-6× improvement)
- Breaking FFI change: nmp_app_claim_profile 4→5 args; all consumer apps must add liveness parameter
- wasm/web dispatch arm uses no-snapshot-on-claim rule as a general invariant
- probed_mailboxes only re-arms on genuine reconnect, not every connect
- Released as nmp-v0.8.0 (PR #1436 kernel, PR #1438 version cut)

## Open Tail

- NIP-60 wallet (nmp-nip60 crate) still parked; issue #1434 filed for follow-up
- ~40% of follows still unresolved (no-NIP-65 cohort + relay availability variance)

## Evidence

- transcript lines 48-83
- transcript lines 3160-3177
