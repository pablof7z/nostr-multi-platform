# nmp-v1.0.0-rc.1 Migration Note

This note is the consumer-facing migration checklist for the `nmp-v1.0.0-rc.1`
release candidate tag. It is a mechanical summary of the v1 feed-surface-freeze
issues closed against #2690 since `nmp-v0.8.4`; it does not re-derive their
design rationale — see the linked issues for that. It complements the durable
target guide in `docs/migration.md`.

This is a release **candidate**. The public surface described here is frozen
per #2690's exit criteria, but a second RC (`nmp-v1.0.0-rc.2`) is expected
once #2899 (concept-read binding surface) lands to give native (iOS/Android)
and web consumers a publishable pin; treat this RC as the reference for the
Rust/CLI surface first.

## Deleted Or Renamed Crates And APIs

- **`FeedSessions` → `Feeds`, `FeedSessionHandle` → `FeedHandle` (#2783).**
  "Session" is internal runtime-bookkeeping vocabulary (#2508) and must not
  appear on any app-facing surface. Hard break, no aliases:
  - `NmpApp::feeds()` now returns `Feeds<'a>` (was `FeedSessions<'a>`).
  - The handle type returned by `open(...)`/`open_spec(...)` is `FeedHandle`
    (was `FeedSessionHandle`) across nmp-native-runtime, nmp-uniffi-support,
    the browser runtime, and the generated web worker protocol helpers.
  - The handle's `session_id: u64` field is now `handle_id: u64`.
  - `close_feed_session(...)` is now `close_feed(...)`.
  - Internal machinery is unaffected: `nmp-feed-session`, `session_engine.rs`,
    `FeedSessionRegistry` keep "session" in their names — the ban is on
    app-facing/public surfaces only.

  Before:

  ```rust
  let sessions: FeedSessions = app.feeds();
  let handle: FeedSessionHandle = sessions.open_spec(key, spec)?;
  handle.close_feed_session()?;
  ```

  After:

  ```rust
  let feeds: Feeds = app.feeds();
  let handle: FeedHandle = feeds.open_spec(key, spec)?;
  handle.close_feed()?;
  ```

- **`NmpApp::open_observed_feed_source` deleted (#2770).** This deprecated,
  app-facing raw feed-source read doorway (and its doc claim that it "remains
  the correct doorway") is removed entirely — no alias, no shim, no deprecation
  period. The repo now hard-bans `#[deprecated]` in workspace source
  (doctrine-lint ratchet; generated FlatBuffers files exempt). The raw
  internal read-machinery that doorway exposed is no longer reachable from any
  app-facing/public surface. If you previously depended on it, replace it with
  a named concept-owned read (see below) or a typed read session, or file an
  app-owned recipe issue — do not reintroduce a generic session/source-reducer
  doorway.

- **Concept doorways relocated out of `nmp-native-runtime` (#2797).** A
  concept's doorway symbol now lives only in its concept crate — "if you
  don't depend on `nmp-nip25`, `open_reactions` does not exist at any layer."
  `nmp-native-runtime` no longer depends on per-concept `nmp-nip*` crates:
  - `open_search` moved to `nmp-nip50`.
  - Group-feed and group-scoped reaction doorways moved to their group concept
    owner (NIP-29 kind-blind transport rules apply; see
    `docs/architecture/crate-boundaries.md`).
  - There are no re-export shims left in `nmp-native-runtime` for either.
  - Practical effect: an app kernel that composes fewer concept crates gets a
    smaller symbol surface, not just smaller binaries — narrower feeds are now
    structural, not a convention.

## Projection Keys And Schema IDs

- **New concept-owned active reads for plain notes (#2758).** `open_replies`,
  `open_reactions`, and `open_reposts` are now implemented for ordinary
  kind:1 notes, not only NIP-29 groups:
  - `nmp-nip01` / `nmp-nip22`: `open_replies(target)`, composing NIP-10
    kind:1 and NIP-22 kind:1111 reply conventions.
  - `nmp-nip25`: `open_reactions(target_event_id)`, including
    delete/retraction handling (generalized from the prior NIP-29-group-scoped
    implementation).
  - `nmp-nip18`: `open_reposts(target_event_id)`.
  - `open_zaps(target_event_id)` also landed, but lives in `nmp-nip57` /
    `nmp-zaps`, which the release manifest classifies as **private, post-v1
    crates** (`release/nmp-release.toml`) — it is not part of the v1 public
    release train in this RC.
  - There is still no global `EventRelationSummary`/bucket API and no generic
    `open_session(namespace, bytes)` doorway — those shapes were explicitly
    rejected by #2508; use the named per-concept reads above.
- Continue treating projection schema IDs and versions as generated contracts,
  as of `nmp-v0.8.4`; nothing about schema ID shape changed in this RC beyond
  the new concept-owned reads above.

## Dispatch Envelopes And Actions

- No dispatch-envelope or action-namespace shape changes landed in this RC
  beyond what `nmp-v0.8.4` already documents. Continue using generated action
  builders / `dispatch_bytes` / `handle_dispatch_bytes`; do not hand-author
  action namespaces or JSON payloads.

## UniFFI And Binding Changes

- Regenerate native (Swift/Kotlin) and web bindings from this release: the
  `FeedSessions`/`FeedSessionHandle`/`session_id`/`close_feed_session` rename
  (#2783) touches nmp-uniffi-support, the browser runtime, and generated
  helpers (e.g. `web/packages/runtime-web/src/feedHelpers.generated.ts`).
  Generated outputs are renamed via their generator/templates — do not
  hand-edit generated files.
- `nmp-uniffi` remains the decided-deleted shared facade crate (#2763); it
  received only the mechanical rename update to keep the tree compiling and
  should not be depended on by new work. App-owned UniFFI facade crates plus
  `nmp-uniffi-support` are the supported path.

## Consumer Checklist

- Re-pin NMP to `nmp-v1.0.0-rc.1`, then run `nmp upgrade --to 1.0.0-rc.1` in
  the app root.
- Replace `FeedSessions`/`FeedSessionHandle`/`session_id`/
  `close_feed_session()` call sites with `Feeds`/`FeedHandle`/`handle_id`/
  `close_feed()`.
- Delete any use of `NmpApp::open_observed_feed_source` or the raw internal
  read-machinery handle it exposed; replace with `open_replies`/
  `open_reactions`/`open_reposts` (and, only if you already depend on the
  private post-v1 zap crates, `open_zaps`), or file an app-owned recipe issue
  for anything not yet covered.
- If you called `open_search` or a group-feed/group-reaction doorway via
  `nmp-native-runtime`, update the import to the concept crate that now owns
  it (`nmp-nip50` for search; the group concept crate for group feed/reaction
  reads).
- Regenerate native and web bindings from this tag before re-pinning consumer
  app code against them.
- Run the app crate tests, native/web binding generation checks, and
  `cargo test -p nmp-testing --test doctrine_lint_smoke` before treating the
  re-pin as complete.
