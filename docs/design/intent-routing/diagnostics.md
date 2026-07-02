# Intent-classed routing + NIP-50 search §6 — Diagnostic discipline

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: NIP-51 fact stream in `nip51-facts.md` (§5); cache-side search
> in `cache-search.md` (§7).

## 6. Diagnostic discipline

The four-lane model in
`docs/design/subscription-compilation/diagnostics.md` §5.0 stretches to
**seven lanes** once both ADR-0071 (this design) and ADR-0072 (relay
roles — Indexer + AppRelay) land:

1. NIP-65
2. Hint
3. Provenance
4. UserConfigured — account read/write, debug (the historical
   `UserConfigured(Indexer)` sub-category is REMOVED here per ADR-0072
   and promoted to lane 6)
5. **ClassRouted** — sourced from NIP-51 lists; carries
   `class: EventClass` and `via: Personal | PublisherKeyed(author)`.
6. **Indexer** (ADR-0072) — always-on for kind:0, kind:3, and
   kind:10000–19999. R+W symmetric. Source: operator default ∪ user's
   kind:10086 list.
7. **AppRelay** (ADR-0072) — per-author substitutive fallback when an
   author has no known NIP-65 mailbox. Whole-session lifetime.
   Source: operator default ∪ user-settings override.

Plus one global subtractive filter:

- **Blocked** — events removed because the target relay is in kind:10006.
  Surfaced as a separate diagnostic stream, not a routing lane (it never
  *adds* a relay, only subtracts). When the subtraction empties a plan,
  the planner errors with `AllRelaysBlocked` rather than continuing.

The diagnostic-discipline doc
(`docs/design/subscription-compilation/diagnostics.md`) is updated as
part of P3's deliverable — a single PR that introduces all three new
role tags simultaneously: `ClassRouted` (this ADR), `Indexer`, and
`AppRelay` (ADR-0072).
