---
type: episode-card
date: 2026-05-19
session: 50510273-d1c9-424a-b877-179d52fba557
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/50510273-d1c9-424a-b877-179d52fba557.jsonl
salience: product
status: active
subjects:
  - publish-routing
  - indexer-relay-semantics
  - has-role
  - outbox-resolver
supersedes: []
related_claims: []
source_lines:
  - 1-6
  - 37-44
  - 45-47
  - 72-78
  - 156-164
  - 277-286
  - 309-316
  - 318-345
  - 554-667
  - 910-920
captured_at: 2026-06-18T04:27:34Z
---

# Episode: Indexer relay publish routing: discovery kinds fan out to indexers, has_role subsumption removed

## Prior State

has_role treated 'indexer' as semantically including 'write' (r=="indexer" && n=="write" clause), and all three docs asserted 'indexers are read-only discovery infrastructure — never publish to indexers' as a blanket rule. No event-kind differentiation existed in the publish path; OutboxResolver::resolve had no kind parameter.

## Trigger

User corrected the assistant's explanation that 'indexer relays accept writes': indexer relays exist for kind:0/3/1xxxx retrieval, NOT for general writes. This invalidated both the has_role subsumption hack AND the blanket 'never publish to indexers' doc rule.

## Decision

Two-part fix on the same decision surface: (1) Remove the (r=="indexer" && n=="write") clause from has_role so indexer relays no longer bleed into write-relay selection. (2) Add kind-aware fan-out: ALL events publish to write relays, and discovery kinds (0, 3, 1xxxx) ADDITIONALLY fan out to the user's configured indexer relays. Implemented via is_discovery_kind(u32), a new kind: u32 parameter on OutboxResolver::resolve, and an Arc<Mutex<Vec<String>>> indexer-relays handle synced from kernel relay_edit_rows.

## Consequences

- has_role no longer subsumes indexer→write; indexer is only matched by explicit 'indexer' needle
- Content events (kind:1, 6, 7, etc.) route exclusively to NIP-65 write relays — no indexer leakage
- Discovery events (kind:0, 3, 10000–19999) get dual fan-out: write relays + indexer relays, ensuring profile/contact-list discoverability
- OutboxResolver trait signature changed — all call sites and mocks must supply kind
- Three doc files updated: subsystems.md routing table, subscription-compilation/outbox.md preamble+step 2, framework-magic/outbox.md C7 assertion
- nip65/mod.rs stale 'do not publish to indexers' comment removed; indexer fan-out is now caller's concern
- Nip65OutboxResolver holds an Arc<Mutex<Vec<String>>> for indexer relays, kept current via kernel set_relay_edit_rows — no stale relay config on publish

## Open Tail

- marmot/ops.rs had a linter-corrupted diff for publish_key_package — needs manual fix and separate commit
- RelayRole enum gained a Bunker variant (from the same prior commit) — may need similar publish-path routing decisions for bunker relays

## Evidence

- transcript lines 1-6
- transcript lines 37-44
- transcript lines 45-47
- transcript lines 72-78
- transcript lines 156-164
- transcript lines 277-286
- transcript lines 309-316
- transcript lines 318-345
- transcript lines 554-667
- transcript lines 910-920

