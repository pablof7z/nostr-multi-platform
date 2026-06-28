# App Model

An app has a Rust composition root. The composition root installs the substrate
and then opts into named reusable features:

```rust
pub fn register(app: &mut impl AppHost, policy: AppPolicy) {
    nmp_defaults::register_substrate(app, policy.substrate);

    nmp_defaults::features::nip02_follow_list(app);
    nmp_defaults::features::nip51_lists(app);
    nmp_defaults::features::nip50_search(app, policy.search);
    nmp_defaults::features::nip29_groups(app, policy.groups);
    nmp_defaults::features::home_feed(app, policy.home_feed);

    my_app::register(app, policy.app);
}
```

The exact names above are illustrative. The rule is not illustrative:
`register_defaults()` should not be the taught mental model for real apps. It
may remain a convenience preset, but product apps should show the features they
install.

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

App-specific product policy stays in the app's Rust crate. For example, NIP-29
group routing belongs in an NMP feature because many Nostr apps need it. A
podcast app's playback queue belongs in that app's Rust crate because it is not
a reusable Nostr primitive.

## Developer Mental Model

An app developer should know:

- which features the app installs;
- which live query a screen opens;
- which typed projection that screen renders;
- which draft builder constructs an event;
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
