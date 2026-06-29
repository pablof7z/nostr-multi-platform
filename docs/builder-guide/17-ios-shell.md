# 17 — iOS shell: SwiftUI consumes the kernel

**Status: SHIPS** (UniFFI native binding + binary FlatBuffers update callback) · Audience:
builders

The kernel is the brain. SwiftUI is a **dumb render of a snapshot the kernel
hands you**. The platform never owns state, never decides retry policy, never
gates content on "is it loaded yet?". This section shows the native binding
pattern and the rules that keep it doctrine-clean.

## The bridge — UniFFI handle, FlatBuffers updates

Native shells import the generated UniFFI module. The binding exposes lifecycle,
byte action dispatch, update callbacks, and capability interfaces. Feed session
open/close is not a native symbol pair: Rust app/protocol composition code owns
typed read sessions/helpers, and iOS consumes the resulting typed output stream.
One callback delivers binary `nmp.transport.UpdateFrame` bytes with file
identifier `NMPU`. The frame is FlatBuffers-only: `Snapshot` or `Panic`, with no
JSON snapshot fallback. Legacy native calls are transitional/internal
compatibility, not the app recipe.

### `KernelHandle` — the thin wrapper (annotated)

Representative wrapper shape:

```swift
final class KernelHandle {
    private let app: NmpAppHandle                     // UniFFI object
    private var updateSink: KernelUpdateSink?          // retains callback target

    init()  { app = NmpAppHandle() }                   // passive until start()
    deinit  {
        app.close()                                    // idempotent actor shutdown
    }

    func listen(_ h: @escaping (KernelUpdateResult) -> Void) {
        let sink = KernelUpdateSink(handler: h)
        updateSink = sink
        app.setUpdateSink(sink)
    }

    func dispatchAction(_ bytes: [UInt8]) {
        let result = app.dispatchActionBytes(bytes)
        // Decode only the enqueue result; terminal state arrives in snapshots.
    }

    func loadOlderFeed(key: String) {
        app.loadOlderFeed(key: key)
    }
    // decode(): UpdateFrame bytes → generated FlatBuffers readers → KernelModel shadow
}
```

Action dispatch returns only an enqueue/validation result; it is not a state
query. Viewport commands such as "load older feed" are also fire-and-forget.
State change arrives later, via the callback, as a fresh snapshot. Feed
open/close handles live on the Rust typed-session side, so Swift never
re-derives filters or owns feed teardown policy. That is the actor model (see
[04 — Actor model (TEA on one thread)](04-actor-and-tea.md)) crossing FFI intact.

The update callback is invoked **on a Rust thread**.
It copies the borrowed bytes, decodes the generated FlatBuffers frame, and then
`KernelModel` hops to `@MainActor` before touching any `@Published`
(`KernelModel.swift:48-53`):

```swift
kernel.listen { [weak self] result in
    Task { @MainActor [weak self] in self?.apply(result: result) }
}
```

## Rust emit → SwiftUI re-render sequence

```
relay frame → kernel actor ingests → reverse-index delta → emit pacer
   │  (one snapshot per emit tick, paced by emit_hz)
   ▼
encode `nmp.transport.UpdateFrame` as FlatBuffers
   │
   ▼  update callback(bytes)                         ── Rust thread
KernelHandle.decode(): generated FlatBuffers readers → KernelUpdateResult
   │
   ▼  Task { @MainActor }                            ── hop to main
KernelModel.apply(result):
   guard result.update.rev > rev else { return }     ── REV GUARD (drop stale)
   rev = update.rev; items = update.items; … (assign every @Published)
   │
   ▼
SwiftUI observes @Published change → diffs view tree → re-renders rows
```

The kernel emits a **whole snapshot frame**: typed envelope fields plus typed
projection rows. SwiftUI's own structural diffing turns "replace the model
slot" into minimal row updates — you do not hand-patch.

## FlatBuffers update shape + the rev guard

`KernelUpdate` remains the decoded shadow model keyed by `rev: UInt64`; the
runtime frame that carries it is FlatBuffers. The guard in `KernelModel.apply`
(`KernelModel.swift:138-141`) is the entire concurrency correctness story:

```swift
private func apply(result: KernelUpdateResult) {
    guard result.update.rev > rev else { return }   // monotonic; drop reorders
    rev = update.rev
    items = update.items                            // wholesale replace
    profile = update.profile                        // ObservableObject diffs
    relayStatuses = update.relayStatuses            // for you
    // …assign every field, then record perf metrics
}
```

`rev` is monotonic in the kernel. If two callbacks land out of order (possible —
they cross a thread boundary), the stale one is dropped. **Never disable this
guard** and never derive UI truth from anything but the latest applied snapshot.

> Nuance: `KernelModel` may keep projection merge caches for incremental apply
> and decode-before-commit. They are transport shadows of Rust-owned state, not
> sources of truth. A `Cleared` typed row removes a cached dynamic key
> immediately; an omitted key in an incremental frame retains the last decoded
> value until a later `Changed` or `Cleared`. Caching the *render input* for the
> update stream is fine; caching *facts* the kernel owns is the D4 violation.

