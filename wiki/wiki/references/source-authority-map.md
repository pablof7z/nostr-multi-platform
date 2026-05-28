---
title: "Source Authority Map"
summary: "A lookup map from common NMP questions to the durable source that owns the answer."
tags: [references, sources]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-source-map.md"
  - "raw/notes/2026-05-28-temporal-plans-correction.md"
---

# Source Authority Map

Use this page to decide where a future wiki article should look first.

| Question | First Source |
|---|---|
| What is NMP trying to be? | `docs/aim.md` |
| What are the binding doctrine rules? | `docs/product-spec/doctrine.md` |
| What belongs in `nmp-core` vs protocol/app crates? | `docs/architecture/crate-boundaries.md` |
| How does the actor update loop work? | `docs/builder-guide/04-actor-and-tea.md` |
| How does reactivity stay bounded? | `docs/builder-guide/06-reactivity-contract.md` and `docs/design/reactivity/` |
| How do subscriptions compile into relay plans? | `docs/builder-guide/07-subscription-planner.md` and `docs/design/subscription-compilation/` |
| How does iOS consume the kernel? | `docs/builder-guide/17-ios-shell.md` and `ios/Chirp/Chirp/Bridge/` |
| What is the runtime update transport? | `docs/decisions/0037-typed-flatbuffers-runtime-projections.md`, `crates/nmp-core/schema/nmp_update.fbs`, and `crates/nmp-core/src/update_envelope.rs` |
| What is active right now? | `WIP.md` |
| What is queued or blocked? | `docs/BACKLOG.md` |
| What is the current release-plan view? | `docs/plan.md` |

## Wiki Use

Wiki articles should synthesize durable sources into a readable map. They should
not replace the source they summarize. If a wiki page needs to make a status
claim, it should identify the temporal source and the date of verification.

## See Also

- [[temporal-plans-vs-durable-docs|Temporal Plans vs Durable Docs]] ([Temporal Plans vs Durable Docs](../concepts/temporal-plans-vs-durable-docs.md))
- [[runtime-update-transport|Runtime Update Transport]] ([Runtime Update Transport](../topics/runtime-update-transport.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
- [Temporal Plans Product Correction](../../raw/notes/2026-05-28-temporal-plans-correction.md)
