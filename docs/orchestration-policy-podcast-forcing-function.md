# Orchestration Policy: Podcast App as Forcing Function

> **Effective:** 2026-05-18 HB65
> **User directive (verbatim):** "I want to start moving towards getting the podcast application completely developed — let's prioritize having at least one agent working on it at all times and pushing to my iphone as frequently as possible, no hacks, no temporary bullshit, if things don't work properly we need to fix them in nmp, not fake shit or pack logic or state that belongs in NMP — let's make the opportunity of making NMP the best i can be by leveraging building the podcast app. As soon as possible, deploy to my iphone … and also deploy an agent to work on the android part of the podcast app -- always keep an agent working on it -- there should be minimal deviation between the two since all logic is supposed to live in the rust side. keep an apk built for me to test as soon as possible, always built, always uncommitted, with the version number (only one version, don't keep a million apks behind)"

## Standing rules

1. **Always at least 2 podcast-build agents in flight:** one iOS (Chirp-style FFI bridge, deploy to iPhone), one Android (cargo-ndk + JNI, build APK).
2. **Both targets must move together.** Rust-side fixes (kernel, capabilities, FFI surface) land for both; per-platform shells stay minimal.
3. **NO HACKS in Swift/Kotlin.** If a Rust gap blocks a feature, file a task and fix in Rust. Never pack logic or state into the platform shell that belongs in NMP. Never fake data.
4. **iPhone deploy as frequently as possible.** The iOS agent's primary deliverable each iteration is a build that runs on the physical iPhone (target device: pablo's iPhone, signed via `DEVELOPMENT_TEAM = 456SHKPP26`).
5. **Single-version APK kept uncommitted, always built.** Path: `android/app/build/outputs/apk/debug/podcast-debug.apk` (renamed from the default), regenerated on every Android agent commit. The agent script deletes any older `app-debug-*.apk` before rebuilding. The file lives outside source control (the `build/` dir is already `.gitignored`).
6. **Pixel parity** with `/Users/pablofernandez/src/podcast` (Swift) for iOS; native-Android polish acceptable for Android (no reference Android app exists).
7. **D0 invariant** stays absolute: NMP kernel gains zero podcast nouns. Verify at every commit.

## Agent identity & continuity

- iOS agent slug: `podcast-ios` — one rolling worktree-agent at a time. When one completes, immediately dispatch the next.
- Android agent slug: `podcast-android` — same pattern.
- Each agent reports its commits, surfaced NMP gaps as new TaskList items, and deploy-status (iPhone build/install OR APK regeneration + path).

## Task-list bookkeeping

When the orchestrator dispatches a podcast agent, it files (or updates) a single task per platform per iteration:
- `T-podcast-ios-N: <one-line scope>` (assigned to that agent ID)
- `T-podcast-android-N: <one-line scope>`

Surfaced NMP gaps become their own task IDs (e.g., `T-podcast-gap-N: <NMP fix needed>`) and feed back into the main NMP roadmap.

## Quality bar (no shortcuts)

- Production code compiles in `--release` mode for both platforms.
- File-size hook applies (300 soft / 500 hard).
- All new tests pass; no `#[ignore]` without an explicit linked task that promises to unblock.
- Each commit's body cites the affected user-facing scenario (e.g., "tap Library → see 10 subscribed podcasts").

## Schedule

The orchestration heartbeat (15 min cron) checks both podcast agents every cycle and re-dispatches if either has completed. Other parallel-orchestration work (T141, T154 doctrine sweep, the remaining roadmap phases) continues alongside, but the two podcast agents are floor-pinned.

## Termination condition

This policy ends when the iOS NmpPodcast app reaches feature parity with `/Users/pablofernandez/src/podcast` AND a fully-functional Android variant ships. Both verified by scripted Sonnet-agent runs over the canonical user flows.
