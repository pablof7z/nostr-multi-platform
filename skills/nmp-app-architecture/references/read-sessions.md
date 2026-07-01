# Typed Read Sessions

> Authority: ADR-0070 (typed read sessions), with ADR-0042, ADR-0053, ADR-0062, ADR-0058
> folded in. For the projection emission/transport mechanics that sessions emit through, see
> `projections-and-emission.md`.

## The read model in one sentence

A **typed read session** is the single unit through which product code opens, observes, and
closes a read demand. It owns the complete lifecycle; nothing else does. One product read =
one session.

## What a session owns

| Concern | Owner |
|---|---|
| Acquisition demand (kinds, authors, scope) | session |
| Route policy and relay provenance | session |
| Bounded replay-before-live | session |
| Live event / store / capability sink | session |
| Admission predicate and fail-closed behavior | session |
| Typed output schema and status | session |
| Wake sources (event, store, source, mailbox changes) | session |
| Teardown on view close / source withdrawal | session |

The session descriptor is the public API the app opens and closes. Internal wiring (observed
delivery, source reducers, projection emission) is hidden executor machinery.

## Correct app-facing verbs

| Path | Open | Close |
|---|---|---|
| Native (Rust) | `app.feeds().open(feed_key, feed_spec)` / generated feed helper | `app.feeds().close(&handle)` |
| UniFFI (iOS/Android) | generated/app-owned facade over `open_feed_session` | generated/app-owned close by handle/session id |
| NIP-50 search | `NmpApp::open_search_session(descriptor)` | `NmpApp::close_search_session(&handle)` |

The shell opens one, renders the pushed typed output on the projection key, and passes the
handle to close. These are typed session helpers, not lifecycle peers of `open_interest`.
`NmpApp::open_feed(params, compiler)` and `open_feed_session` are lower-level
compiler/bridge mechanics, not the product API taught to apps.

## Substrate-only primitives — NOT the product read API

- **`open_interest` / `close_interest`** — raw acquisition verbs on `KernelReducer` and the
  actor `InterestsCommand`. Permitted only in substrate, protocol-internal, diagnostic/test/
  export, or an explicit migration surface with a named deletion trigger. A product screen
  that calls `open_interest` directly is a D0+D5 violation; it carries no replay, admission,
  typed output, or teardown.
- **`ObservedProjection` / `ObservedProjectionRegistrar` / `ObservedProjectionReconciler`** —
  observer executor machinery used internally by session executors. ADR-0070 classifies these
  as private; product code must not assemble them.
- **`ReducedSource` dynamic source wiring** — source reconciliation is internal. Source
  replacement (follow-list update, empty-source fail-closed) is owned by the session, not app
  code.
- **`declare_consumed_projections` / Tier-1/Tier-2 vocabulary** — output transport plumbing,
  not the product read API (ADR-0053 folded into ADR-0070). Do not teach it as a composition
  step.
- **`PullCursor` / `pull_page`** (ADR-0058) — raw event-log pull for external mirrors and feed
  pagination internals, not a UI read API. Host projection consumption stays push (ADR-0039);
  `pull_page` must never run on the UI thread from `apply()`.

The `product_raw_read` doctrine-lint rule bans `open_interest`, `ObservedProjection`,
`register_event_observer`, `register_live_event_tap`, and friends in `apps/**/src/**` and CLI
templates. The architecture scanner adds a complementary cross-repo warning.

## Invariants to enforce

1. **One session per product read.** A screen opening multiple raw interests and self-filtering
   is a D4+D5 violation (multiple writers, unbounded delivery scope).
2. **Empty dynamic source sets fail closed.** Empty authors/tags/refs never become wildcard
   relay demand. A session with no resolved sources surfaces as status, not a broad
   subscription.
3. **Teardown is atomic and handle-driven.** `close_feed(&handle)` is the only teardown path.
   No shell re-derives a filter string to compute teardown.
4. **Replay-before-live is mandatory.** A session executor delivers cached/stored matching
   events before live broadcasts. Apps must not hydrate read models by querying LMDB directly
   (ADR-0062).
5. **Source changes are internal.** When the active account's follow list changes, the
   session's source reducer replaces its materialized child-interest set and sends a CLOSE
   diff + new REQ. The shell observes the updated typed output; it does not re-open or
   re-configure the session.
6. **Typed output is the FFI surface.** The session emits a bounded, screen-shaped typed
   projection (e.g. `microblog.items`, `nmp.nip29.group_events`). Raw events, event arrays,
   and unbounded store slices do not cross FFI (D5).

## D5 and sessions

D5 (only what is on screen crosses FFI) is enforced by session *ownership*, not only snapshot
sizing. The session scopes acquisition to the demand it was opened with; only its typed output
leaves Rust. The event store, watermarks, signer state, and gossip cache stay inside. A
session opened for a closed view leaks D5 — teardown must run when the view is gone.

## Violation checklist (blocking)

- Product screen / app-core crate calls `open_interest` directly.
- Product code constructs or passes `ObservedProjection` / `ObservedProjectionRegistrar`.
- Product code wires source reducers, dynamic source sets, or dependent-interest replacement.
- Shell subscribes a raw filterless event observer and self-filters.
- Shell passes relay URLs to a view-open or feed-open call.
- Session teardown re-derives a filter string instead of using the stored handle.
- Empty follow/group/author set opens a wildcard or unscoped subscription.
- Docs teach `open_interest`, `declare_consumed_projections`, or Tier vocabulary as the
  product read API.

## See also

- `docs/decisions/0070-typed-read-sessions.md` — primary ADR, full fitness functions.
- `docs/builder-guide/19a-walkthrough-microblog.md` — `register_microblog_read_session` shape.
- `docs/builder-guide/06-reactivity-contract.md`, `07-subscription-planner.md`.
- `projections-and-emission.md` — the typed-projection transport sessions emit through.
