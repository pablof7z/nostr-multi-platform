# App Model

An app has a Rust composition root. The composition root installs the substrate
and then opts into named reusable NMP features and app-owned product features:

```rust
pub fn register(app: &mut impl AppHost, policy: AppPolicy) {
    nmp_defaults::register_substrate(app, policy.substrate);

    nmp_defaults::features::nip02_follow_list(app);
    nmp_defaults::features::nip51_lists(app);
    nmp_defaults::features::nip50_search(app, policy.search);
    nmp_defaults::features::nip29_groups(app, policy.groups);
    nmp_defaults::features::home_feed(app, policy.home_feed);

    highlighter_app::features::register(app, policy.highlighter);
    podcast_app::features::register(app, policy.podcast);
}
```

The exact names above are illustrative. The rule is not illustrative:
`register_defaults()` should not be the taught mental model for real apps. It
may remain a convenience preset, but product apps should show the features they
install.

The substrate is the correctness floor: actor, store, indexes, signer ports,
capabilities, planner, publish engine, and typed update delivery. Feature bundles
sit on top of it.

## Feature Bundles

A feature bundle is not an open screen. It installs reusable capability:

- read/query descriptors;
- typed projection producers;
- actions and reducers;
- event draft builders;
- parsers and protocol validation;
- capability needs;
- protocol-owned state;
- publish route policy where the protocol owns it.

NMP feature bundles provide reusable Nostr mechanisms: NIP-02 follows, NIP-29
groups, NIP-17 DMs, NIP-65 routing, profile refs, event refs, search, generic
thread reduction, generic live counts, and publish policy.

App feature bundles provide product behavior that is not reusable Nostr
infrastructure: Highlighter capture/OCR/share queues/article chrome, podcast
playback/downloads/feed fetching/transcripts/agents, or gallery showcase
catalog state. They use NMP features, but they do not become NMP crates unless
the mechanism is useful to other Nostr apps.

The app crate is also the right owner for cross-protocol product composition.
For example, publishing a highlight and then sharing it into a NIP-29 room is
Highlighter behavior. The highlight feature and NIP-29 feature do not need to
import each other's domain types.

## Composition Gates

The composition root should make these gates explicit:

- `substrate`: actor, store, planner, signer ports, capability registry, and
  update delivery.
- `protocol features`: reusable Nostr read/write/query/publish behavior.
- `app features`: product state, product actions, app-owned projections, and
  capability needs.
- `output contract`: typed outputs the shell may render or cache for rendering.
- `capability contract`: native/web capabilities Rust may request and the raw
  results the shell reports back.

This keeps framework defaults useful without hiding the product architecture.
Feature-install helpers should live in feature composition crates such as
defaults, runtimes, protocol crates, or app crates. They should not become a
pile of unrelated methods on `nmp-core::AppHost`.

Composition should be idempotent where practical and explicit across browser,
native, TUI, and test roots. A browser `start()` path should not silently install
a different product architecture from the native root.

## Developer Mental Model

An app developer should know:

- which features the app installs;
- which feature/ref session a screen, component, widget, or service opens;
- which typed output that owner renders;
- which draft builder or action builder constructs an event or product action;
- which signer should sign when the active account is not enough;
- whether publishing uses automatic routing, protocol-pinned routing, or an
  explicit relay override;
- when to close query handles.

An app developer should not need to know:

- projection tiers;
- `SnapshotRegistry`;
- muted observers;
- replay shapes;
- raw relay fanout;
- NIP-65 mailbox lookup internals;
- cache/store replay mechanics;
- FlatBuffers sidecar registration;
- snapshot tick reconcilers;
- publish retry classification;
- native-side relay routing.

## Shell Responsibilities

Native and web shells have three jobs:

- render the typed state Rust gives them;
- execute capabilities requested by Rust;
- hold ephemeral presentation state such as focus, animation, scroll affordance,
  or transient sheet state.

The discriminating test stays simple: if a second platform would have to
reimplement the behavior to stay correct, the behavior belongs in Rust.

Projection caches generated for Swift, Kotlin, TypeScript, C, or TUI are not
product state. They are render adapters for typed Rust outputs. A shell may keep
row-delta caches for profiles, event refs, playback rows, or domain slices when
the adapter owns their lifecycle and Rust remains the durable source of truth.

## Downstream App Shape

`nmp-gallery` mostly exercises NMP features directly. It needs component-scoped
profile and event refs, generated ref caches, embed resolution, and no shell
retry timers.

Highlighter needs NMP features plus Highlighter-owned bundles for capture,
share, article reading, curation, room chrome, comments, feedback, and podcast
surfaces. Its native shells should execute Keychain, OCR, share extension,
camera, AVPlayer, NIP-55, and connectivity capabilities, then report raw
results to Rust.

Podcast Player needs NMP features plus podcast-owned bundles for playback,
queue, downloads, feed subscription, OPML/import, transcripts, widgets,
Blossom-backed publish flows, and agent behavior. RSS, playback, queue, and
transcript logic must stay in the app Rust crate.
