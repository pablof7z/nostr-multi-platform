---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - adr-source-of-truth
  - plan-files-demoted
supersedes: []
related_claims: []
source_lines:
  - 1881-1884
  - 1906-1913
captured_at: 2026-06-15T13:54:27Z
---

# Episode: ADR established as durable authority; plan files demoted to temporal

## Prior State

ADR-0057 referenced `docs/plans/arch-fixes.md` as 'the source of every decision here.' Plan files were treated as authoritative decision records.

## Trigger

Codex review identified that ADRs citing temporal plan files as authoritative creates a durability problem — plan files are WIP marked for deletion on merge. An ADR must stand alone as the durable record.

## Decision

ADR-0057 is the self-contained durable authority. All `arch-fixes.md` references reframed as 'temporal tactical PR-sequencing tracker — deleted when the work merges — not an authority for any decision.' GitHub issues (#1440, #1442, #1443) are the durable 'why.' The ADR header now states this explicitly.

## Consequences

- ADRs no longer depend on ephemeral plan files as source-of-truth
- Decision provenance is traceable via durable GitHub issues, not temporal WIP documents
- Three durable docs (08-eventstore.md, subsystems.md, crate-boundaries.md) verified clean — no admission framing to remove; the wrong framing only existed in ADR-0042 and the code

## Open Tail

*(none)*

## Evidence

- transcript lines 1881-1884
- transcript lines 1906-1913
