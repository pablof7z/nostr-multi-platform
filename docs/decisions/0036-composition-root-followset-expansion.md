# ADR-0036 - Active follow-set source and follow-feed expansion

Status: accepted

Date: 2026-05-28

## Context

App feeds need two different facts from the active account's follow set:

1. a live membership predicate so a projection can decide whether an author's
   event or reference is relevant to the current active user;
2. a reactive perspective so a feed declaration can acquire primary content
   kinds from the active user's current follows and recompile when the list
   changes.

The framework must not encode a Chirp-specific "social timeline" shape in the
planner or in `nmp-feed`. A media app, a long-form app, or a relay-set app must
be able to use the same machinery with different primary content kinds and
different admission/ranking rules.

## Decision

The active follow set has one producer. Feed declarations, not a special
kernel-owned home-feed API, consume it:

- `nmp-nip02::ActiveFollowSet` produces a reactive pubkey set and a closure
  predicate.
- app/defaults composition declares a feed as primary content kinds from a
  perspective such as "the active account's follows".
- `nmp-core` executes the resulting active-account follow interest and rewires
  it when the active account, kind:3, mute/block, delete, or replacement inputs
  change. It does not own a built-in home-feed product shape or primary
  kind policy.

The composition root wires consumers and declares the feed. It does not pass a
static follow list snapshot to the kernel or to native code.

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

## Feed Declaration

The app/defaults layer declares a feed as:

- primary content kinds;
- a reactive perspective, for this ADR the active account's follows;
- the feed engine, admission/ranking policy, and projection key that will render
  the resulting events.

The acquisition kinds are app-declared primary content kinds transformed by the
relevant protocol adapter before they reach the kernel. A Chirp notes feed
declares primary kind `[1]`; the NIP-18/NIP-01 adapter may derive kind `6`
wrapper acquisition. A non-kind-1 feed declares its primary kind, and generic
repost wrapper acquisition is kind `16`. The app never declares wrapper kinds as
primary feed content.

Admission, ranking, and sorting are also composition-owned. A WoT preset,
relay-set rule, mute/block rule, or app-defined quality function can change
which already-acquired rows render and how they are ordered without becoming a
new kernel feed kind. Changing any of those rules is a perspective change: the
feed window resets and regrows from the current store/pull contract.

The app must not pass a static copy of "the current user's follows" to native
or to the kernel. It selects a reactive source such as active-user follows.
The active-follow producer and kernel subscription machinery react to kind `3`,
list, account, mute/block, delete, and replacement changes and reset/regrow the
declared feed without UI code re-declaring it.

## Composition Root

`nmp-defaults::register_op_feed_defaults` and app-specific registration code
wire:

- the `ActiveFollowSet` observer;
- the active-follows declared feed and its primary content kinds;
- the feed engine's membership predicate, admission/ranking policy, and reset
  hook;
- event lookup and claim/release closures;
- identity-change reset hooks;
- typed projection registration.

They do not also open a separate home-feed API or pass concrete follow
pubkeys. Duplicating the active-follows declaration would violate D4 and produce
duplicate wire subscriptions.

## Consequences

- `nmp-feed` remains a mechanics crate: windows, controllers, cursors,
  provenance containers, and bounded paging.
- `nmp-core` stores and executes the adapter-derived acquisition shape declared
  by apps/defaults, but owns neither app feed semantics nor primary-kind,
  admission, ranking, or sorting policy.
- protocol crates derive wrapper kinds and target provenance.
- app Rust crates choose feed keys, primary kinds, source expressions,
  admission/ranking policy, and row projections.
- native shells render snapshots and execute capabilities only.

This document is the current rule. If the implementation changes, edit this
document in place instead of adding a later correction document.
