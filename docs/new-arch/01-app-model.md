# App Model

An NMP app has a Rust composition root. That root installs the substrate and
then opts into explicit reusable NMP features and app-owned product features.

Illustrative shape:

```rust
let mut app = runtime_builder.storage_path(policy.storage).build_core();

nmp_defaults::register_substrate(&mut app, policy.substrate);
nmp_nip02::install_follow_list(&mut app);
nmp_nip51::install_lists(&mut app);
nmp_nip50::install_search(&mut app, policy.search);
nmp_nip29::install_groups(&mut app, policy.groups);
microblog_app::install_home_feed(&mut app, policy.home_feed);
```

The names are placeholders. The rule is not: production apps should show the
features and policy they install. A hidden `register_defaults()` preset is not
the desired production architecture.

## What Goes Where

NMP crates provide reusable Nostr mechanisms:

- relay routing and provenance;
- signing and signer continuations;
- event storage and bounded replay;
- NIP implementations;
- typed read sessions;
- typed action and publish machinery;
- generic projections, refs, search, threads, counts, and protocol parsers.

App Rust crates provide product behavior:

- home-feed ranking or fallback policy;
- Highlighter capture/OCR/share queues;
- podcast playback, downloads, chapters, and agents;
- gallery showcase state;
- any domain logic a different Nostr app would not reuse.

Native and web shells render, execute capabilities, and hold ephemeral
presentation state. They do not own routing, retry, publish status, protocol
tagging, durable caches, signer policy, or product-correct derived facts.

## Feature Installation

A feature installer can register:

- typed read session descriptors;
- typed outputs and reducers;
- typed actions;
- event draft builders;
- protocol parsers and validators;
- capability needs;
- publish route policy owned by that protocol or app feature.

Feature installers should be explicit functions or narrow builder extensions.
Avoid a broad public `AppFeature` object or a method pile on `AppHost` unless a
concrete slice proves existing seams cannot express the feature without keeping
larger old surface.

## Developer Experience

The desired app-authoring loop is:

1. Define Rust-owned state and output types.
2. Define typed read sessions for screens or widgets.
3. Define typed actions and event draft builders for user intent.
4. Install reusable NMP features and app-owned product features in the root.
5. Let generated or contract-tested host adapters open sessions, dispatch
   actions, render outputs, and report raw capability results.

The host experience can feel like a simple `open(HomeFeed(...))`, but that
door is backed by Rust-owned feature definitions, not arbitrary shell-authored
relay subscriptions.

## Composition Gates

The composition root should make these facts visible:

- storage and substrate policy;
- installed protocol features;
- installed app features;
- output contracts exposed to hosts;
- capabilities the shell must provide;
- app identity and outbound client-tag policy.

The first implementation should reuse the existing runtime builders, `AppHost`
composition seam, and narrow registrars before adding a new framework-wide
composition abstraction.
