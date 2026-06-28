# ADR 0009: App Extension Kernel Boundary

**Date:** 2026-05-17
**Status:** accepted; amended by ADR-0069 and ADR-0072
**Current design:** `docs/design/app-extension-kernel.md`
**Companion ADR:** ADR-0010

**Current disposition:** This ADR still owns the core boundary: `nmp-core`
contains reusable Nostr substrate, app/product logic belongs in app Rust crates,
and shells render plus execute capabilities. ADR-0069 replaces the
defaults-era composition wording below: production apps use explicit feature
composition, and `nmp-defaults::register_defaults` is not the production app
architecture.

## Context

NMP must support apps whose product nouns differ: social feeds, highlighter
artifacts, podcast episodes, TENEX workspaces, daily plans, and other
app-specific concepts. Putting those nouns in `nmp-core` would make the core a
product dump and would force every platform to carry unrelated app behavior.

The opposite failure is pushing product logic into Swift, Kotlin, or browser
shells. That violates the NMP doctrine that Rust owns domain correctness and
native shells render state plus execute capabilities.

## Decision

`nmp-core` owns generic Nostr application infrastructure only:

- actor runtime and reducer-owned state,
- verified event ingest and storage,
- subscription compilation and relay routing,
- publish orchestration,
- signer/session plumbing,
- action registration and dispatch,
- capability request/result plumbing,
- snapshot and typed-projection emission,
- diagnostics and doctrine gates.

Reusable Nostr concepts live in protocol/substrate crates. App-specific
concepts live in app Rust crates. Native shells render and report native facts.

The shipped extension seams are:

- `ActionModule` plus `register_action` for write intents,
- `open_observed_projection` for event-driven Rust projections that declare
  shape, scope, owner, and replay before receiving events,
- `register_typed_snapshot_projection` for host
  state,
- `CapabilityModule` and capability sockets for native facts,
- `AppHost` and `nmp-defaults::register_defaults` for composition, plus
  platform runtime builders such as `nmp-native-runtime::NmpAppBuilder`.

If implementing an app requires adding that app's nouns to `nmp-core`, the
boundary is wrong. Either add a reusable Nostr mechanism in an NMP crate or add
the product concept to the app's Rust core.

## Layer Ownership

| Layer | Owns | May contain app nouns? |
|---|---|---|
| `nmp-core` | actor, store, planner, routing, publish, action/capability/projection substrate | No |
| Protocol crates | reusable Nostr mechanisms and protocol nouns | Protocol nouns only |
| App Rust crates | app records, policies, projections, workflows | Yes |
| Native shells | rendering, OS handles, ephemeral presentation state | UI labels only |

## Consequences

- The kernel stays reusable and app-agnostic.
- Social-client behavior is implemented through protocol/defaults/app modules,
  not as hard-coded kernel view kinds.
- External consumers compose NMP through `nmp-defaults` and a platform runtime
  builder, then add their own Rust-owned modules.
- Future apps prove the boundary by adding app crates or protocol crates, not by
  growing `nmp-core`.

## Rejected Alternatives

- Put social-client view kinds and app actions directly in `nmp-core`.
- Let native shells own app policy to avoid Rust app crates.
- Create app-specific cargo features in a monolithic framework crate.
- Add type-erased host surfaces that bypass typed action/projection ownership.
