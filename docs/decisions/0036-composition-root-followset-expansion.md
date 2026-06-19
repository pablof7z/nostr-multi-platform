# ADR-0036 - Active follow-set source and follow-feed expansion

Status: accepted

Date: 2026-05-28

## Context

App feeds need two different facts from the active account's follow set:

1. a live membership predicate so a projection can decide whether an author's
   event or reference is relevant to the current active user;
2. an acquisition shape so the kernel can subscribe to the active user's
   current follow set and recompile that subscription when the list changes.

The framework must not encode a Chirp-specific "social timeline" shape in the
planner or in `nmp-feed`. A media app, a long-form app, or a relay-set app must
be able to use the same machinery with different primary content kinds and
different admission/ranking rules.

## Decision

The active follow set has one producer and one acquisition owner:

- `nmp-nip02::ActiveFollowSet` produces a reactive pubkey set and a closure
  predicate.
- `nmp-core::Kernel::sync_follow_feed_interests` owns the active-user
  follow-feed acquisition interest.

The composition root wires consumers. It does not duplicate the kernel's
follow-to-interest expansion.

## Producer

`ActiveFollowSet` owns an `Arc<RwLock<BTreeSet<String>>>` of the active
account's follows plus the active account's own pubkey. It keeps the set
current by observing kind `3` ingest and by receiving explicit account-change
notifications from the app composition root.

Its public surface is closure-shaped:

```rust
impl ActiveFollowSet {
    pub fn new(active_pubkey: ActiveAccountSlot) -> Arc<Self>;
    pub fn follows(&self) -> Vec<String>;
    pub fn predicate(&self) -> Arc<dyn Fn(&str) -> bool + Send + Sync>;
    pub fn on_change(&self, callback: Box<dyn Fn() + Send + Sync>);
    pub fn notify_account_changed(&self);
}
```

`predicate()` captures the shared set. A predicate handed to a feed engine
before a kind `3` update observes the new membership after that update without
re-registration.

`ActiveFollowSet::new` takes `ActiveAccountSlot`, not `&NmpApp`, so
`nmp-nip02` does not depend on `nmp-ffi`.

## Acquisition Owner

The kernel owns active-user follow-feed subscription state. On active-account
kind `3` changes, account switches, and contact-feed reopens,
`sync_follow_feed_interests` withdraws the old interest and registers a single
multi-author `LogicalInterest` for the active user plus their current follows.

The acquisition kinds are app-declared primary content kinds transformed by
the relevant protocol adapter before they reach the kernel. A Chirp notes feed
declares primary kind `[1]`; the NIP-18/NIP-01 adapter may derive kind `6`
wrapper acquisition. A non-kind-1 feed declares its primary kind, and generic
repost wrapper acquisition is kind `16`.

The app must not pass a static copy of "the current user's follows" to native
or to the kernel. It selects a reactive source such as active-user follows.
The kernel and protocol modules react to kind `3`, list, account, mute/block,
and replacement changes.

## Composition Root

`nmp-defaults::register_op_feed_defaults` and app-specific registration code
wire:

- the `ActiveFollowSet` observer;
- the feed engine's membership predicate;
- event lookup and claim/release closures;
- identity-change reset hooks;
- typed projection registration.

They do not register duplicate follow-feed REQs. Duplicating the kernel's
follow-feed interest would violate D4 and produce duplicate wire
subscriptions.

## Consequences

- `nmp-feed` remains a mechanics crate: windows, controllers, cursors,
  provenance containers, and bounded paging.
- `nmp-core` owns active-user follow acquisition, but not app feed semantics.
- protocol crates derive wrapper kinds and target provenance.
- app Rust crates choose feed keys, primary kinds, source expressions,
  admission/ranking policy, and row projections.
- native shells render snapshots and execute capabilities only.

This document is the current rule. If the implementation changes, edit this
document in place instead of adding a later correction document.
