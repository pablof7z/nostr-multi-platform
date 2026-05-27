# 17 — iOS shell: SwiftUI consumes the kernel

**Status: SHIPS** (legacy raw C FFI on master) · FlatBuffers transport target
in progress · Audience: builders

The kernel is the brain. SwiftUI is a **dumb render of a snapshot the kernel
hands you**. The platform never owns state, never decides retry policy, never
gates content on "is it loaded yet?". This section shows the exact bridge that
ships today in `ios/Chirp` (the active kernel-wired iOS app) and the rules
that keep it doctrine-clean.

## The bridge — legacy raw C FFI today, FlatBuffers target

There is no UniFFI on master (that is M14; see
[15 — Codegen and FFI](15-codegen-and-ffi.md)). iOS calls the hand-written
`extern "C"` surface in `crates/nmp-core/src/ffi.rs:44-275`
(`nmp_app_new`, `nmp_app_start`, `nmp_app_open_author`, `nmp_app_close_thread`,
…). On master, one C callback delivers updates as a single JSON string. The
transport migration replaces that callback payload with the canonical
FlatBuffers update schema; JSON is not retained as a runtime fallback.

### `KernelHandle` — the thin wrapper (annotated)

`ios/Chirp/Chirp/Bridge/KernelBridge.swift`:

```swift
final class KernelHandle {
    private let raw: UnsafeMutableRawPointer          // opaque *mut NmpApp
    private var updateSink: KernelUpdateSink?          // retains the closure box

    init()  { raw = nmp_app_new() }                    // spawns the Rust actor
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

    func openAuthor(pubkey: String) {                  // a command. NO return value.
        pubkey.withCString { nmp_app_open_author(raw, $0) }   // fire-and-forget
    }
    // decode(): update bytes → generated FlatBuffers reader → KernelUpdate shadow
}
```

Every command method is **fire-and-forget** — `nmp_app_open_author` returns
`void`. There is no synchronous "give me the result". State change arrives only
later, via the callback, as a fresh snapshot. That is the actor model (see
[04 — Actor model (TEA on one thread)](04-actor-and-tea.md)) crossing FFI intact.

The C callback (`KernelBridge.swift:101-110`) is invoked **on a Rust thread**.
In the legacy path it decodes JSON; in the FlatBuffers target it reads the
generated buffer. In both cases `KernelModel` hops to `@MainActor` before
touching any `@Published` (`KernelModel.swift:48-53`):

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
encode `AppUpdate` / `KernelUpdate` as a FlatBuffers frame
   │
   ▼  callback(context, bytes)                       ── Rust thread
KernelHandle.decode(): generated FlatBuffers reader → KernelUpdateResult
   │
   ▼  Task { @MainActor }                            ── hop to main
KernelModel.apply(result):
   guard result.update.rev > rev else { return }     ── REV GUARD (drop stale)
   rev = update.rev; items = update.items; … (assign every @Published)
   │
   ▼
SwiftUI observes @Published change → diffs view tree → re-renders rows
```

The kernel emits a **whole snapshot** (`KernelUpdate`,
`KernelBridge.swift:119-138`: `items`, `authorView`, `relayStatuses`,
`logicalInterests`, `metrics`, …) plus delta hints (`inserted`/`updated`/
`removed`). SwiftUI's own structural diffing turns "replace the array" into
minimal row updates — you do not hand-patch.

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

> Nuance: `KernelModel` keeps a 60s TTL `authorViewCache`/`threadViewCache`
> (`KernelModel.swift:130-199`). That is a *projection* cache for instant
> back-navigation, refreshed every snapshot — not a source of truth. The view
> still prefers the live `model.authorView` when it matches
> (`ProfileViews.swift:41-46`). Caching the *render input* briefly is fine;
> caching *facts* the kernel owns is the D4 violation.

## What a kernel-consuming SwiftUI view looks like

`@EnvironmentObject KernelModel`, render the snapshot, dispatch commands on
appear/disappear. No business logic, no fallbacks. The D1 pattern — render a
**placeholder**, never a spinner gate:

```swift
// ProfileViews.swift:51 — never "if missing { ProgressView() }"
ProfileCardView(profile: view?.profile ?? .placeholder(pubkey: pubkey))
// .task { model.openAuthor(pubkey:) }  /  .onDisappear { model.closeAuthor }
```

`ProfileInterestAvatar` (`SharedViews.swift:47-73`) claims the profile interest
`onAppear` and releases `onDisappear` — refcounted subscription lifecycle driven
purely by view lifecycle. The kernel reference-counts; the view just says "I'm
looking at this now / not anymore".

## Per-iOS-app status box

```
┌─ ios/Chirp ──────────────────── ACTIVE / kernel-wired ──────────────┐
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
[15 — Codegen and FFI](15-codegen-and-ffi.md) ·
[16 — Capabilities (D7)](16-capabilities.md)
