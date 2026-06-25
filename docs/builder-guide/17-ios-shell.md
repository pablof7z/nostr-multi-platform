# 17 — iOS shell: SwiftUI consumes the kernel

**Status: SHIPS** (raw C FFI + binary FlatBuffers update callback) · Audience:
builders

The kernel is the brain. SwiftUI is a **dumb render of a snapshot the kernel
hands you**. The platform never owns state, never decides retry policy, never
gates content on "is it loaded yet?". This section shows the exact bridge that
ships today in `apps/chirp/ios` (the active kernel-wired iOS app) and the rules
that keep it doctrine-clean.

## The bridge — raw C calls, FlatBuffers updates

There is no UniFFI on master (that is M14; see
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md)). iOS calls the `extern "C"`
surface exported by `crates/nmp-ffi` (`nmp_app_new`, `nmp_app_start`,
`nmp_app_dispatch_action`, the generic feed doorway, capability callbacks,
etc.).
One C callback delivers binary `nmp.transport.UpdateFrame` bytes with file
identifier `NMPU`. The frame is FlatBuffers-only: `Snapshot` or `Panic`, with no
JSON snapshot fallback.

### `KernelHandle` — the thin wrapper (annotated)

`apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`:

```swift
final class KernelHandle {
    private let raw: UnsafeMutableRawPointer          // opaque *mut NmpApp
    private var updateSink: KernelUpdateSink?          // retains the closure box

    init()  { raw = nmp_app_new() }                    // allocates a passive handle
    deinit  {                                          // ordered teardown:
        nmp_app_set_update_callback(raw, nil, nil)     //  1. detach callback
        nmp_app_free(raw)                              //  2. free → actor shutdown
    }

    func listen(_ h: @escaping (KernelUpdateResult) -> Void) {
        let sink = KernelUpdateSink(handler: h)
        updateSink = sink                              // Swift owns the box…
        nmp_app_set_update_callback(                   // …Rust gets a raw ptr to it
            raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    func openFeed(paramsJson: String) {                // returns an opaque close handle.
        paramsJson.withCString { paramsPtr in
            guard let handle = nmp_app_open_feed(raw, paramsPtr) else { return }
            defer { nmp_app_string_free(handle) }
            // Store String(cString: handle) and pass it to nmp_app_close_feed.
        }
    }
    // decode(): UpdateFrame bytes → generated FlatBuffers readers → KernelModel shadow
}
```

Feed open returns only an opaque handle for symmetric teardown; it is not a data
result. State change arrives later, via the callback, as a fresh snapshot. Feed
close passes that handle back to `nmp_app_close_feed`. That is the actor model (see
[04 — Actor model (TEA on one thread)](04-actor-and-tea.md)) crossing FFI intact.

The C callback (`KernelBridge.swift:101-110`) is invoked **on a Rust thread**.
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
   ▼  callback(context, bytes)                       ── Rust thread
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
projection sidecars. SwiftUI's own structural diffing turns "replace the model
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
> [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md)). It is **not** the ViewModule
> view-delta system, and it is **not** a host-owned source of truth.

The named typed fields in `apply()` (`items`, `profile`, `relayStatuses`) are
the kernel's built-in slices. App- and module-owned state arrives in the same
frame as keyed typed sidecars — you read the sidecar bytes by key, decode them
with the generated Swift decoder, and assign the resulting value wholesale to an
`@Published` property. Chirp's OP feed typed sidecar is the precedent:

```swift
// KernelUpdateFrameDecoder extracts the sidecars from SnapshotFrame.typedProjections.
let typedProjections = extractTypedProjections(from: snapshot)
let homeFeed = TypedHomeFeedDecoder.decode(from: typedProjections)

// KernelModel.apply consumes decoded typed values every accepted rev.
private func apply(result: KernelUpdateResult) {
    guard result.update.rev > rev else { return }   // same rev guard — drop reorders
    rev = result.update.rev
    relayStatuses = result.update.relayStatuses      // envelope field
    homeFeed = result.update.homeFeed                // typed projection sidecar
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
┌─ apps/chirp/ios ──────────────────── ACTIVE / kernel-wired ──────────────┐
│ Production Nostr client and current NMP showcase.                   │
│ Real actor, real relays, real snapshot loop.                        │
└─────────────────────────────────────────────────────────────────────┘
```

Only **Chirp** is an active iOS product proof today. Additional app shells are
deferred until Chirp is complete; treating deleted historical scaffolds as proof
of the iOS path is drift; see [27 — Doc/code discrepancies](27-discrepancies.md).

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

- Annotated `KernelHandle` snippet — opaque ptr, fire-and-forget commands,
  ordered teardown, unmanaged callback box.
- Rust emit → SwiftUI re-render sequence with the rev guard placed exactly.
- FlatBuffers update shape + rev-guard code; per-iOS-app status box.

See also: [04 — Actor model (TEA on one thread) (TEA on one thread)](04-actor-and-tea.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[16 — Capabilities (D7)](16-capabilities.md)
