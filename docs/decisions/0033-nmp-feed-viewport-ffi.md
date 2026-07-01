# ADR-0033 - NMP Feed Viewport FFI

Status: accepted

Date: 2026-05-26

## Context

Chirp is a showcase app. It should prove that an app can render a Nostr feed
without owning feed mechanics. A previous shape exposed
`nmp_app_chirp_snapshot_window(handle, request_json)`, which made the shells
construct feed-window requests with limits and optional cursors. That kept the
cursor comparison code in Rust, but it still leaked the feed protocol into the
showcase app boundary.

The reusable concern is not "Chirp timeline paging"; it is "an NMP app renders
a bounded Nostr feed and reports that the visible tail was reached."

## Decision

Add a reusable `nmp-feed` crate. It owns:

- stable `(created_at, id)` cursors;
- newest-first block ordering;
- bounded current-window state;
- stateless cursor pages for Rust consumers that need them;
- default page size and max cap;
- transitive inclusion of referenced event cards;
- `make_window_us` observability;
- a keyed feed-controller registry.

Add exactly one generic C ABI symbol:

```c
void loadOlderFeed(const char *feed_key);
```

`nmp-app-chirp` registers the reusable OP-centric feed under an app-owned
snapshot-projection key such as `"chirp.timeline.home"`. iOS and the TUI read
that value from the normal NMP update stream:

```json
{ "blocks": [...], "cards": [...], "page": {...}, "metrics": {...} }
```

When the rendered tail becomes visible, shells call
`loadOlderFeed("chirp.timeline.home")`. They do not construct cursor
requests, do not know page-size or cap constants, and do not call a
Chirp-specific feed read API.

`load_older` is measured by rendered progress, not by raw acquisition-page
consumption. A feed controller may scan over deleted, muted, blocked,
superseded, replaced, or app-filtered rows while advancing its internal cursor.
Those invisible rows do not satisfy the user action; the controller keeps
pulling until the visible window grows or the current perspective is exhausted.

`nmp-note-feed` owns the concrete note-feed rows that compose kind:1/NIP-10
facts, repost facts, content trees, and feed-window mechanics. `nmp-nip01`
remains the lower-level kind:1/NIP-10 fact owner. The reusable traversal,
dedupe, cursor, and viewport policy live in `nmp-feed`.

## Consequences

- The C ABI grows by one generic feed viewport intent, not a Chirp-specific
  timeline/window protocol.
- Chirp consumes a standard feed projection and stays a showcase app.
- The former C viewport symbol added by this ADR was deleted after the UniFFI
  native session API became the public native path.
