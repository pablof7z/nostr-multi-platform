---
title: "Crate Boundaries and Module Ownership"
summary: "NMP separates kernel substrate, routing implementations, protocol crates, app composition, bindings, and platform shells."
tags: [crates, architecture, d0]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-source-map.md"
  - "raw/repos/2026-05-28-app-composition-and-chirp-sources.md"
---

# Crate Boundaries and Module Ownership

NMP's crate boundary rule is an ownership rule. A concept belongs where its
facts and policy are owned, not where it happens to be convenient to call from.

The durable architecture spec owns the long-term crate graph. It should not own
implementation progress tables or migration status. Those are temporal release
facts.

## Layer Intent

- Kernel substrate owns actor state, action dispatch seams, capability sockets,
  snapshot envelopes, and generic contracts.
- Routing and planning crates implement routing algorithms and subscription
  compilation without leaking app nouns into the kernel.
- Protocol crates own reusable Nostr protocol modules that another app could
  use.
- App crates own app-specific domain policy and composition.
- Binding crates expose generated or hand-written FFI surfaces.
- Native app shells render and execute capabilities.

## Generic Nostr vs App-Specific

A feature belongs under `crates/` when it is reusable Nostr infrastructure. It
belongs under `apps/<app>/` when it is specific to that app's domain. The test is
not "is this protocol-shaped"; the test is whether a different Nostr app would
use the same crate directly.

This distinction is how NMP keeps D0 enforceable. The kernel should not gain
terms like "podcast episode" or "Chirp home feed policy". A protocol module may
own a reusable projection or action; an app crate composes those modules into
its product.

`nmp-app-template` sits at the composition layer, not in the kernel. It wires
generic Nostr defaults that many apps should inherit. `apps/chirp/nmp-app-chirp`
then adds product-specific Rust glue: Chirp projection registration, group
chat/discovery surfaces, wallet feature wiring, and the home feed key.

This is the practical version of the boundary: shared defaults are reusable
composition; app crates decide which reusable pieces are actually part of that
product.

## Status Boundary

Crate-boundary docs can say what the boundary is. They should not be the current
source for which migration step is merged, in CI, or blocked. That state belongs
in the temporal trackers while it is live and is removed once it is no longer
needed.

When a design doc still describes an old migration location, follow the
current code and the canonical boundary. For example, planner implementation
now lives in `crates/nmp-planner`; code lookup should not stop at older
references to `nmp-core/src/planner`.

## See Also

- [[temporal-plans-vs-durable-docs|Temporal Plans vs Durable Docs]] ([Temporal Plans vs Durable Docs](../concepts/temporal-plans-vs-durable-docs.md))
- [[source-authority-map|Source Authority Map]] ([Source Authority Map](../references/source-authority-map.md))
- [[app-composition-and-chirp-wiring|App Composition and Chirp Wiring]] ([App Composition and Chirp Wiring](app-composition-and-chirp-wiring.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
- [App Composition and Chirp Wiring Sources](../../raw/repos/2026-05-28-app-composition-and-chirp-sources.md)
