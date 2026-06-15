---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-chirp-config
  - nmp-recompile
  - relay-role-assignment
supersedes:
  - 2026-06-15-2-relay-role-doctrine-primal-as-app
related_claims: []
source_lines:
  - 3319-3328
  - 3425-3453
  - 3530-3563
captured_at: 2026-06-15T10:39:31Z
---

# Episode: primal.net changed from indexer to app relay; kind:10002 discovery made additive to app relays

## Prior State

relay.primal.net was configured as both,indexer in nmp-chirp-config. The kind:10002 discovery probe (D3 in recompile.rs) targeted only indexer_relays. With primal as an indexer, this probe had two targets (primal + purplepag).

## Trigger

User directive to make primal an app relay (line 3319). Analysis revealed this would leave purplepag.es as the sole indexer, and since purplepag AUTH-walls anonymous queries, the kind:10002 discovery probe would become inert → the outbox model (just fixed in v0.8.0) would regress. Additionally, empirical measurement showed adding an app relay (nos.lol) lifts resolution from 60% to ~89%, confirming app relays are structurally necessary for the no-NIP-65 cohort.

## Decision

Changed primal.net from both,indexer to both (app relay, no longer a dedicated indexer). Modified the kind:10002 discovery probe in recompile.rs to target indexer_relays ∪ app_relays (additive), and route the probe through auth_gate.partition() so probes to AUTH-walled/paused relays are correctly buffered. Added 3 new tests (app_relay_only_still_emits_mailbox_probe, probe_unions_indexer_and_app_relays, no_indexer_no_app_relay_means_no_probe).

## Consequences

- kind:0 queries are unaffected — the app-relay lane was already additive (case_a_authors.rs:145-156)
- kind:10002 discovery now survives losing dedicated indexers — primal-as-app-relay serves relay lists via the app lane
- purplepag.es is now the sole indexer; if it AUTH-walls, discovery still works because app relays carry the probe
- Web feed E2E verified clean — no recompile/snapshot loop (exactly 2 mailbox-probe REQs per run, dedup holds)
- PR #1448 merged to master as 71a442787

## Open Tail

- On-device deployment gap: phones are on v0.8.0 which predates #1448 and #1451 — primal-as-app-relay not yet running on device

## Evidence

- transcript lines 3319-3328
- transcript lines 3425-3453
- transcript lines 3530-3563
