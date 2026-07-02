# ADR-0069: Explicit feature composition and app-owned product policy

## Decision

Production NMP app roots compose named owners directly. An app Rust composition
root installs the substrate, selected reusable protocol features, app-owned
product features, shell capability contracts, typed outputs, and the app/client
identity used by outbound finalization and transport metadata.

Hidden default presets are not production architecture. A maintainer should be
able to read the composition root and see which substrate pieces, protocol
installers, app features, capability contracts, read helpers, write builders,
and product policy knobs the app uses.

Protocol crates expose named installers with explicit configuration and returned
handles when the crate owns runtime or projection state. The uniform installer
shape is:

```rust
pub struct Config {
    // Explicit app policy knobs; empty when the crate has none.
}

pub struct Handles {
    // Runtime or projection handles the app may retain; empty when none exist.
}

pub fn register(
    app: &mut (impl RequiredNarrowRegistrarTraits),
    config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError>;
```

Reusable protocol installers take only the narrow registrar traits they need.
They do not take a broad app host trait to smuggle unrelated policy into the
framework.

Protocol crates may delegate from `register` to crate-private seam modules.
Those helpers are implementation detail, not app composition APIs, and must not
be re-exported as protocol installers.

App-specific nouns live in app Rust crates unless they are reusable Nostr
mechanisms. A downstream need is evidence for a generic mechanism; it is not
permission to add app policy or product defaults to shared NMP crates.

## Context

NMP app setup used to hide protocol features, projection wiring, runtime pieces,
and product policy behind broad bundles. That made it hard to inspect what an
app actually did and encouraged app policy to leak into shared crates.

Explicit composition keeps the architecture readable: substrate construction is
shared, protocol features stay reusable, app Rust owns product meaning, and
native/web shells stay thin.

## Consequences

The composition root is more visible than a magic preset, but that visibility is
the point. Extra setup code is preferable to hidden relay brands, seed follows,
signer permission defaults, onboarding policy, app relay policy, or product
defaults.

Installer APIs may need small typed config and handle structs even when they are
empty. This keeps future policy explicit without inventing a new preset layer.

## Boundaries

Permitted:

- substrate construction through `nmp-substrate`;
- reusable protocol installers in protocol crates;
- app-owned installers in app Rust crates;
- explicit policy knobs in installer `Config` values;
- returned runtime/projection handles in installer `Handles` values.

Forbidden:

- hidden production presets;
- compatibility bundles that silently install product policy;
- app-named helpers in shared crates when the concept is not a reusable Nostr
  mechanism;
- broad app host bounds in reusable protocol installers;
- native or web shells choosing protocol policy to avoid Rust composition.

## Enforcement

Reviewers check production roots, templates, and builder docs for explicit
installer calls and app-owned policy. Doctrine gates reject reintroduced default
bundle vocabulary and starter templates that teach hidden composition.

Protocol installer linting checks that scoped protocol crates expose one
canonical `register` entry point and do not grow public split installers such as
`register_actions`, `register_runtime`, or `register_*_scopes`.

## Related

- [ADR-0070](0070-typed-read-sessions.md) - typed read sessions.
- [ADR-0071](0071-write-intents-and-route-provenance.md) - write ownership and
  route provenance.
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md) - runtime and shell
  ownership.
- [docs/architecture/crate-boundaries.md](../architecture/crate-boundaries.md)
  - crate ownership and layering.
- [docs/builder-guide/20-new-protocol-module.md](../builder-guide/20-new-protocol-module.md)
  - protocol module installer guidance.
- #2724 - protocol installer uniformity.
- #2746 - ADR current-only cleanup.
