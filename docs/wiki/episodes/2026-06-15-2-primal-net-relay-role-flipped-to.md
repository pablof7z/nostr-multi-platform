---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-chirp-config-relay-roles
  - nmp-core-discovery-routing
  - relay-lane-architecture
supersedes:
  - 2026-06-15-2-primal-net-changed-from-indexer-to
related_claims: []
source_lines:
  - 3319-3330
  - 3425-3448
  - 3577-3598
captured_at: 2026-06-15T10:55:53Z
---

# Episode: primal.net relay role flipped to app-only, kind:10002 discovery probe made additive to app relays

## Prior State

relay.primal.net was configured as `both,indexer` (app relay + dedicated indexer). The kind:10002 discovery probe (the D3 probe in recompile.rs) targeted only `indexer_relays`. purplepag.es was also an indexer. Profile resolution stacked: indexers → outbox → but discovery of relay lists depended solely on dedicated indexers.

## Trigger

User measured that adding a broad app relay (nos.lol) lifts resolution from 60% to 89% — app relays are critical. User then explicitly directed: 'make primal an app relay instead of an indexer.' Investigation revealed that flipping primal to app-only would leave purplepag.es (which AUTH-walls anonymous queries) as the sole indexer, silently killing the kind:10002 discovery probe and regressing the outbox model.

## Decision

Flipped primal.net's default Chirp role from `both,indexer` to `both` (app relay, no longer a dedicated indexer). This is app-level config in nmp-chirp-config, not in nmp-core. To prevent the regression, also changed nmp-core's kind:10002 discovery probe to target `indexer_relays ∪ app_relays` (deduplicated via BTreeSet) instead of `indexer_relays` only. Routed probe frames through auth_gate.partition() so probes to paused/AUTH-walled app relays are correctly buffered. Added 3 tests.

## Consequences

- Profile resolution now stacks three additive tiers: indexers (~28%) → outbox via author's own relays (~60%) → app relays (~89%)
- The kind:10002 discovery probe survives having zero or only AUTH-walled dedicated indexers, because it also targets app relays
- The nmp-core routing change introduces a framework-level policy ('discovery isn't indexer-only') that the user flagged concern about — they explicitly stated relay config MUST be app-level and have not yet decided whether to keep or revert the nmp-core probe change
- Web feed E2E test verified clean against the recompile.rs change (3/3 passes, no snapshot loop, correct probe count)

## Open Tail

- User asked whether the nmp-core routing change (kind:10002 probe → indexer ∪ app_relays) should stay in the framework or be reverted to keep discovery purely app-configurable — decision pending

## Evidence

- transcript lines 3319-3330
- transcript lines 3425-3448
- transcript lines 3577-3598
