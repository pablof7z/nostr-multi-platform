# ADR-0036: Active Follow Source Reconciliation

## Status

Folded into ADR-0070 for typed read sessions.

## Context

App feeds need a live source such as "the active account's follows" without
moving social-feed policy into `nmp-core` or native shells. The original ADR
introduced a `ReducedSource`-style model so the framework could recompile
materialized interests when the active account, contact list, mute/block state,
or replacement events changed.

That invariant still matters. What changes under ADR-0070 is the public shape:
`ReducedSource` is not app-facing architecture vocabulary. It is private source
reconciliation machinery behind a typed read session unless a later ADR proves a
specific public need.

## Current Decision

The active follow set has one Rust owner. Product reads may depend on that owner
through a typed session descriptor such as:

```text
read long-form articles
  where authors are active-account follows
  plus articles those follows reacted to or commented on
```

The session compiler turns that descriptor into concrete acquisition demand,
route policy, replay, admission, and output. Native shells never pass a static
copy of follows, expand source sets, or re-subscribe when follow state changes.

Dynamic source reconciliation is fail-closed. If the source is empty,
unavailable, or revoked, the session emits an empty/blocked typed state unless
the session explicitly declares a fallback source.

## Consequences

- The follow-set source remains a reusable input to feeds, refs, search, groups,
  and other protocol reads.
- `nmp-core` still does not own a built-in "home feed" product shape.
- App Rust crates own primary content kinds, ranking, admission, and app policy.
- Native/web shells render typed output and execute capabilities only.
- `ReducedSource`-named APIs should move inward or be classified as migration
  surfaces with an owner and deletion/formalization trigger.

## Fitness Functions

- No product shell expands follow/list/source sets itself.
- Session tests cover source arrival, source replacement, source withdrawal,
  account switch, and empty-source fail-closed behavior.
- New source families must prove shared semantics before introducing a public
  abstraction.
