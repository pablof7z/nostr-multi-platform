# Cardinal Doctrines D0–D10

Eleven named principles that subsume the rest of the spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.

**Two kinds of doctrine.** D0–D5 and D10 are *policy* doctrines — they govern user-facing semantics (what the framework promises, what it forbids). D6–D9 are *substrate invariants* — they govern how the runtime is allowed to be implemented (what crosses FFI, how state propagates, what the hot path can do, how time is decided). Both kinds are equally binding. Policy review flags "this API choice violates a user-facing principle"; substrate review flags "this implementation choice will leak across FFI, hide policy on the native side, degrade reactivity, or trust a value the kernel must own."

## D0. Kernel + extension modules — no app nouns in `nmp-core`

`nmp-core` is substrate. Protocol behavior and app behavior live in typed modules — `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, `IdentityModule` — that contribute variants to the kernel. App nouns (Chirp, Notes, Highlighter, podcast, group-chat) are banned from `nmp-core`. If implementing a real app requires adding domain nouns to the kernel, the kernel boundary is wrong and must change.

This rules out:

- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
- App-specific business logic in Swift, Kotlin, or TypeScript shells.
- Closed FFI enums that prevent modules from contributing typed views, actions, updates, capabilities, or identity scopes.

Internal test-facing surface (e.g., `spawn_actor`) is gated behind `#[cfg(any(test, feature = "test-support"))]` so production builds export no actor internals.

## D1. Best-effort rendering — render now, refine in place

Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.

The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.

This rules out, by construction, the most common Nostr-client failure modes:

- Hiding a post because the author's profile hasn't loaded yet.
- Replacing cached profile metadata with a spinner because "we might have something newer."
- Refusing to render threads because the root event isn't in cache.
- Profile-picture flicker between cached and placeholder.

## D2. Negentropy first, REQ second

NIP-77 negentropy reconciliation is the default backfill mechanism. Every `(filter, relay)` pair the app touches is treated as a tracked sync target with a watermark. Live REQ remains the tailing path, but historical gaps consult coverage first and prefer sync over REQ scans when relays support it.

This is not a product feature you opt into later; it is a subscription policy built on explicit coverage metadata.

## D3. Outbox routing is automatic; manual relay selection is the opt-out

Per NIP-65, reads and writes are routed to the relevant relays by framework policy without normal app code specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.

The safe public path does not ask the developer to pick relays per operation. Explicit override and diagnostic/test paths exist, but they are named, observable, and excluded from the default app-building flow.

This rules out, by construction:

- Posts to relays the author hasn't declared as write relays.
- DMs leaked to public relays.
- Silent reads against a default relay set that miss an author's actual relays; unknown relay lists surface as coverage/diagnostic state and use a bounded fallback policy.
- Hand-rolled fan-out logic in app code.

## D4. Single writer per fact; caches derive

The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.

## D5. Snapshots bounded by what's open

What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI.

## D6. Errors never cross FFI as exceptions

Operational failures surface as `toast: Option<String>` state fields, never as exceptions or `Result<T, E>` across the FFI boundary. Long-running operations expose `busy` flags that clear on completion regardless of outcome. Native `dispatch` calls never need `try` / `catch`; native reconciler callbacks never receive a Rust error type.

This rules out, by construction:

- Swift / Kotlin code wrapping framework calls in `do { try }` or `try { } catch`.
- Per-operation error-type proliferation across UniFFI.
- Silent failure: every error has at least one observable state field carrying its consequences (a toast, a `busy` flag clearing, a diagnostic record).

## D7. Capabilities report; never decide policy

Native bridges (Keychain, NIP-46 bunker, BGTask scheduler, `AVPlayer`, NIP-07 web extension, FilePicker, Blossom upload, etc.) execute platform APIs and report raw events back into the kernel. They never decide *whether to retry*, *whether an error is recoverable*, *which relay to publish to*, *which encryption scheme to negotiate*, *what state should become*, or *whether a duplicate request is a no-op*. Policy is Rust's; capabilities are reports.

