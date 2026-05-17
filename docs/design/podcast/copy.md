# Step 0 — the copy step

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md) · sibling: [`inventory.md`](inventory.md).
> The single most important step. Locks the UI-fidelity invariant before any Rust is wired.

---

## 1. Why this step exists

The biggest threat to M11 is **gradual UI drift**: each port lane "improves" the Swift code in subtle ways (better naming, tighter view structure, "cleaner" gestures) until the rebuild no longer looks like the original. We prevent this structurally by **copying the views verbatim, committing, and adding screenshot diff gates from line 1**. Any subsequent change to a Swift view that breaks the screenshot diff requires explicit whitelisting in `docs/perf/m11/parity-screenshots.md` with a reason.

This is the same lesson the podcast-rmp project learned the slow way (cf. their `docs/plans/iphone-view-fidelity-restoration.md` — restoring fidelity *after* drift was a multi-week effort). We pay the upfront cost.

---

## 2. Protocol — `cp -R`

```bash
# Run once, in a fresh worktree.
SRC=/Users/pablofernandez/src/podcast
DST=/Users/pablofernandez/Work/nostr-multi-platform/ios/NmpPodcast

mkdir -p "$DST/NmpPodcast/Views" "$DST/NmpPodcast/Resources" "$DST/NmpPodcast/Bridge"

# Verbatim copy of the entire Views/ tree.
cp -R "$SRC/PodcastApp/Views/." "$DST/NmpPodcast/Views/"

# Resources: assets and sanitized Info.plist.
cp -R "$SRC/PodcastApp/Resources/Assets.xcassets" "$DST/NmpPodcast/Resources/"
# Info.plist is hand-curated (bundle id, capabilities — see §6) but visual keys
# preserved (font name, accent color, background modes).

# The two UI-only utilities (no business logic).
cp "$SRC/PodcastApp/Utilities/ErrorPresentation.swift" "$DST/NmpPodcast/Bridge/"
cp "$SRC/PodcastApp/Utilities/Logger.swift" "$DST/NmpPodcast/Bridge/"

# DO NOT copy: PodcastApp/{Models,Services,App,ViewModels}/* — those become Rust.

git add -A ios/NmpPodcast
git commit -m "ios/NmpPodcast: verbatim copy of ../podcast Views/ and Resources/"
```

After this single commit, the new app **does not compile**. Every Swift reference to `AudioService`, `ProcessingQueue`, `Settings.shared`, `@Query`, `@Model`, `RAGService`, etc., is a placeholder. The next commit's job is to make it compile against the `NMP-WIRE` shims.

---

## 3. The `// MARK: NMP-WIRE` placeholder pattern

Goal: a Swift compiler that succeeds with no business logic. Every reference into a missing Service/Model gets replaced **only at the immediate call site** by a placeholder marked with `// MARK: NMP-WIRE` so it's grep-able. A Rust call replaces the placeholder later; the SwiftUI body around it is unchanged.

### 3.1 Before (Swift in `EpisodeRow.swift`)

```swift
private var isPlaying: Bool {
    guard let audioService = audioService else { return false }
    return audioService.currentEpisode?.id == episode.id &&
           audioService.playbackState == .playing
}
```

### 3.2 After Step 0 (compiles, does nothing)

```swift
// MARK: NMP-WIRE
private var isPlaying: Bool {
    false  // wired in podcast-core::NowPlayingViewModule (see useEpisodeRow)
}
```

### 3.3 After Step 7 (wired)

```swift
// MARK: NMP-WIRE — wired
@StateObject private var row = useEpisodeRow(episodeId: episode.id)

private var isPlaying: Bool { row.payload.isPlaying }
```

### 3.4 Rules

- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
- Surrounding SwiftUI body shape is byte-identical to the source.
- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).

---

## 4. SwiftData decoupling

Every `@Query private var podcasts: [Podcast]` line in a view is the same shape; we replace each with a placeholder that returns an empty array. Example for `LibraryView.swift`:

```swift
// MARK: NMP-WIRE — TODO
@State private var podcasts: [PodcastRowPayload] = []  // wired via useLibrary()
```

The supporting `PodcastRowPayload` is a Swift struct in `ios/NmpPodcast/Bridge/Placeholders.swift` that mirrors what the generated wrapper will eventually deliver. Once Step 7 wires the view, the placeholder struct is deleted (the generated type from `nmp-app-podcast` Swift bindings replaces it).

`Placeholders.swift` is **intentionally** the only file that grows during Step 0; it is **deleted incrementally** as each view gets wired in Step 7. It must be empty at M11 exit.

---

## 5. Screenshot-diff harness — design

### 5.1 Tool

ImageMagick `compare` with `MAE` metric (mean absolute error per pixel, 0-65535). Threshold per screen recorded in `docs/perf/m11/parity-screenshots.md`.

### 5.2 Reference store

```
docs/perf/m11/parity-screenshots/
├── manifest.toml          # screen id, simulator config, threshold
├── reference/             # captured from ../podcast (the canonical Swift app)
│   ├── 01-feed-empty.png
│   ├── 02-feed-populated.png
│   ├── 03-library-empty.png
│   ├── 04-library-podcast-row.png
│   ├── 05-discover-trending.png
│   ├── 06-discover-search-result.png
│   ├── 07-podcast-detail-sheet.png
│   ├── 08-episode-detail.png
│   ├── 09-episode-row-downloading.png
│   ├── 10-mini-player-playing.png
│   ├── 11-player-sheet.png
│   ├── 12-chapters-panel.png
│   ├── 13-transcript-view.png
│   ├── 14-guest-agent-sheet.png
│   ├── 15-insights-empty.png
│   ├── 16-insights-card.png
│   ├── 17-ask-empty.png
│   ├── 18-ask-conversation.png
│   ├── 19-settings.png
│   └── 20-queue.png
├── candidate/             # captured from ios/NmpPodcast (the rebuild) on each run
└── diff/                  # generated by `compare`; non-empty pixels visualised
```

