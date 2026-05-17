# Step 6 — side-by-side screenshot diff harness

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md). Companion to [`copy.md`](copy.md) §5.

This doc fully specifies the screenshot diff harness — the gate that turns "pixel parity" from aspiration into automation.

---

## 1. Architecture

```
┌──────────────────────────┐        ┌──────────────────────────┐
│  Simulator A (reference) │        │ Simulator B (rebuild)    │
│  bundle: com.podcast.app │        │ bundle: com.example.nmppodcast │
│  (../podcast)            │        │ (ios/NmpPodcast)         │
└────────────┬─────────────┘        └─────────────┬────────────┘
             │                                    │
             │ xcrun simctl io booted screenshot  │
             │                                    │
             ▼                                    ▼
  docs/perf/m11/parity-screenshots/    docs/perf/m11/parity-screenshots/
  reference/{screen_id}.png            candidate/{screen_id}.png
             │                                    │
             └───────────────┬────────────────────┘
                             │
                             ▼
            ImageMagick `compare -metric MAE`
                             │
                             ▼
            docs/perf/m11/parity-screenshots/diff/{screen_id}.png
            + manifest pass/fail line
```

---

## 2. Tool choice — `compare -metric MAE`

`compare` from ImageMagick is sufficient because:

- It returns a numeric metric per image (we use `MAE` — mean absolute error, 0..65535 at 16-bit depth).
- The diff PNG visualises mismatched pixels for human review.
- It runs locally and in CI with one binary; no GUI dependency.
- Threshold semantics are well-understood (1 step = ~0.015 % of a pixel value at 8-bit).

Rejected alternatives:

- **Snapshot Testing in XCTest** (`pointfreeco/swift-snapshot-testing`) — tied to the candidate app's test target, which adds build-graph weight; cannot compare two apps.
- **Apple's `xcrun simctl ui appearance`** — diff tools sit at the snapshot level, not the simctl level.
- **OpenCV `cv::PSNR`** — bigger dependency, no diff visualisation out of the box, identical results.

Threshold defaults in `manifest.toml`:

```toml
[default]
metric = "MAE"
mae_threshold = 0          # pixel-perfect
animation = "disabled"

[fonts]
mae_threshold = 0.001      # ≈ 0.1% pixel variance allowed for SF Symbols antialiasing
```

---

## 3. Screenshot harness — XCUITest target

**Automation surface: XCUITest, not MCP tools.**

The harness is a dedicated `NmpPodcastScreenshotTests` XCUITest target in `ios/NmpPodcast/NmpPodcastScreenshotTests/`. It drives both the reference app and the rebuild via standard XCTest APIs:

- **UI interaction**: `XCUIApplication`, `XCUIElement`, `tap()`, `swipeUp()`, `typeText()` — all standard XCUITest.
- **Screenshot capture**: `XCUIScreen.main.screenshot()` (returns `XCUIScreenshot`); raw PNG is written via `XCTAttachment` to the test result bundle, then extracted by the harness runner.
- **Diff step**: a shell script `scripts/compare-screenshots.sh` invokes `compare -metric MAE ref.png cand.png diff.png` (ImageMagick). This is the gating step — unchanged from the original design.

```swift
// ios/NmpPodcast/NmpPodcastScreenshotTests/ParityScreen.swift (excerpt)
class FeedEmptyParityTest: XCTestCase {
    func testFeedEmptyParity() throws {
        let app = XCUIApplication(bundleIdentifier: bundleId)
        app.launchArguments = ["--reset-state", "--pin-time", "2026-05-18T12:00:00Z",
                               "--disable-animations"]
        app.launch()
        app.tabBars.buttons["Feed"].tap()
        XCTAssertTrue(app.staticTexts["No Episodes"].waitForExistence(timeout: 10))
        let screenshot = XCUIScreen.main.screenshot()
        let attachment = XCTAttachment(screenshot: screenshot)
        attachment.name = "01-feed-empty"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
```

