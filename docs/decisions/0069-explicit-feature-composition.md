# ADR-0069: Explicit feature composition and app-owned product policy

## Status

Accepted for the architecture redesign direction.

## Context

Issue #2313 exposed that NMP's app-facing model had become too hard to read:
production apps could call `register_defaults()` and receive an opaque bundle of
protocol features, runtimes, projections, and policy. Issue #2316 showed that
this is not just a naming problem. One feature's state is split across many
independent mechanisms, so hiding more work behind a larger preset would only
make the architecture less inspectable.

The old defaults-era ADRs still preserve useful invariants: reusable substrate
installation, composition observability, extension seams, and runtime builders.
What they no longer own is the production app architecture. A real app root must
show which substrate, protocol features, app features, capability needs, and
policy owners it installs.

## Decision

Production NMP apps use explicit feature composition.

An app Rust composition root installs:

- substrate: actor, store, planner, signer ports, capability registry, and typed
  update delivery;
- reusable Nostr protocol features such as follows, lists, search, groups, DMs,
  refs, routing, and publish helpers;
- app-owned product features such as Highlighter capture, Podcast playback, or a
  gallery showcase;
- typed outputs/status the shell may render or cache mechanically;
- capability contracts that describe raw OS/browser work the shell may execute;
- one app/client identity used by outbound finalization and transport metadata.

The composition root is the only place where an app chooses product policy. It
is the answer to "what does this app do?" A maintainer should be able to read
that root and see the installed protocol features, app features, read sessions,
write builders, shell capabilities, and product defaults without reverse
engineering a preset crate.

Typical construction is:

```text
create NMP app builder
  -> install substrate/runtime pieces
  -> install selected reusable Nostr protocol features
  -> install app-owned product features
  -> install shell capability contracts
  -> start actor/runtime
```

The exact Rust API can change. The invariant is that production apps compose
named pieces explicitly, and app-specific nouns stay in app crates unless they
are a reusable Nostr mechanism.

`nmp-defaults` may survive as a reusable installer library. It must not be a
hidden production preset or a leaf-app policy owner. It may provide substrate,
generic routing, mailbox, parser, signer, and publish installers. It must not
own seed follows, bootstrap relay brands, signer permission defaults, onboarding
policy, app relay policy, or product defaults.

`register_defaults()` is rejected as production app architecture. If a preset
survives, it is tutorial, test, or migration compatibility with named live
consumers, owner, support window, and deletion or formalization trigger.

App-specific behavior belongs in app Rust crates unless it is a reusable Nostr
mechanism. A request from one downstream app is evidence, not permission to add
app-named helpers or product policy to NMP crates.

## Consequences

Positive:

- Reading the composition root tells an app developer what is installed.
- NMP crates can shrink toward reusable protocol/runtime mechanisms.
- App policy does not leak into native shells or framework defaults.
- Composition ledgers remain useful because they report an explicit root, not a
  magic preset.

Negative/tradeoffs:

- Existing templates, examples, builder-guide pages, and downstream roots that
  teach `register_defaults()` as the production path must migrate or be labeled
  as compatibility/tutorial surfaces.
- Some installer APIs may need to become more granular before the explicit root
  is pleasant.
- A production app may initially write more visible setup code, but that setup
  replaces hidden behavior.
- Existing downstream apps may need a transition pass where each `register_*`
  call is classified as substrate, reusable protocol feature, app feature,
  capability, or compatibility.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Keep `register_defaults()` as the normal app root | It hides active protocol features and policy, preserving the #2313 confusion. |
| Add a broad `dyn AppFeature` or global `AppHost` method pile first | It risks another abstraction layer without deleting old public surface. Existing builders, registrars, and installer functions must be tried first. |
| Move common downstream product behavior into NMP crates | It violates the generic Nostr mechanism vs. app domain boundary. |
| Make shells configure raw feature dictionaries | It moves protocol and product policy out of Rust. |

## Fitness functions / enforcement

- Production scaffold docs and tests reject hidden `register_defaults()` and
  `declare_consumed_projections` teaching paths.
- `nmp-defaults` has no platform-runtime dependency and no leaf-app product
  policy.
- Every compatibility preset has live consumers, owner, support window, and
  deletion/formalization gate.
- Downstream app roots keep app-specific policy in app Rust crates; native/web
  shells render, execute capabilities, and hold only ephemeral presentation
  state.

## Linked work

- #2313: app-developer API complexity.
- #2316: fragmented feature-state lifecycle.
- #2320: ADR reset and stale source-of-truth cleanup.
- Amends ADR-0009, ADR-0046, ADR-0049, ADR-0067, and ADR-0068 where they taught
  defaults-era production composition.