20 reference screens cover every UI surface in the Swift app (Discover has 5 sub-states; PlayerSheet has 3). Per AGENTS.md the screens are listed individually in `manifest.toml`, not enumerated in a wall-of-text.

### 5.3 Capture script (Rust)

`crates/nmp-testing/bin/screenshot-diff/main.rs` — drives the reference and the rebuild simulators in parallel via `mcp__xcode` and `xcrun simctl`, scripts the named scenario, captures `xcrun simctl io booted screenshot`, runs `compare -metric MAE ref.png cand.png diff.png`, emits a `manifest.toml`-driven pass/fail.

```rust
// pseudocode
for screen in manifest.screens {
    boot_sim(REFERENCE_SIM, screen.bundle_id_reference)?;
    drive_scenario(REFERENCE_SIM, &screen.scenario)?;
    snap(REFERENCE_SIM, &screen.reference_path)?;

    boot_sim(CANDIDATE_SIM, screen.bundle_id_candidate)?;
    drive_scenario(CANDIDATE_SIM, &screen.scenario)?;
    snap(CANDIDATE_SIM, &screen.candidate_path)?;

    let mae = run_compare(&screen.reference_path, &screen.candidate_path, &screen.diff_path)?;
    if mae > screen.threshold_mae {
        return Err(ParityError::AboveThreshold { screen: screen.id, mae });
    }
}
```

### 5.4 Threshold policy

- **Default threshold: 0** (byte-equal). Pristine pass.
- **Allowed drift category A: font-rendering** — explicit whitelist per screen, threshold ≤ 0.001 MAE (~1 in 1000 pixels off by 1 step). Required for SF Symbols glyph antialiasing variation across simulator builds.
- **Allowed drift category B: dynamic data** — screens that include `RelativeDateTimeFormatter` output ("2 min ago") are pinned to a fixed `Date()` via `simctl status_bar override` + a `--frozen-time` flag in the rebuild that the kernel reads as `now_ms_override`.
- **No other drift category accepted.** Every "we'll allow this one too" gets an entry in `parity-screenshots.md` with reason and reviewer.

### 5.5 When this runs

- Locally per developer via `just screenshot-diff`.
- Pre-merge CI on every PR that touches `ios/NmpPodcast/` or any `apps/podcast/` crate.
- Nightly against `iPhone 12` hardware in addition to the simulator.

### 5.6 What does NOT count as parity

- Pixel parity on splash / launch animations — out of scope (handled by Apple, not by us).
- Pixel parity on background blur (`MiniPlayer` uses `.ultraThinMaterial`) — measured at MAE ≤ 0.005, whitelisted.
- Pixel parity during in-flight animations — captures are taken in steady state with `--animation-disabled`.

---

## 6. `Info.plist` curation

We do **not** copy `Info.plist` verbatim. We copy the **visual** keys (`UIAppFonts`, accent color reference, launch storyboard reference) and re-author the **capability** keys for the rebuild:

- Bundle ID: `com.example.nmppodcast` (matches `apps/podcast/nmp.toml`).
- `UIBackgroundModes`: `audio`, `processing`, `fetch`, `remote-notification` (matches reference).
- `BGTaskSchedulerPermittedIdentifiers`: `com.example.nmppodcast.refresh`, `com.example.nmppodcast.processing` (new — the reference used no BG tasks).
- `NSMicrophoneUsageDescription`: copied from reference (Insights voice capture).
- `NSSpeechRecognitionUsageDescription`: copied from reference (transcription).
- `PODCAST_INDEX_API_KEY` / `_SECRET`: omitted from Info.plist (replaced by `KeyValueStoreCapability` read at boot, env-var fallback retained).

---

## 7. Acceptance for Step 0

- `cp -R` commit lands.
- `just gen-ios && just build-ios` succeeds (build only — app shows empty data everywhere).
- `just screenshot-diff --baseline-only` runs without errors and populates `docs/perf/m11/parity-screenshots/reference/` from `../podcast`.
- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.

---

## 8. Risks specific to this step

- **Asset-catalog UUID churn**: copying `Assets.xcassets` brings `Contents.json` files whose `info.author`/`info.version` may shift. Resolved by `git diff --stat` review — only `info.version` is allowed to drift.
- **Symlink trap**: `cp -R` follows symlinks; the reference app has no symlinks in `Views/`/`Resources/`, but verify with `find ../podcast/PodcastApp -type l` (must be empty).
- **Wallpaper / status-bar drift between simulators**: pinned via `simctl status_bar override` in the screenshot scenario.
- **Reference repo divergence**: the canonical Swift app may be edited after Step 0. The screenshot harness pins to a recorded `cp -R` snapshot under `ios/NmpPodcast/.reference-snapshot/` (gitignored) for re-capture; the snapshot points at a SHA in `../podcast` written into `parity-screenshots.md` so re-baselining is deliberate.
