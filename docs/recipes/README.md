# NMP Recipe Book

These recipes are durable how-to material, not a roadmap. Active work and
release ordering stay in GitHub Issues.

Start each recipe by naming the owner of every fact:

- reusable NMP crates own generic Nostr mechanisms;
- app Rust cores own app product nouns, policies, and product projections;
- platform shells render, execute OS capabilities, and hold ephemeral UI state;
- runtime and component-host code transport snapshots, refs, and typed actions.

If a recipe would make Swift, Kotlin, TypeScript, or a component parse raw
Nostr events, compute relay policy, or mirror a Rust-owned cache, the recipe is
wrong.

## Recipes

| Page | Use it for |
|---|---|
| [Common app shapes](app-shapes.md) | Timeline viewers, kind explorers, long-form readers, rich app-owned projections, component-host wiring, and browser signer caveats. |
| [Nostr content rendering](content-rendering.md) | Installing content components and overriding renderers through registry seams. |

## Cross-links

- [Browser signer/private-flow capability model](../wasm-surface.md#browser-signerprivate-flow-capability-model)
- [NIP support matrix](../nips.md)
- [Concept-owned active reads](../builder-guide/28-action-triggered-subscriptions.md)
