# T156 Step 1 — Deployment evidence

## Device

- Name: Pablo's iPhone
- UDID: `3C438D9B-2021-5A30-93DB-910F7754F9A2`
- Model: iPhone 17 Pro Max (`iPhone18,2`)
- State: available (paired)
- Discovery: `xcrun devicectl list devices` shows the device under section
  "Pablo's iPhone — available (paired)".

## Build

- Artifact path: `/tmp/NmpPodcast-DD-Device/Build/Products/Debug-iphoneos/NmpPodcast.app`
- Bundle id: `com.podcast.app`
- Signed with DEVELOPMENT_TEAM: `456SHKPP26` (Pablo's team)
- Linked static archives (per project.yml `OTHER_LDFLAGS`):
  - `libnmp_app_podcast.a` — per-app FFI (T156)
  - `libnmp_signer_broker.a` — NIP-46 bunker:// hook
  - `libnmp_core.a` — kernel ABI
- Build commands:
  ```
  cargo build --release -p nmp-core -p nmp-app-podcast -p nmp-signer-broker \
              --target aarch64-apple-ios
  xcodegen generate --spec ios/NmpPodcast/project.yml
  xcodebuild -project ios/NmpPodcast/NmpPodcast.xcodeproj \
             -scheme NmpPodcast \
             -destination 'generic/platform=iOS' \
             -derivedDataPath /tmp/NmpPodcast-DD-Device build
  ```

## Install + launch

- Install: `xcrun devicectl device install app --device <UDID> <path-to-.app>`
  - First install at 2026-05-18 19:24:21 local — installed at
    `/private/var/containers/Bundle/Application/36C17837-57E0-4120-9243-D5FF735FB04F/NmpPodcast.app/`
- Launch: `xcrun devicectl device process launch --device <UDID> com.podcast.app`
  - First launch at 2026-05-18 19:24:29 local. Process listed at PID 16430.
  - Second launch at 2026-05-18 19:31:34 local (after a device-log-capture
    session closed the first instance). Process listed at PID 16444.
- Both PIDs were verified live via
  `xcrun devicectl device info processes --device <UDID>` — output line:
  ```
  16444   /private/var/containers/Bundle/Application/36C17837-57E0-4120-9243-D5FF735FB04F/NmpPodcast.app/NmpPodcast
  ```
- The app remains installed and runnable on Pablo's iPhone for live
  visual verification by the user.

## Screenshot status

No screenshot is included in this iteration.

- `xcrun devicectl` does not expose a screenshot subcommand for physical
  devices.
- `libimobiledevice` / `idevicescreenshot` is not installed on this host
  (`brew list` returned no `libimobiledevice` package; `which
  idevicescreenshot` empty).
- The Xcode MCP `screenshot` tool requires a simulator UUID, not a
  physical-device UDID.

The deploy is real (PID 16444 currently running on the iPhone) and the
user can verify the visual state directly. A future iteration will land
`libimobiledevice` in the agent toolchain so the screenshot becomes
part of the deploy artifact.

## User-facing scenario now live on Pablo's iPhone

1. Open NmpPodcast → 4-tab `TabView` (Feed / Ask / Insights / Library).
2. Library tab → empty `ContentUnavailableView` "No Podcasts" with caption
   "Subscribe to podcasts to build your library."
3. Tap toolbar `+` (accessibilityIdentifier `addPodcastButton`) → Add
   Podcast sheet appears with feed URL field + optional title / author
   fields.
4. Enter feed URL (e.g. `https://feeds.megaphone.fm/lex-fridman`) plus
   optional metadata → tap Add (accessibilityIdentifier
   `addPodcastConfirm`) → row appears in the kernel-backed Library list.
5. Swipe-to-delete a row → kernel removes the subscription and the next
   snapshot omits it.
6. Other tabs (Feed / Ask / Insights) render `ContentUnavailableView`
   placeholders until their respective `ViewModule` lands (filed as
   T-podcast-gap-002).

## Kernel-boundary verification (D0)

```
$ grep -RnE 'Podcast|Episode|Transcript|Chapter|Player|Feed|Insight|Guest|RSS|Audio|MP3' \
       crates/nmp-core/src/ | grep -v audio_playback.rs
$ # (no output)
```

Zero matches. The kernel stays podcast-noun-free. The full M11 keyword
set is verified, including `Player|Feed|Insight|Guest` that the initial
sweep missed.

## Data path proof

Every byte rendered by the Library list crosses the FFI boundary:

```
Swift                                         | Rust
──────────────────────────────────────────────┼──────────────────────────────────
User taps Add → KernelModel.subscribe(url)    |
  → KernelHandle.podcastSubscribe              |
    → nmp_app_podcast_subscribe(handle, url,   →  state::PodcastApp::subscribe
       title?, author?)                        |    pushes PodcastRecord onto
                                               |    Mutex<Vec<_>>
KernelModel.refresh                            |
  → KernelHandle.podcastSnapshot               |
    → nmp_app_podcast_snapshot(handle)         →  state::PodcastApp::snapshot
                                               |    serializes podcast_core::
                                               |    views::LibraryView as JSON
  ← JSON                                       ←  CString::into_raw
JSONDecoder<LibrarySnapshot>                   |
  → @Published library                         |
    → SwiftUI re-render of Library list rows   |
```

No Swift-side state. No SwiftData. No service shims. The `PodcastRecord`
domain type lives in `podcast-core::domain::records`; the kernel knows
nothing about it.

## Filed follow-ups

A `TaskCreate` tool is not surfaceable in the current agent tool list
(only `TaskStop` exists). The four gap-tasks the brief asks for are
documented here as a fallback for the orchestrator to land in TaskList
on the next heartbeat:

- **T-podcast-gap-001** — Verbatim-view restoration. The 27 view files
  under `ios/NmpPodcast/NmpPodcast/Views/` (copied verbatim from
  `/Users/pablofernandez/src/podcast/PodcastApp/Views/` in an earlier
  iteration) reference SwiftData `@Query`, `@Model Podcast`,
  `AudioService`, `ProcessingQueue` — none of which exist in
  NmpPodcast. M11 forbids them. Two paths: (a) generate Swift shim
  types that proxy kernel snapshot rows and re-export the SwiftData /
  observable surface, or (b) ship the `nmp gen modules` generator
  referenced by `docs/design/podcast-app-rebuild.md` §1.
  `nmp-codegen` today only crawls descriptors; the generator that
  emits per-view Swift wrappers is not implemented.
- **T-podcast-gap-002** — Feed / Ask / Insights tabs are
  `ContentUnavailableView` placeholders. Each tab wires up once its
  respective `ViewModule` is implemented behind the kernel:
  `FeedViewModule`, `AskQuestion` action + `AskViewModule`,
  `InsightsViewModule`.
- **T-podcast-gap-003** — `podcast-feeds` RSS / Atom / JSON Feed /
  Podcasting 2.0 parsing plus a `SubscribePodcast` action chain that
  auto-populates `title` / `author` / `artwork_url` /
  initial episode list on subscribe. Until then `AddPodcastView`
  requires manual metadata entry.
- **T-podcast-gap-004** — Domain-store persistence. The library
  currently lives in `Mutex<Vec<PodcastRecord>>` inside
  `nmp-app-podcast`. Persistence-by-domain-store (LMDB-backed) is the
  M11 design target; right now `cargo test --features lmdb-backend`
  exists but the kernel actor doesn't yet drive `ActionModule::start`
  / `reduce` through it. The chirp pattern uses
  `KernelEventObserver` (a runtime observer) for projections; the
  podcast equivalent will hook a domain store the same way.