`xcrun xcodebuild test -scheme NmpPodcastScreenshots -destination '...'` produces the `.xcresult` bundle; `xcrun xcresulttool export attachments` extracts the PNGs. The same XCUITest structure runs against both `com.podcast.app` (reference) and `com.example.nmppodcast` (rebuild) in separate runs, driven by the `bundleId` parameter.

### Scenario manifest

The `manifest.toml` describes the screens and thresholds. Each screen entry maps to one `XCTestCase` method:

```toml
[[screens]]
id = "01-feed-empty"
title = "Feed tab — empty state"
reference_bundle_id = "com.podcast.app"
candidate_bundle_id = "com.example.nmppodcast"
simulator = "iPhone 16 Pro, iOS 26.5"
threshold_mae = 0
xctest_method = "testFeedEmptyParity"
```

### Animation suppression

`--disable-animations` launch arg → the target `AppDelegate` calls `UIView.setAnimationsEnabled(false)` at startup. Reduce Motion is also forced via `simctl status_bar override` before launch.

### Role of MCP tools (development-time inspection only)

`mcp__xcode__describe_ui`, `mcp__xcode__screenshot`, and sibling tools are **development-time inspection aids** used by engineers iterating on scenarios in Claude Code sessions. They are not part of the gate's automation surface and are never invoked in CI. The CI pipeline runs `xcodebuild test` only.

---

## 4. Fixture data

Reproducible screens need reproducible data. The fixture loaders:

- **Reference loader** (Swift code in a separate testing-only target): given a JSON fixture, `ModelContext.insert`s `Podcast`/`Episode`/`Transcript`/etc. SwiftData rows. Invoked via a custom URL scheme (`podcast://seed?fixture=lex-3-eps`).
- **Candidate loader** (Rust): a hidden `kernel.test.SeedFixture { json }` action wired only behind a `#[cfg(feature = "test-seeds")]` build of `podcast-core`. Same JSON.

JSON fixture format:

```json
{
  "podcasts": [
    { "id": "P1", "title": "Lex Fridman Podcast", "author": "Lex Fridman", "feed_url": "https://example/lex.rss", "artwork_url": "https://example/lex.png", "subscribed_at_ms": 1716000000000 }
  ],
  "episodes": [
    { "id": "E1", "podcast_id": "P1", "guid": "lex-456", "title": "Naval — The Art of Wealth", "audio_url": "https://example/E1.mp3", "duration_s": 8040, "published_at_ms": 1716086400000, "download_state": "Downloaded", "local_audio_path": "/cache/E1.mp3", "playback_position_s": 0, "has_been_played": false, "ai_summary": "Naval shares ..." }
  ],
  "transcripts": [],
  "chapters": [],
  "guests": [],
  "insights": []
}
```

Each screen pins a specific fixture (none / `lex-1-ep` / `lex-3-eps` / `multi-podcast-50-eps` / `with-insights` / etc.). Fixtures live in `docs/perf/m11/parity-screenshots/fixtures/`.

---

## 5. Run modes

```bash
# Capture reference baseline (one-shot, after Step 0 lands).
just screenshot-diff --baseline

# Compare candidate against reference (CI default).
just screenshot-diff

# Update candidate-only (developer flow, when iterating).
just screenshot-diff --candidate-only

# Single screen, with the diff opened in Preview.app.
just screenshot-diff --screen 11-player-sheet --open
```

CI fails on any screen with `mae > threshold_mae`. The diff PNG and both screenshots are uploaded as a job artifact.

---

## 6. Threshold-drift workflow

When a legitimate UI change requires a new baseline (rare; should be an ADR):

1. Developer runs `just screenshot-diff` and sees the failure.
2. Developer reviews the diff PNG — confirms the change is intentional.
3. Developer copies the new candidate over the reference for that screen.
4. Developer adds a note to `docs/perf/m11/parity-screenshots.md`:

```md
## 2026-05-22 — re-baseline `11-player-sheet`
**Reason**: Updated MiniPlayer corner radius from 16 → 18 px per design review.
**Reviewer**: @pablof7z
**Sources of truth diff**: `Views/Player/MiniPlayer.swift` line 122, `RoundedRectangle(cornerRadius: 18)`.
**ADR**: [adr-XXXX-mini-player-corner-radius.md](../../decisions/...)
```

