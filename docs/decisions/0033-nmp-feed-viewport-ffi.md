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
void nmp_app_load_older_feed(void *app, const char *feed_key);
```

`nmp-app-chirp` registers the reusable home feed under the snapshot-projection
key `"nmp.feed.home"`. iOS and the TUI read that value from the normal NMP
update stream:

```json
{ "blocks": [...], "cards": [...], "page": {...}, "metrics": {...} }
```

When the rendered tail becomes visible, shells call
`nmp_app_load_older_feed(app, "nmp.feed.home")`. They do not construct cursor
requests, do not know page-size or cap constants, and do not call a
Chirp-specific feed read API.

`nmp-nip01` remains the protocol projection that knows how to build
`TimelineEventCard` values and extract quoted-event references from their
content trees. The reusable traversal, dedupe, cursor, and viewport policy live
in `nmp-feed`.

## Consequences

- The C ABI grows by one generic feed viewport intent, not a Chirp-specific
  timeline/window protocol.
- Chirp consumes a standard feed projection and stays a showcase app.
- Existing `nmp_app_chirp_snapshot` remains a legacy pull helper for REPL/tests,
  but the iOS and TUI showcase paths no longer use it for the home feed.
- `nmp_app_chirp_snapshot_window` is not exported.
- The ABI freeze override applies only to `nmp_app_load_older_feed`.
