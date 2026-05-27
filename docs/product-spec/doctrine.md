# Cardinal Doctrines D0–D10

These are not guidelines. They are the reasons why certain bugs are impossible to introduce through the safe API.

When you try to route a DM to a public relay, D10 stops you. When you want a spinner while a profile loads, D1 stops you. When you try to pick relays per-publish, D3 stops you. The framework doesn't document footguns — it refuses to expose the API that lets you make those mistakes.
Eleven principles in total. Every API decision answers to at least one; conflicts resolve in the order listed (D0 outranks D1, D1 outranks D2, and so on).

**Two kinds.** D0–D5 and D10 are *policy* doctrines — they govern what the framework promises and forbids. D6–D9 are *substrate invariants* — they govern how the runtime is implemented underneath. Both are equally binding, but they answer different review questions: policy doctrines ask "does this API make the wrong thing easy?" and substrate invariants ask "does this implementation break something the kernel must own, such as FFI boundaries, state propagation, hot-path cost, or time?"

---

## D0. The framework core knows nothing about your app's domain

The shared Rust core (`nmp-core`) has no concept of a tweet, a podcast episode, a highlight, or a group chat. Those nouns belong to your app. You add them through typed extension modules (`ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, `IdentityModule`). Two apps can be built on the same core without either one leaking its domain concepts into the shared substrate.

This rules out:

- The framework core becoming a grab-bag of every app's domain objects.
- Business logic written in Swift, Kotlin, or TypeScript instead of once in Rust.
- Apps poisoning each other through the shared core.

*Implementation detail: `nmp-core` enforces this via module traits. App nouns (including NIP-specific domain terms) are banned from `nmp-core` by the doctrine lint. Internal test surface is gated behind `#[cfg(any(test, feature = "test-support"))]` so production builds export no actor internals.*

---

## D1. Render now, refine in place

Your UI never waits for data before it can render. If a user's display name hasn't loaded yet, the framework supplies a shortened version of their public key. When the real name arrives, the cell updates automatically. The type system enforces this: display fields in view payloads are never `Option` — they always carry a value, either real or a deterministic placeholder. There is no `if loading { spinner }` branch available in the API, because there is no loading state in the payload types.

This rules out, by construction:

- Hiding a post because the author's profile hasn't arrived yet.
- Replacing cached data with a spinner on the theory that "fresher data might exist."
- Refusing to render a thread because the root event isn't in the local cache.
- Profile-picture flicker between a cached image and a placeholder.

*Implementation detail: Placeholders are part of the type contract. Freshness is exposed as an optional "badge hint" (not a render gate) when relevant. The view payload type physically cannot represent an empty display-name field.*

---

## D2. History syncs by diff, not by re-download

When your app needs historical events, the framework doesn't request everything from scratch — it uses a set-reconciliation protocol (NIP-77 / negentropy) that compares your local index against the relay's and fetches only what's missing. Live events still stream in real-time as usual.

This isn't an optimization you opt into later. It's the default subscription policy, built on watermark metadata the framework maintains per relay.

This rules out:

- Re-downloading your entire timeline every launch.
- Unbounded REQ scans for history when a relay supports smarter sync.
- Pagination logic you write yourself.

*Implementation detail: Every `(filter, relay)` pair the framework touches is tracked as a sync target with a watermark. Live REQ handles the tail; NIP-77 reconciliation handles historical backfill where supported.*

---

## D3. Relay routing is handled for you

You never specify which relay to send a request to. The framework follows Nostr's publish-list protocol (NIP-65): reads go to the relays each author publishes to; writes go to the author's declared write relays; direct messages go only to the recipient's inbox relays; discovery falls back to a configurable indexer set.

Manual relay selection exists as an audited override — named, observable, and excluded from the default app-building path. The safe path has no relay-selection field on subscriptions or publishes.

This rules out, by construction:

- Posts going to relays the author hasn't declared as their write relays.
- DMs leaking to public relays (D10 owns this specific invariant, but D3 is the prerequisite).
- Missing events because you asked the wrong relay.
- Relay fan-out logic written in app code.

*Implementation detail: The subscription planner and router compute relay targets from NIP-65 relay lists (kind:10002 events) stored in the framework's working set. Unknown relay lists surface as diagnostic state with bounded fallback policy, never as a silent default.*

---

## D4. One source of truth; everything else follows

Every piece of state has exactly one owner. The framework maintains several derived layers (in-memory working set, view projections, platform reactive shadow), but they all derive from a single authoritative source automatically. You don't invalidate caches. You don't sync state between layers. When the source changes, downstream representations update on their own.

Cache invalidation is not a concept in the public API — because it's not a concept the developer needs.

This rules out:

- The same fact showing different values in different parts of the UI.
- Cache invalidation logic in app code.
- State that can drift between the local store and the on-screen view.

*Implementation detail: Five layers exist (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow), each a mechanical derivation of the one above. The actor owns recomputation; the platform only receives derived state.*

---

## D5. Only what's on screen crosses the language boundary

The data that crosses from Rust to Swift/Kotlin/TypeScript is limited to what your currently-open views need. Your full event history never serializes across the boundary. When you close a view, its data is immediately evicted from the snapshot. Memory cost scales with the number of open screens, not with how much data you've cached locally.

This rules out:

- Memory growing proportionally to your event history instead of your screen count.
- Slow marshaling of large datasets across the language boundary.
- The event store ever being visible to native code.

*Implementation detail: `AppState` carries small screen-shaped state plus a `HashMap<ViewId, ViewPayload>` for views currently open. Closing a view drops its entry. The event store, gossip cache, sync watermarks, working set, and signer state live exclusively in the Rust actor.*

---

## D6. Errors show up in state, not as thrown exceptions

When something goes wrong, it surfaces as a state field — a toast message, a cleared loading indicator, a diagnostic record. The dispatch function (`dispatch_action`) is fire-and-forget: it never throws, never blocks, never returns an error. Swift never wraps framework calls in `do { try }`. Kotlin never writes `try { } catch`. Every failure has at least one observable state field carrying its consequence.

This rules out, by construction:

- Per-operation error types polluting the Swift/Kotlin API layer.
- Silent failures — errors without any observable state consequence.
- Native code needing to know whether a Rust operation succeeded.

*Implementation detail: Failures surface as `toast: Option<String>` state fields. Long-running operations expose `busy` flags that clear on completion regardless of outcome. No `Result<T, E>` ever crosses the Rust ↔ native boundary.*

---

## D7. Native bridges execute; the kernel decides

When native code needs to touch something the Rust kernel can't reach directly — the iOS Keychain, the Android file picker, push notifications, the camera — it executes the OS call and reports the raw result back to the kernel. It makes no decisions: no retry logic, no relay selection, no encryption-scheme negotiation, no policy. The kernel decides everything, in one place, in Rust.

This means the behavior on iOS and Android is always the same, because it's always decided by the same code.

This rules out, by construction:

- The same policy being written differently on iOS vs Android vs web.
- Retry logic or relay selection duplicated in native code.
- Platform bridges holding state the kernel doesn't know about.
- Capability lifecycles that aren't safe to start, stop, and restart multiple times.

*Implementation detail: Native bridges implement `CapabilityModule`. They may hold transient OS handles (Keychain handle, audio session token) but no policy state. Start/stop/restart of any bridge must be idempotent.*

---

## D8. Reactivity is bounded — UI updates stay predictable under any event volume

No matter how fast events arrive, each open view updates at most 60 times per second. Memory grows with the number of open views, not with the size of your event history. The framework maintains an index of which views care about which data, and only recomputes the views that are actually affected by a change. The UI thread is never the bottleneck.

This rules out, by construction:

- The UI updating 500 times per second because 500 events arrived.
- Memory growing with your cache size instead of your screen count.
- Every view recomputing on every event regardless of relevance.

*Implementation detail: A composite reverse index maps each inserted event to the views interested in it. View recompute is bounded to ≤60 deltas per second per view. Working set is hot-resident with claim-pinned overlays; allocations after warmup are linear in active-view count. Idle ticks that produce no state change do not emit snapshots. Validated continuously by `reactivity-bench` (`crates/nmp-testing/bin/reactivity-bench/`).*

---

## D9. Timestamps are the kernel's call, not the relay's

Which version of a profile is "newer", whether an event has expired, what timestamp goes on something you publish — all of these are decisions the kernel makes using its own clock. Relays cannot manipulate these by sending events with different timestamps. The same logic runs under test with a fixed clock, so behavior is deterministic and reproducible.

This rules out:

- A relay feeding stale profile data by claiming it's newer than what you have.
- Non-deterministic test behavior caused by reading real wall-clock time.
- Expiration decisions that differ between the kernel and the relay that delivered the event.

*Implementation detail: The kernel reads time exclusively through an injected `Clock` trait (`SystemClock` in production, `FixedClock` under `test-support`). Replaceable-event resolution (profile metadata, contact lists, relay lists, parameterized-replaceable kinds) picks winners by strict `>` on `created_at`. Relay-supplied timestamps cannot be tampered with without invalidating the event's cryptographic signature, so the only threat D9 guards against is the kernel's own reducers reading wall-clock directly.*

---

## D10. Private messages stay private; the framework enforces it

Direct messages (encrypted per the NIP-17/NIP-59 gift-wrap protocol) are published only to the specific inbox relays where the recipient can receive them. The framework refuses to publish a private message to a public relay, even as a fallback. If the recipient's inbox relays are unknown, the send fails rather than leaking. Every stored event also records which relay delivered it; that provenance is respected if the event is ever re-published.

This rules out, by construction:

- A direct message appearing on a public relay.
- Private events forwarded to a different relay because they happened to land in the local cache.
- The framework silently falling back to a public relay when inbox relays are unknown.

*Implementation detail: This is the provenance half of D3. D3 governs which relays a write routes to; D10 governs that private content never widens its audience and that received events are not laundered between relays. Gift-wrapped private message envelopes (NIP-59 kind:1059) may only be published to verified recipient inbox relays. `PublishPlanError::PrivateRecipientUnroutable` is the fail-closed error when inboxes are unknown.*
