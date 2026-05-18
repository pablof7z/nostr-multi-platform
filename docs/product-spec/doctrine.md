# Cardinal Doctrines D0–D8

[Back to Product Spec: Overview And Developer Experience](./overview-and-dx.md)

Nine named principles that subsume the rest of the spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.

**Two kinds of doctrine.** D0–D5 are *policy* doctrines — they govern user-facing semantics (what the framework promises, what it forbids). D6–D8 are *substrate invariants* — they govern how the runtime is allowed to be implemented (what crosses FFI, how state changes propagate, what the hot path can do). Both kinds are equally binding; their distinction is the kind of review they enforce. Policy review flags "this API choice violates a user-facing principle"; substrate review flags "this implementation choice will leak across FFI / hide policy on the native side / degrade reactivity."

## D0. Kernel + extension modules — no app nouns in `nmp-core`

Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.

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

This is not a product feature you opt into later; it is a subscription policy built on explicit coverage metadata. See §7.8.

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

What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI. See §6.2 and the FFI architecture appendix (§A1).

## D6. Errors never cross FFI as exceptions

Operational failures surface as `toast: Option<String>` state fields, never as exceptions or `Result<T, E>` across the FFI boundary. Long-running operations expose `busy` flags that clear on completion regardless of outcome. Native `dispatch` calls never need `try` / `catch`; native reconciler callbacks never receive a Rust error type.

This rules out, by construction:

- Swift / Kotlin code wrapping framework calls in `do { try }` or `try { } catch`.
- Per-operation error-type proliferation across UniFFI.
- Silent failure: every error has at least one observable state field carrying its consequences (a toast, a `busy` flag clearing, a diagnostic record per ADR-0007).

Per RMP bible invariant #2 (`docs/aim.md` §2).

## D7. Capabilities report; never decide policy

Native bridges (Keychain, NIP-46 bunker, BGTask scheduler, `AVPlayer`, NIP-07 web extension, FilePicker, Blossom upload, etc.) execute platform APIs and report raw events back into the kernel. They never decide *whether to retry*, *whether an error is recoverable*, *which relay to publish to*, *which encryption scheme to negotiate*, *what state should become*, or *whether a duplicate request is a no-op*. Policy is Rust's; capabilities are reports.

This rules out, by construction:

- Capability bridges holding cached state beyond transient OS handles (Keychain handle, audio session token, network monitor, push registration).
- Native code making decisions that the kernel should reproduce identically across platforms.
- Capability lifecycles that aren't idempotent: start / stop / restart of any bridge must be safe N times.

Per RMP bible cardinal rule #6 (`docs/aim.md` §2) and idempotence invariant #7.

## D8. Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded

The substrate that delivers D1, D2, D4, D5 in practice. Every inserted event participates in a composite reverse index that names the views interested in it; view recompute is bounded to ≤60 deltas per second per view; the working set is hot-resident with claim-pinned overlays. Allocations after warmup are linear in active-view count, never in cached-event count. False-wakeup rate ≤ 0.10 candidates per delta.

The idle-tick emit path is gated on `kernel.changed_since_emit()` — idle ticks that produce no state change must not emit snapshots (D8 regression guard).

This rules out, by construction:

- Per-event allocations on the hot path after warmup.
- Wakeups proportional to event volume instead of view count.
- Memory growth that tracks history depth instead of working set.
- A view re-rendering at greater than 60 Hz regardless of upstream burst.

Per ADRs 0001 (composite dependency keys), 0002 (per-view delta budget), 0003 (working-set memory), and 0004 (allocation measurement), all in `docs/decisions/`. Validated continuously by `reactivity-bench` (`crates/nmp-testing/bin/reactivity-bench/`).