This procedure means **no silent re-baselining**. Every drift has an owner, a reason, and an ADR.

---

## 7. Hardware vs simulator

The simulator covers ≥ 95 % of pixel-parity work because the rendering pipeline is identical. The remaining ≤ 5 % (display gamut, font hinting on physical glass) is covered by a nightly hardware run on iPhone 12:

```bash
just screenshot-diff --device "iPhone 12 (Wi-Fi)"
```

Per-device thresholds may be looser (≤ 0.002 MAE for fonts) and are recorded in `manifest.toml` `[devices.iphone12]`.

---

## 8. What we explicitly do NOT diff

- Splash / launch storyboard frames (Apple-owned).
- In-flight animations (we only snapshot steady state; the DSL waits for the relevant accessibility label).
- Modal sheet presentation animations (we wait until the sheet's primary text is visible).
- Native system pickers (date/time pickers, photo picker, document picker) — Apple owns these; we don't render them.
- TestFlight crash dialogs (obviously).
- Status bar live elements (time, battery, signal) — pinned via `simctl status_bar override`.

---

## 9. Where the harness implementation lives

```
crates/nmp-testing/bin/screenshot-diff/
├── main.rs              # CLI entry
├── manifest.rs          # parse manifest.toml
├── scenario.rs          # DSL interpreter
├── sim.rs               # xcrun simctl wrappers
├── compare.rs           # ImageMagick wrapper
├── fixtures.rs          # seed-fixture loader (talks to both apps)
└── report.rs            # human-readable summary
```

Per AGENTS.md each file ≤ 500 LOC. The CLI binary registers in `crates/nmp-testing/Cargo.toml`. CI runs `cargo run -p nmp-testing --bin screenshot-diff -- --manifest docs/perf/m11/parity-screenshots/manifest.toml --fail-on-gate`.

---

## 10. Initial 20 screens

| ID | Surface | Fixture | Notes |
|---|---|---|---|
| 01-feed-empty | Feed tab, no episodes | none | `ContentUnavailableView` rendering |
| 02-feed-populated | Feed tab, 5 episodes | `lex-3-eps` + `huberman-2-eps` | `EpisodeRow` rendering, sort order |
| 03-library-empty | Library tab, no podcasts | none | |
| 04-library-podcast-row | Library tab, 3 podcasts | `multi-podcast-3` | `PodcastRow` rendering |
| 05-discover-trending | Discover sheet, hero + trending | none + mock PI response | requires PodcastIndex mock |
| 06-discover-search-result | Discover sheet, search "naval" | mock search response | |
| 07-podcast-detail-sheet | PodcastDetailSheet for one result | mock | |
| 08-episode-detail | EpisodeDetailView with summary + 1 insight | `lex-1-ep-with-insight` | |
| 09-episode-row-downloading | EpisodeRow with active download job | seeded ledger row | tests active-job badge |
| 10-mini-player-playing | MiniPlayer overlay during playback | `lex-1-ep` + mock audio | |
| 11-player-sheet | PlayerSheet open, full | `lex-1-ep-with-chapters` | |
| 12-chapters-panel | ChaptersPanel open | `lex-1-ep-with-chapters` | |
| 13-transcript-view | TranscriptView with current-chunk highlight | `lex-1-ep-with-transcript` | |
| 14-guest-agent-sheet | GuestAgentSheet for one guest | `lex-1-ep-with-guest` | empty conversation |
| 15-insights-empty | Insights tab, no items | none | |
| 16-insights-card | Insights tab, 1 card | `lex-1-ep-with-insight` | |
| 17-ask-empty | Ask tab, empty state with suggestion chips | none | |
| 18-ask-conversation | Ask tab, mid-conversation with 1 source chip | `mock-rag-result` | |
| 19-settings | Settings sheet | default settings | |
| 20-queue | QueueView with active + completed jobs | seeded ledger | |

This list is the M11 acceptance set. Adding a screen is an ADR (cheap; usually a 1-line manifest entry plus a fixture).
