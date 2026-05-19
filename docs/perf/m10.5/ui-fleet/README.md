# M10.5 D4 — UI-Scripted Simulator Fleet (NmpStress)

- **Date:** 2026-05-18
- **Deliverable:** re-scoped M10.5 gate, D4 — every NmpStress user-visible
  surface exercised from the outside (boot, launch, tap, scroll, swipe,
  kill-relaunch).
- **Device:** iPhone 17 Pro simulator `5AC400C2-2ECB-4B1C-BEFE-A4AEB5B80F98`
  (iOS 26.2), Apple-Silicon host. No iPhone 16 Pro / iPhone 12 in this
  environment (current default = iPhone 17 Pro; original plan device names
  stale — see re-scope addendum). Hardware UI run deferred to the Pulse track.
- **Driver:** `mcp__xcode` (`simctl` launch/terminate/io, `tap`, `swipe`,
  `gesture`, `describe_ui`) against the live build linking the fresh
  `aarch64-apple-ios-sim` `libnmp_core.a`.
- **App under load:** live `wss://relay.primal.net` firehose throughout (real
  network, real events) — not a mocked fixture.

## Result: 9 / 9 PASS

| # | Scenario | FFI path exercised | Evidence | Result |
|---|----------|--------------------|----------|--------|
| F1 | Cold launch → Timeline renders under live relay | `nmp_app_new` → `start` → connect → snapshot callback | `F1-cold-launch-timeline.png` (CONNECTED, 292 ev, 80 visible, profile card) | **PASS** |
| F2 | Timeline scroll down/up | snapshot reconcile under scroll; view-handle churn | `F2-scroll.png` (list responsive, no stall) | **PASS** |
| F3 | Open author profile + back | `open_author`/`claim_profile` ↔ `close_author`/`release_profile` | `F3-author-profile.png` (ProfileDetailView, state ready, 14 notes) | **PASS** |
| F4 | Open thread + back | `open_thread` ↔ `close_thread` | `F4-thread.png` (ThreadDetailView, root + events bound) | **PASS** |
| F5 | Diagnostics tab | FFI diagnostics projection (relays / interests / wire subs) | `F5-diagnostics.png` (2 relays connected, interests `ref 1`, wire subs cycling) | **PASS** |
| F6 | Toolbar refresh → `resetAndRestart` | `stop` + `reset` + `start` (full FFI teardown+rebuild) | `F6-refresh-reset.png` (timeline reset + repopulated cleanly) | **PASS** |
| F7 | Toolbar start/stop toggle | `nmp_app_stop` / `nmp_app_start` | `F7-toggle-stop.png` (icon flipped ‖→▶ = stopped state confirmed) | **PASS** |
| F8 | Pull-to-refresh (`.refreshable`) | `resetAndRestart` via swipe-down overscroll | `F8-pull-refresh.png` (scrolled-to-top + reset, rev/events reset) | **PASS** |
| F9 | Kill + relaunch (cold-start resilience) | `simctl terminate` → `nmp_app_new` lifecycle from scratch | `F9-kill-relaunch.png` (fresh CONNECTED, state rebuilt) | **PASS** |

## Honest caveat (tooling, not app)

SwiftUI `.toolbar` `ToolbarItemGroup` buttons (`demo-refresh`, `demo-toggle`)
are **not surfaced in the `simctl` accessibility tree** — `describe_ui` reports
the Nav-bar group with **0 children** and no `AXButton` anywhere. F6/F7 were
therefore driven by screen-coordinate taps (refresh ≈ `(291,86)`, toggle ≈
`(366,89)`), verified by observable state change (F6 timeline reset; F7 icon
flip ‖→▶), not by accessibility-id targeting. This is a UI-automation tooling
limitation of `simctl`+SwiftUI-toolbars, **not** an app defect — every list
row, tab, and the timeline/diagnostics content *are* exposed by
`accessibilityIdentifier` and were targeted normally (F1–F5, F8). A first-class
XCUITest target (which can address toolbar buttons via the XCTest accessibility
API) is the durable fix and is routed to the Pulse track per the re-scope
addendum.

## Verdict

Every NmpStress user-visible surface — both tabs, all navigation
(author/thread push+pop), the diagnostics projection, both toolbar controls,
pull-to-refresh, and cold kill-relaunch — works under a live relay firehose on
the iPhone 17 Pro simulator. **9/9 PASS**, captured with screenshots.
iPhone-hardware UI run + XCUITest target deferred to the Pulse track.

*Cross-ref: `sim-baseline.md`, `doctrine-review.md`, `leak-evidence/`,
re-scope addendum.*
