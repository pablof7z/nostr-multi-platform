# Kind-Specific Event Wrappers

Typed event helpers are allowed when they keep protocol parsing out of apps
without moving app nouns into `nmp-core`.

## Rule

Each protocol or app crate owns the typed helpers for the event kinds it owns:

- protocol crates own NIP-defined wire formats;
- app Rust crates own app-specific or experimental wire formats;
- `nmp-core` owns storage, routing, action dispatch, capabilities, and snapshots,
  but not kind-specific product records.

There is no central kind-wrapper crate and no kernel registry that imports every
known kind. Apps opt in by depending on the protocol or app crate that owns the
kind.

## Read Side

A read-side helper is a pure decoder over a raw or stored event:

```rust
pub fn try_from_event(event: &StoredEvent) -> Option<ArticleRecord>;
```

The decoder validates the kind, normalizes tags into canonical fields, and
returns an immutable record. It does not perform network I/O, read process
state, mutate tags, or cache derived facts behind mutable accessors.

Read models and projections call these helpers at their owning boundary. A feed
or timeline still opens through the normal interest and projection seams; the
helper only turns an event into the crate-owned record shape.

## Write Side

A write-side helper is a pure builder that produces an unsigned event:

```rust
pub struct ArticleBuilder { /* fields omitted */ }

impl ArticleBuilder {
    pub fn into_unsigned(self, author: &str, created_at: u64) -> UnsignedEvent;
}
```

Signing, publish routing, retry policy, relay selection, and capability results
remain on the existing action and publish paths. A builder must not pick relays,
call a signer, inspect local accounts, or perform I/O.

## Ownership

Protocol crates define helpers only for their protocol surface. For example:

- `nmp-nip01` owns short-note/profile primitives;
- `nmp-nip17` owns gift-wrap and DM protocol helpers;
- `nmp-nip29` owns group records and group event builders;
- media or article helpers belong in their NIP crates when a consuming app needs
  them.

Application-specific kinds stay in the application Rust core. A request from one
app is evidence that a reusable helper might be useful, not permission to add an
app-named API to NMP.

## Forbidden Shapes

- Mutable wrapper classes around shared event state.
- Setters that rewrite tag arrays.
- Async getters or decoders that fetch missing data.
- A central `wrap_event` function that imports every protocol crate.
- A shared `nmp-kinds` crate whose job is to collect unrelated product records.
- Kernel APIs named after product kinds or app workflows.

## Adding A New Helper

1. Put the decoder/builder in the crate that owns the wire format.
2. Add focused tests for valid events, missing required tags, malformed tags,
   and builder output.
3. Keep the public API protocol-shaped, not app-shaped.
4. Expose the resulting record through the app's existing projection or action
   surface only when an app actually consumes it.

The intended benefit is typed parsing at the boundary. The architecture still
has one writer per fact, Rust-owned domain behavior, and native shells that only
render snapshots or execute capabilities.