## Reading a typed projection in `apply()`

> **Disambiguation.** "Snapshot projection" here means an app/module-owned slice
> delivered under its key in `SnapshotFrame.typed_projections` (registered
> Rust-side via `register_typed_snapshot_projection`; see
> [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md)). It is not a
> host-owned source of truth.

The named typed fields in `apply()` (`items`, `profile`, `relayStatuses`) are
the kernel's built-in slices. App- and module-owned state arrives in the same
frame as keyed typed rows — you read the row bytes by key, decode them
with the generated Swift decoder, and assign the resulting value wholesale to an
`@Published` property. Chirp's OP feed typed row is the precedent:

```swift
// KernelUpdateFrameDecoder extracts typed rows from SnapshotFrame.typedProjections.
let typedProjections = extractTypedProjections(from: snapshot)
let homeFeed = TypedHomeFeedDecoder.decode(from: typedProjections)

// KernelModel.apply consumes decoded typed values every accepted rev.
private func apply(result: KernelUpdateResult) {
    guard result.update.rev > rev else { return }   // same rev guard — drop reorders
    rev = result.update.rev
    relayStatuses = result.update.relayStatuses      // envelope field
    homeFeed = result.update.homeFeed                // typed projection row
}
```

Projection reads obey the same rules as every other snapshot field:

- **Wholesale-replace** the projection on every snapshot; never merge or append
  into the prior value (D4).
- **Honor the monotonic rev guard** — a projection read from a stale frame is
  dropped with the rest of that frame.
- **Apply presence deliberately** — `Changed` replaces the value, `Cleared`
  removes it, and omission in an incremental frame retains the last decoded
  value. An idle/empty value is a decoded payload, not absence.
- **Render placeholders, not spinners** — missing optional fields inside a
  decoded payload render D1 placeholders; they never become loading gates.
- **Never derive UI truth** from anything but the latest applied snapshot;
  typed projections are shadows of kernel-owned state, not caches you mutate.

## What a kernel-consuming SwiftUI view looks like

`@EnvironmentObject KernelModel`, render the snapshot, dispatch commands on
appear/disappear. No business logic, no fallbacks. The D1 pattern — render a
**placeholder**, never a spinner gate:

```swift
// ProfileViews.swift:51 — never "if missing { ProgressView() }"
ProfileCardView(profile: view?.profile ?? .placeholder(pubkey: pubkey))
// .task { model.openAuthorFeed(pubkey:) } / .onDisappear { model.closeAuthorFeed(pubkey:) }
```

`ProfileInterestAvatar` (`SharedViews.swift:47-73`) claims the profile interest
`onAppear` and releases `onDisappear` — refcounted subscription lifecycle driven
purely by view lifecycle. The kernel reference-counts; the view just says "I'm
looking at this now / not anymore".

## Per-iOS-app status box

```
┌─ apps/nmp-gallery/ios ────────────── IN-TREE / kernel-wired ─────────────┐
│ In-repo native shell proof over UniFFI-style bindings and FlatBuffers.   │
└──────────────────────────────────────────────────────────────────────────┘
┌─ github.com/pablof7z/chirp ───────── EXTERNAL CONSUMER ──────────────────┐
│ Extracted production Nostr client; consumes NMP as an external framework. │
└──────────────────────────────────────────────────────────────────────────┘
```

Gallery is the in-tree iOS shell proof. Chirp is the extracted production
consumer and should be fixed in its own repository, not restored to this
monorepo to satisfy native-shell docs.

## Anti-patterns

1. **Caching kernel facts in Swift.** `@Published` fields are a *shadow* of the
   latest snapshot, reassigned wholesale every `apply`. Don't merge, append, or
   persist them — that re-owns state the kernel owns (D4).
2. **Calling C FFI off-main without hopping back.** The callback fires on a Rust
   thread; mutating `@Published` there crashes SwiftUI. Always
   `Task { @MainActor }` before assignment (`KernelModel.swift:48-53`).
3. **Business logic in SwiftUI.** No retry, no relay choice, no "is logged in?"
   gate in views. Views render `KernelUpdate` and dispatch commands. Policy is
   kernel/capability territory (D7).
4. **`if missing { ProgressView() }` content gates.** Render the placeholder
   (`.placeholder(pubkey:)`), let the snapshot fill in. Withholding cached
   content behind a spinner violates D1.
5. **Disabling / second-guessing the rev guard.** `guard update.rev > rev` is
   the only thing making out-of-order callbacks safe. Removing it = flicker and
   stale UI; "fixing" symptoms by patching views instead is worse.

## Concrete deliverables recap

- Annotated `KernelHandle` snippet — UniFFI handle, fire-and-forget commands,
  ordered teardown, retained update sink.
- Rust emit → SwiftUI re-render sequence with the rev guard placed exactly.
- FlatBuffers update shape + rev-guard code; per-iOS-app status box.

See also: [04 — Actor model (TEA on one thread) (TEA on one thread)](04-actor-and-tea.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[16 — Capabilities (D7)](16-capabilities.md)
