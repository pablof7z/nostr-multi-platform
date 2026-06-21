---
type: episode-card
date: 2026-05-27
session: cd2b6122-2b7c-43fc-941b-c51e79ffc691
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cd2b6122-2b7c-43fc-941b-c51e79ffc691.jsonl
salience: architecture
status: active
subjects:
  - nmp-nip59
  - nmp-store
  - dead-code-policy
  - nmp-doctrine
supersedes: []
related_claims: []
source_lines:
  - 3970-3999
  - 4005-4060
  - 4082-4132
captured_at: 2026-06-18T06:10:48Z
---

# Episode: Dead-code handling doctrine: delete outright, do not backlog-track

## Prior State

Two production functions — gift_wrap() in nmp-nip59 and nip40_row() in nmp-store — were annotated with #[allow(dead_code)] and retained as 'reference implementations' despite having zero callers. The implicit assumption was that dead code should be tracked as a violation if it couldn't be immediately removed.

## Trigger

Scaffolding audit found both items. Opus validation agent determined that gift_wrap was superseded by gift_wrap_with_signer (ADR-0026 seam) and the 'reference impl' justification was fiction since EventBuilder::gift_wrap from the nostr crate serves that role; nip40_row had zero callers because gc.rs inlines its own logic.

## Decision

Established a doctrine: #[allow(dead_code)] on production code with zero callers and no deletion date is itself a violation, but the correct remedy is immediate deletion, not adding a V-entry to BACKLOG.md. Both functions were deleted in the same commit as the V-77..V-79 additions.

## Consequences

- gift_wrap() removed from nmp-nip59/src/wrap.rs — only gift_wrap_with_signer remains as the ADR-0026 seam
- nip40_row() removed from nmp-store/src/lmdb/tombstones.rs — gc.rs already inlines its own construction
- Sets precedent: unjustified dead code gets deleted, not bureaucratically tracked
- Unused import warnings for EventBuilder and Tag appeared after gift_wrap removal, requiring a follow-up cleanup

## Open Tail

- The doctrine-lint grep (D6 gate) should potentially also flag #[allow(dead_code)] on non-test items as a CI failure, enforcing this precedent automatically

## Evidence

- transcript lines 3970-3999
- transcript lines 4005-4060
- transcript lines 4082-4132

