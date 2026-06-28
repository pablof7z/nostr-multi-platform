# Typed Read Sessions

A screen opens a typed read session for the state it wants to render:

```text
open(HomeFeed { account })
open(Profile { pubkey })
open(GroupFeed { group_id, host_relay })
open(Search { query })
open(ProfileRef { pubkey, owner })
open(EventEmbed { event_ref, owner })
open(LiveCount { source, filter, owner })
```

These are examples, not final API names. The important boundary is that the
session is defined by Rust feature code. The shell opens and closes it, then
renders typed output frames.

## What A Session Owns

A real session owns the whole lifecycle for one rendered state:

- acquisition demand;
- relay planning or protocol relay pinning;
- cache/store replay before live delivery;
- live event admission;
- dynamic source changes;
- typed output schema and owner;
- status and failure state;
- teardown when the owner closes.

If a feature author still has to wire raw interest, replay, observer, admission,
projection sidecar, tick observer, and close token separately, the architecture
has only renamed the old problem.

## What The Shell Sees

The shell should see a small handle and typed output:

```text
SessionHandle {
  query_key,
  owner_id,
  output_key,
  scope,
}
```

Swift, Kotlin, TypeScript, or TUI code can open the handle, render frames, send
typed actions such as "load more", and close the handle. It should not compute
follow-set expansion, replay ordering, relay provenance, profile hydration, or
event admission.

## Relationship To Existing Primitives

`open_interest` is acquisition-only. It can fetch events without making them a
screen-owned output lifecycle, so it should become internal, diagnostic, test,
export, or migration-scoped. Product screens should use typed sessions.

`ObservedProjection` contains useful invariants: scoped admission, replay before
live, typed output ownership, and teardown. The destination is not to preserve
every existing projection noun. The destination is to reuse or narrow the good
invariants under a smaller typed-session door.

`ReducedSource` describes source dependency reduction: for example, a home feed
whose authors come from the user's follows, or whose events come from follows'
reactions and comments. That reduction should be feature-owned session logic,
not host-owned subscription plumbing.

## Home Feed Example

A microblog app can define a `HomeFeed` session whose source expression means:

```text
kind 30023 events from followed authors
OR kind 30023 events reacted to or commented on by followed authors
```

The app Rust feature owns ranking, mute policy, source expansion, load-more
behavior, and output rows. NMP reusable features provide NIP-02 follows,
references, reactions/comments, routing, storage, and projection machinery.

The host code should only open the home session and render `HomeFeedOutput`.

## Push/Pull Boundary

Typed sessions are the UI-state path: pushed, bounded, Rust-owned outputs.

Raw event logs and pull cursors remain useful for diagnostics, import/export,
history inspection, tests, and migration tools. They should not be the normal
way a product screen reconstructs live UI state.
