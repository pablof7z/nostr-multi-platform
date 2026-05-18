# Step 8 — M11 exit-gate verification

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Reference: [`docs/plan/m11-podcast.md`](../../plan/m11-podcast.md) "Exit gate (kernel boundary)" + "Exit gate (product fidelity)" + "Stress + perf gates".

Every M11 exit-gate bullet maps to a single, named evidence artifact. This doc is the cross-reference. The artifacts live under `docs/perf/m11/` and are produced by gates the design above has already specified.

---

## A. Kernel-boundary gates

| Plan bullet | Evidence artifact | Produced by |
|---|---|---|
| `nmp-core` gains zero podcast nouns | `docs/perf/m11/kernel-boundary.md` with the grep output | `grep -RE 'Podcast\|Episode\|Transcript\|Chapter\|Player\|Feed\|Insight\|Guest\|RSS\|Audio\|MP3' crates/nmp-core/src/` (whitelist the `audio_playback.rs` trait file) → expected empty |
| Capability families are general | `docs/perf/m11/capabilities-review.md` | Per-capability review against [`capabilities.md`](capabilities.md) §L — Request/Result must not name podcast nouns; a one-line reviewer signoff per capability |
| Reactivity behavior identical to Twitter slice | `docs/perf/m11/reactivity.md` | Re-run `reactivity-bench --standard --fail-on-gate` with `podcast-core` views registered alongside the existing nip01 views; assert all gates pass |
| No app-state leaks across the boundary in either direction | same as row 1 (kernel) + a sibling grep across `crates/nmp-core/src/` for `nostr\|relay\|nip` produces no hit inside `apps/podcast/` crates | the grep is added to CI |

---

## B. Product-fidelity gates

| Plan bullet | Evidence artifact | Produced by |
|---|---|---|
| UI parity: pixel diff ≤ 1 px per screen, font/rendering whitelisted | `docs/perf/m11/parity-screenshots.md` plus `docs/perf/m11/parity-screenshots/diff/*.png` (must be empty for all 20 screens) | `just screenshot-diff --fail-on-gate` (see [`screenshots.md`](screenshots.md)) |
| Feature parity: every flow reproduced as scripted agent run | `docs/perf/m11/feature-flows.md` listing each `Lane<N>_<surface>.swift` XCUITest pass | XCUITest runs in CI on each PR + nightly aggregate report |
| Subscribe to 10 real podcasts | `docs/perf/m11/subscriptions.md` with the 10 feed URLs + parsed metadata snapshot | Scripted: `just demo-subscribe-10` runs `SubscribePodcast` actions and dumps `PodcastRecord` JSON |
| Download an episode in background | `docs/perf/m11/background-downloads.md` with the simulated suspend timeline + resume evidence | XCUITest scenario `lane3-background-download.swift` |
| Play with background audio (lock screen) | `docs/perf/m11/background-audio.md` with screenshots of lock-screen controls + scripted seek/skip | manual test + simulator video recorded via `xcrun simctl io booted recordVideo` |
| Resume playback after kill-relaunch | covered in the lane-4 XCUITest scenario `lane4-resume-after-kill.swift` | same scenario asserts post-relaunch `NowPlaying.payload.current_s == prev_position_s ± 1` |
| Push notification on new-episode arrival | `docs/perf/m11/episode-push.md` with `xcrun simctl push` script + screenshot of the notification | scripted: refresh a feed where a new episode is injected; assert notification scheduled via `LocalNotificationCapability` |
| Ask a question via **rig.rs** against a real non-Apple-Intelligence LLM endpoint (M11 multiplatform proof); separately also via Apple Intelligence on-device | `docs/perf/m11/ask-streaming.md` with first-token-latency on both routes; rig.rs path is **required**, not optional — it is the proof that the architecture works off iOS | benchmarked: `just demo-ask-stream --route rig` AND `just demo-ask-stream --route apple-intelligence` both pass |
| Insights view generates structured summary on demand | `docs/perf/m11/insights-on-demand.md` showing the `RunInsight` action output structure | scripted scenario `lane5-run-insight.swift` |
| Guest enrichment populates guest cards | `docs/perf/m11/guest-enrichment.md` showing before/after `GuestRecord.bio` | scripted scenario `lane6-guest-enrich.swift` |

