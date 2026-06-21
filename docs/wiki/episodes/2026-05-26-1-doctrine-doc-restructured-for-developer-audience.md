---
type: episode-card
date: 2026-05-26
session: 56d215c4-1aee-47cc-95c2-fd17269b92b6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/56d215c4-1aee-47cc-95c2-fd17269b92b6.jsonl
salience: product
status: active
subjects:
  - doctrine-doc
  - nmp-product-spec
  - doc-voice
supersedes: []
related_claims: []
source_lines:
  - 14221-14384
captured_at: 2026-06-18T06:07:40Z
---

# Episode: Doctrine doc restructured for developer audience after user found it incomprehensible

## Prior State

docs/product-spec/doctrine.md was written for internal agents and contributors — full of NIP numbers (NIP-77, NIP-65, NIP-59), kind codes (kind:0, kind:3, kind:1059), code snippets, and technical headings like 'Negentropy first, REQ second' and 'Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded'. Linked directly from the marketing landing page (nostr-mp.f7z.io → StartHere → doctrine.md), but opaque to developers arriving from there.

## Trigger

User explicit feedback after reviewing the marketing-linked doctrine page: 'the product-spec doctrine stuff, linked directly from the main marketing page into the github repo, is very hard to understand, I don't even understand what half the things mean'

## Decision

Restructured every doctrine entry into a dual-audience format: (1) plain-English headline replacing jargon, (2) 1–2 sentence plain-English lead explaining what it means for a developer, (3) 'This rules out:' bullets rewritten in plain English with no unexplained jargon, (4) implementation details preserved but demoted to italic *Implementation detail: …* blocks. NIP numbers and kind codes explained inline on first use (e.g. 'a set-reconciliation protocol (NIP-77 / negentropy)'). FFI explained as 'Rust ↔ Swift/Kotlin boundary'. Key headline changes: D2 'History syncs by diff, not by re-download', D8 'Reactivity is bounded — UI updates stay predictable under any event volume', D10 'Private messages stay private; the framework enforces it'.

## Consequences

- The marketing page now links to a doctrine document comprehensible to developers without Nostr protocol knowledge
- Technical depth preserved for agents and ADR citations via demoted italic blocks — no information lost
- Established a reusable pattern for dual-audience docs: plain-English surface with implementation depth underneath
- Every doctrine headline changed, so any cross-references using old heading text need updating

## Open Tail

- Other builder-guide chapters may need the same dual-audience treatment if they're also linked from public-facing pages

## Evidence

- transcript lines 14221-14384

