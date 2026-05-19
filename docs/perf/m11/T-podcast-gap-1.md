# T-podcast-gap-1 — podcast-core has no kernel integration

> **Surfaced by:** T157 step 1-2 (Android podcast APK pipeline)
> **Filed:** 2026-05-18
> **Status:** Open — blocks all M11 podcast iterations on Android and iOS

## Symptom

`apps/podcast/podcast-core` compiles. It defines:

- Domain records (`PodcastRecord`, `EpisodeRecord`, …) in `src/domain/records.rs`
- Action enums (`SubscribePodcast`, `UnsubscribePodcast`, `Play`, `Pause`, …)
  in `src/actions/mod.rs`
- View payload types (`LibraryView`, `FeedView`, `NowPlayingView`, …) in
  `src/views/mod.rs`

But **none of them are connected to the kernel.** Specifically:

- `nmp-core` has no `DomainHandle` registered for the podcast namespaces
  (the kernel's `DomainRegistry` does not see `PodcastRecord`/`EpisodeRecord`)
- No `ViewModule` is registered, so the kernel's snapshot does not include a
  `library` field — the JSON envelope `{"t":"snapshot","v":{…}}` has zero
  podcast keys today
- No `ActionModule` is registered, so dispatching `SubscribePodcast` from a
  shell (iOS/Android) has no effect — there's no handler
- No orchestrator wires the action chain (subscribe → fetch feed → parse →
  store episodes); the design exists in `docs/design/podcast/podcast-core.md`
  but isn't implemented

## Evidence

```bash
# podcast-core types exist and compile
$ cargo build -p podcast-core
Finished `dev` profile

# But no kernel wiring — no integration tests, no DomainModule impls
$ grep -RE 'DomainModule|ViewModule|ActionModule' apps/podcast/podcast-core/src/
(no matches)

# And the snapshot envelope has no library field
$ grep -RE 'library|podcasts' crates/nmp-core/src/kernel/ | grep -v test
(no matches)
```

## Impact

**Android (T157):** the Library tab renders the canonical "No Podcasts"
empty state but cannot transition out of it. Subscribing requires
`PodcastAction::SubscribePodcast` to dispatch, which requires an
`ActionModule` registration — blocked here.

**iOS (M11 step 1):** the Swift `LibraryView.swift` reads
`@Query(sort: \Podcast.title) private var podcasts: [Podcast]` from
SwiftData. Per `docs/design/podcast-app-rebuild.md` §1 the SwiftData
queries must be rewritten to read from the kernel's `LibraryViewModule`.
That rewrite is gated on this gap.

## Resolution path

Per `docs/design/podcast/podcast-core.md`:

1. Add `DomainModule<PodcastRecord>` + `DomainModule<EpisodeRecord>` impls
   that bind to a `DomainHandle::Mem` (LMDB comes later, per ADR-0012)
2. Add `LibraryViewModule` that projects `PodcastRecord` rows into the
   `LibraryView` payload — must register with the kernel's `ViewRegistry`
   so the snapshot envelope carries `library: {...}`
3. Add `ActionModule` impls for `SubscribePodcast`/`UnsubscribePodcast`
4. Add an integration test in `apps/podcast/podcast-core/tests/` that
   subscribes a podcast and observes the row appear in the view module
   snapshot

Estimated scope: 2-4 hr per the design doc.

## Workaround (today)

The Android shell (T157) renders the empty state honestly — no fake rows,
no Kotlin-side fallback data. When this gap closes, the snapshot starts
carrying `library: {podcasts: [...]}` and the existing
`PodcastKernelModel.decodeLibrary` picks it up automatically.

## Cross-references

- Parent design: `docs/design/podcast-app-rebuild.md`
- Component design: `docs/design/podcast/podcast-core.md`
- M11 plan: `docs/plan/m11-podcast.md`
- Sibling: [`T-podcast-gap-2.md`](T-podcast-gap-2.md) (FFI surface)