This rules out, by construction:

- Capability bridges holding cached state beyond transient OS handles (Keychain handle, audio session token, network monitor, push registration).
- Native code making decisions that the kernel should reproduce identically across platforms.
- Capability lifecycles that aren't idempotent: start / stop / restart of any bridge must be safe N times.

## D8. Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded

The substrate that delivers D1, D2, D4, D5 in practice. Every inserted event participates in a composite reverse index that names the views interested in it; view recompute is bounded to ≤60 deltas per second per view; the working set is hot-resident with claim-pinned overlays. Allocations after warmup are linear in active-view count, never in cached-event count. False-wakeup rate ≤ 0.10 candidates per delta.

The idle-tick emit path is gated on `kernel.changed_since_emit()` — idle ticks that produce no state change must not emit snapshots (D8 regression guard).

This rules out, by construction:

- Per-event allocations on the hot path after warmup.
- Wakeups proportional to event volume instead of view count.
- Memory growth that tracks history depth instead of working set.
- A view re-rendering at greater than 60 Hz regardless of upstream burst.

Validated continuously by `reactivity-bench` (`crates/nmp-testing/bin/reactivity-bench/`).

## D9. The kernel owns time; relay-supplied `created_at` is untrusted

Time is a kernel decision, never a relay assertion. The kernel reads "now" exclusively through its injected `Clock` trait (`SystemClock` in production, `FixedClock` under `test-support`), so every time-dependent reduction is deterministic under test and reproducible during replay. The kernel — not the relay that delivered an event — owns every decision that consumes time:

- **Signing.** When the kernel stamps `created_at` on an event it is about to sign, the value comes from the injected `Clock`.
- **Replaceable-event resolution** (kinds 0, 3, 10002, and parameterized-replaceable). The canonical "winner" is picked by strict `>` on `created_at` in the `EventStore` and the kernel's local caches.
- **NIP-40 expiration.** Whether an event has expired is computed against the kernel clock, not asserted by the relay that delivered it.
- **`received_at_ms` provenance.** The wall-clock stamp the store records for an inbound event reads the injected `Clock`, so replay is deterministic.

This rules out, by construction:

- Trusting a relay's word on whether an event is "newer" or "expired".
- Non-deterministic reducers that read wall-clock directly and break replay.

A relay **cannot** tamper with an inbound event's `created_at`: the Schnorr signature covers `[0, pubkey, created_at, kind, tags, content]`, so any change to the timestamp invalidates the signature and the event is rejected by signature verification. A future-dated event can therefore only have been produced by the author's own signing client — there is no inbound future-timestamp threat to gate against, and NMP performs no such ingest-time rejection.

## D10. Provenance — private events never escape to public relays

The kernel tracks where every event came from and respects that origin for routing. Two invariants:

1. **Source is recorded.** Every stored event carries per-event provenance — which relay(s) delivered it, with first/last-seen timestamps and a primary entry. An event received from one relay is never silently forwarded to a *different* relay without explicit user intent; the kernel does not treat "I have this event" as license to republish it anywhere.
2. **Private events stay private.** "Private" in the Nostr sense means the NIP-59 gift-wrap (kind:1059) and the NIP-17 direct-message rumor payloads it carries. A kind:1059 gift-wrap is addressed to a specific recipient and must be published **only** to that recipient's direct-message / inbox relays — never to general-purpose public relays, never to a recipient-unknown fallback set. When the recipient inbox is unknown, the publish **fails closed** (no public-relay fallback).

This is the provenance half of D3 (D3 owns *which* relays a write routes to; D10 owns *that private content never widens its audience* and *that received events are not laundered between relays*). It rules out, by construction:

- Republishing a privately-delivered event to a public relay because it happened to land in the store.
- A kind:1059 gift-wrap leaking onto a non-DM relay (or onto an indexer fallback set when the recipient's inbox relays are unknown).
- Cross-relay forwarding of received events as an implicit side effect of having cached them.
