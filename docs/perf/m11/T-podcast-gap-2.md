# T-podcast-gap-2 — no `nmp_podcast_*` FFI surface

> **Surfaced by:** T157 step 1-2 (Android podcast APK pipeline)
> **Filed:** 2026-05-18
> **Status:** Open — blocks all podcast action dispatch from any shell
> **Depends on:** [`T-podcast-gap-1.md`](T-podcast-gap-1.md) (kernel
> integration must land first so there's a handler to dispatch to)

## Symptom

`crates/nmp-core` exposes ~30 `nmp_app_*` C ABI entrypoints (see
`crates/nmp-core/src/lib.rs` re-exports — `nmp_app_new`, `nmp_app_start`,
`nmp_app_publish_note`, `nmp_app_follow`, etc.). Every one of them is a
Nostr-flavored noun. There are **zero podcast-flavored entrypoints**:

- No `nmp_podcast_subscribe(*const c_char) -> i64` (or similar)
- No `nmp_podcast_unsubscribe`, `nmp_podcast_play`, `nmp_podcast_pause`
- No way for the iOS Swift `PodcastBridge` or the Android Kotlin
  `PodcastKernelBridge` to dispatch a podcast action from the shell to
  the kernel
- No iOS-side bridge work yet that this Android task would mirror — the
  `git log --oneline | grep -iE "ios-podcast|podcast.*ffi"` returns
  zero matches, so there's no iOS surface to coordinate with

## Evidence

```bash
$ grep -nE '^pub use .*::nmp_' crates/nmp-core/src/lib.rs | head -10
# all nmp_app_*, all Nostr nouns

$ grep -RE 'nmp_podcast_' crates/nmp-core/ crates/nmp-android-ffi/ apps/podcast/
(no matches)

$ git log --oneline | grep -iE "ios-podcast|podcast.*ffi|podcast/ffi"
(no matches)
```

## Impact

**Android (T157):** the `PodcastKernelModel.onAddPodcastPressed` handler
exists as a stub-with-log; pressing the CTA produces a logcat line but
no kernel dispatch. The Library list can never become non-empty until
this gap closes.

**iOS (M11 step 1):** the design doc `docs/design/podcast-app-rebuild.md`
§3 describes a `PodcastBridge.swift` using generated `@PodcastLibrary` /
`@NowPlaying` wrappers. The generated wrappers don't exist; the
`nmp-codegen` tooling has no podcast plugin; the iOS shell has no
project file at all (only Swift sources at
`ios/NmpPodcast/NmpPodcast/Views/`).

## Resolution path (suggested split)

Per `docs/design/app-extension-kernel.md` + `docs/ffi-surface.md`:

1. **Decide on the FFI shape.** Options:
   - **Generic dispatch** — extend the existing
     `nmp_app_dispatch_capability` to accept an enum tag for any
     `ActionModule` registered with the kernel, including podcast ones.
     Cleanest long-term; aligns with ADR-0010 generated aggregator.
   - **Per-action symbols** — `nmp_podcast_subscribe`,
     `nmp_podcast_play`, … one C entrypoint per action. Cheap to build;
     hard to extend.
2. **Land the chosen surface in `nmp-core`** (under the
   `android-ffi`/`ios-ffi` feature gates per the existing pattern)
3. **Mirror into iOS `NmpCore.h`** + Swift `PodcastBridge.swift`
4. **Mirror into Android Kotlin** `PodcastKernelBridge.kt` (new
   `nativeSubscribe(...)` external methods) — at that point the T157
   "Add podcast" CTA wires up end-to-end
5. **Document in `docs/ffi-surface.md`** + add a coordination note in
   the `docs/ffi-surface.md` change log so iOS + Android stay in step

Estimated scope: 4-8 hr depending on whether the generic dispatch path
is chosen (1-2 hr Rust + 2-3 hr each shell side) or per-action symbols
(quicker per action but N actions to author).

## Workaround (today)

The Android shell (T157) wires the CTA to a `Log.i` marker so QA can
verify the call path is reachable end-to-end (button → ViewModel
method). No kernel work happens, but the empty-state UI is correct and
the dispatch wiring is one method-body away from working.

## Cross-references

- Parent design: `docs/design/podcast-app-rebuild.md`
- FFI doctrine: `docs/ffi-surface.md`
- Generated-aggregator ADR: `docs/decisions/0010-app-aggregator-generated.md`
- Sibling: [`T-podcast-gap-1.md`](T-podcast-gap-1.md) (kernel integration)