---

## C. Stress + perf gates

| Plan bullet | Evidence artifact | Produced by |
|---|---|---|
| 100 podcasts × 50 episodes scroll at 60 fps on iPhone 12 | `docs/perf/m11/feed-scroll-60fps.md` | Instruments Time Profiler trace at 60 Hz; assertion = no main-thread frame > 16 ms over 30 s of scrolling. Hardware required (iPhone 12). |
| Player UI updates every 250 ms without jank | `docs/perf/m11/player-tick.md` | scripted: `usePlayerSheet()` payload mutation rate measured against frame-time histogram |
| 20 concurrent downloads keep UI responsive | `docs/perf/m11/download-fanout.md` | scripted: queue 20 downloads, scroll the Library list during; main-thread frame budget ≤ 16 ms p99 |
| LLM ask: first token ≤ 1500 ms over Wi-Fi | `docs/perf/m11/ask-streaming.md` (shared) | measured on iPhone 12 + reference Mac dev machine |
| Full answer in ≤ 8 s for average episode | same | same |
| Battery drain ≤ Swift baseline + 10 % for 1 h BG playback | `docs/perf/m11/battery.md` | hardware iPhone 12, paired Mac with the Energy gauge; recorded on a paired test session — one hour each, reference vs rebuild, same audio file, charge delta |

---

## D. Required-tests inventory

```
ios/NmpPodcast/NmpPodcastUITests/
├── Lane1_Settings.swift
├── Lane2_Library.swift            (subscribe, refresh, unsubscribe, navigate)
├── Lane3_Feed.swift               (scroll, swipe-prioritize, swipe-delete)
├── Lane4_Player.swift             (play, pause, seek, skip, lock-screen, resume-after-kill)
├── Lane4_BackgroundAudio.swift
├── Lane4_AdSkip.swift
├── Lane5_Insights.swift           (capture, list, play-thought, play-excerpt, delete)
├── Lane6_Ask.swift                (suggestion, stream, citation tap)
├── Lane6_GuestAgent.swift         (enrich, ask)
├── Lane7_Discover.swift           (hero, trending, categories, search)
├── Cross_LedgerStress.swift       (20 concurrent downloads)
└── Cross_KillRelaunch.swift       (state persists)

crates/nmp-testing/tests/
├── action_subscribe_chain.rs
├── action_download_to_transcribe_chain.rs
├── action_insight_chain.rs
├── action_ask_streaming.rs
├── view_module_reactivity.rs
├── capability_audio_playback.rs
├── capability_background_work.rs
├── capability_local_notification.rs
├── capability_http_streaming.rs
├── capability_embedding.rs
├── capability_key_value_store.rs
├── capability_transcription.rs
├── capability_voice_recording.rs
└── capability_apple_intelligence.rs
```

Every test file ≤ 500 LOC. The cross-cutting kill-relaunch test is the most load-bearing — it asserts D8 (reactivity) and D4 (single writer per fact) hold under app termination at every state-transition boundary.

---

## E. CI binding

`.github/workflows/m11-gates.yml` adds three new jobs:

1. **`screenshot-diff`** — runs `just screenshot-diff --fail-on-gate` on a macOS-15 simulator host. Uploads diff PNGs as artifacts on failure.
2. **`kernel-boundary`** — runs the grep above; fails if any matches.
3. **`capability-review`** — runs `cargo run -p nmp-codegen -- validate-capabilities --capabilities-dir crates/nmp-core/src/substrate/capabilities/` — a new codegen subcommand that asserts request/result type definitions don't mention any podcast-domain noun (uses a curated wordlist + AST traversal).

All three are required for merge to `master`.

---

## F. Sign-off

M11 is **done** when:

- Every row in §A / §B / §C above has a green evidence artifact.
- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D8 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
- `cargo test --workspace` is green.
- `just screenshot-diff --fail-on-gate` is green.
- The release artifact `ios/NmpPodcast.app` runs on iPhone 12 hardware (build + install + the 20 scripted scenarios run).
- A tagged git commit `m11-podcast-app` on `master` represents the runnable artifact.

The M11 report at `docs/perf/m11/podcast-app.md` is the index pointing at all of the above, written last.
