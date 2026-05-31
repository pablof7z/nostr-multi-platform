---
title: Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work
slug: chirp-cross-platform-parity-plan
summary: The Opus architect plan for making Chirp consistent across iOS, TUI, desktop, and Android by moving business logic into shared Rust crates and eliminating divergent shell paths.
tags:
  - chirp
  - cross-platform
  - parity
  - architecture
  - plan
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work

> The Opus architect plan for making Chirp consistent across iOS, TUI, desktop, and Android by moving business logic into shared Rust crates and eliminating divergent shell paths.

## Platform State (Honest Feature Matrix)

The cross-platform audit revealed the following parity matrix across iOS, TUI, Desktop, and Android:

| Capability | iOS | TUI | Desktop | Android |
|---|---|---|---|---|
| Timeline / publish / react / follow | ✅ | ✅ | ✅ | 🟡 read-only |
| DMs / Zaps / Wallet / Groups / Marmot | ✅ | ✅ | ❌ | ❌ |
| Bunker login / multi-account | ✅ | ✅ | ❌ | ❌ |
| Profile edit / outbox / notifications | ✅ | ✅ | ❌ | ❌ |

Android has zero write capability — `crates/nmp-android-ffi` has no `dispatch_action` JNI symbol. Desktop is read + basic-write only. iOS is the reference implementation; TUI is the only other near-complete shell. [^f3d8d-1]

## Root Causes — Three Structural Divergences

Three structural divergences cause four divergent platform paths instead of four thin renderings sharing a common Rust business-logic core:

1. **Action envelopes are hand-rolled per shell.** Every platform builds `json!({"PublishNote":…})` strings individually. The fix is a typed Rust client API on `nmp-app-chirp` so shells call `chirp.publish_note(content)`, not JSON literals.

2. **Snapshot structs are re-declared per shell.** Desktop has 18 local structs, TUI has its own parallel set, iOS has ~40 hand-rolled `Decodable`s. These should be public types in `nmp-app-chirp`, consumed directly by Rust shells and generated for FFI shells (F-05).

3. **Desktop is on a divergent transport.** Still decodes legacy JSON `ModularTimelineSnapshot`; TUI/iOS/Android moved to FlatBuffers + `RootFeedSnapshot` (V-80). Desktop never got the cutover. [^f3d8d-2]

## Ordered Work Items (v1-Relevant Subset)

The ordered work items, with the v1-blocking spine being A1 → A2 → A3 + B1 + B2 → C1:

| ID | Item | Effort |
|---|---|---|
| A1 | Typed Rust client API on `nmp-app-chirp` — backs all C-ABI symbols | M |
| A2 | Move snapshot types into `nmp-app-chirp` as public types; delete shell-local structs | M |
| A3 | Desktop → FlatBuffers transport + `ChirpTimelineSnapshot` (desktop arm of F-10) | M |
| B1 | Desktop + iOS consume `nmp-chirp-config` (kill inline `primal.net` hardcodes) | S |
| B2 | Android `nativeDispatchAction` JNI — prerequisite for ALL Android write parity | S |
| C1 | Android write baseline (compose/react/follow/sign-in via B2 door) | M |
| C2–C4 | Desktop + Android full feature parity | L |
| D1–D2 | iOS/Android codegen from A2 types (F-05) | post-v1 |

Full parity (C2–C4) and codegen (D1–D2) are post-v1. [^f3d8d-3]

## Codex Review — Plan Corrections

Codex reviewed the plan with code-grounded citations and identified several needed corrections before execution:

### Desktop Transport Correction
Desktop is already on FlatBuffers — `app.rs:54` calls `nmp_core::decode_update_frame`. The actual bug is subtler: it deserializes the result into its own local structs and renders `snap.items`, ignoring `nmp.feed.home`/NOFS OP-feed data (`app.rs:61`, `app.rs:283`). A3 was reframed as "desktop OP-feed/NOFS render cutover", not "desktop FlatBuffers transport."

### Confirmed Real Diagnoses
- Action envelopes: `runtime.rs:157`, `bridge.rs:191+221`, `KernelBridge.swift:323`
- Snapshot divergence: `chirp-desktop/snapshot.rs:24`, TUI `snapshot.rs:5` (`home_feed: Option<Value>`)
- Android no-dispatch: `KernelBridge.kt:90`, `nmp-android-ffi/lib.rs:30+108`

### Sequencing Corrections
1. Fix plan ownership first — temporal plans belong in `docs/plan.md` / `docs/BACKLOG.md` / `WIP.md` / `docs/plan/m*.md`
2. B1 early (desktop relay hardcodes) — tiny, real, independent
3. A3 reframed (desktop OP-feed render cutover) — can happen before A1/A2
4. A2 (shared snapshot types)
5. A1 (typed action facade) — but do NOT add per-verb C symbols; `docs/plan.md:125` freezes new bespoke FFI
6. B2 parallel with A1 — Android dispatch door is a total blocker for all Android writes [^f3d8d-4]

## Codex Review — Missed Gaps

Codex identified four gaps not captured in the original plan:

1. **Shared runtime/session duplication** — `chirp-desktop/bridge.rs:1` literally documents itself as mirroring TUI. A typed action API alone doesn't fix duplicated boot/register/start/drop/update-bridge boilerplate.

2. **Capability bridges** — TUI installs a keyring capability at `runtime.rs:62`; desktop and Android don't. This is a prerequisite for account persistence and write parity (`aim.md:52`).

3. **Android read/navigation parity ≠ write parity** — `openThread`/`openAuthor` are read operations that should be a separate item; they're currently buried in C1. Android today only exposes `openTimeline` (`KernelBridge.kt:29`).

4. **Acceptance tests under-specified** — `docs/plan/m15-cross-platform.md:32+37` already defines cross-platform consistency tests with byte-identical checkpoint snapshots. The plan should reference those gates per rung. [^f3d8d-5]

## What Shipped — Batch 1 (10 Parallel Haiku)

All tasks landed on master. Each Haiku agent ran in an isolated worktree; Sonnet reviewed and merged.

| Task | What Landed |
|---|---|
| B1 | Desktop relay config → `nmp-chirp-config` (no more hardcoded URLs) |
| B2+nav | Android `dispatchAction` + `openThread` + `openAuthor` JNI |
| desktop-profile-edit | Edit profile (publish kind:0) UI |
| desktop-switch-account | `switch_account` + `remove_account` bridge + settings UI |
| desktop-remove-relay | Remove relay button in relay editor |
| desktop-zap | ⚡ Zap button on note cards |
| desktop-dm | DM conversations infrastructure |
| desktop-bunker | NIP-46 bunker/nostrconnect login flow | [^f3d8d-6]

## What Shipped — Batch 2 (10 Parallel Haiku)

All tasks landed on master.

| Task | What Landed |
|---|---|
| android-write-baseline | Compose button + send, `openThread`/`openAuthor` wired in Kotlin UI |
| A1-typed-api | `ChirpClient` typed API in `nmp-app-chirp` (publish, react, follow, DM, zap, accounts) |
| desktop-wallet | NWC wallet connect/disconnect in desktop settings |
| ios-config-audit | Confirmed iOS relay defaults flow from Rust kernel |
| desktop-diagnostics | Routing & relay diagnostics tab |
| A2-shared-snapshot-types | `RelayStatus`, `ProfileCard`, `ActionResult` etc. as public types in `nmp-app-chirp` | [^f3d8d-7]

## What Shipped — Fix Batch (Sonnet + Haiku)

4 of 4 fixes merged. Complex tasks (A3, desktop-keyring) used Sonnet agents; simpler tasks (outbox, android-create-account-dispatch) used Haiku.

| Task | What Landed |
|---|---|
| A3-desktop-feed | Desktop feed cutover: `decode_snapshot_with_typed` + `nmp_nip01::OP_FEED_SCHEMA_ID` sidecar |
| desktop-keyring | OS keychain (`nmp_app_set_capability_callback`) wired into chirp-desktop |
| desktop-outbox | Outbox tab: publish retry/cancel (`nmp_app_retry_publish` / `nmp_app_cancel_publish`) |
| android-create-account-dispatch | Android `nativeCreateLocalAccount` migrated to dispatch door | [^f3d8d-8]


Account operations C-ABI fix: chirp-desktop bridge and nmp-android-ffi nativeCreateLocalAccount were both routing account lifecycle operations through dispatch_action, which silently failed because no ActionModule is registered for those namespaces. Both now call the bespoke C-ABI symbols (nmp_app_create_new_account, nmp_app_signin_nsec, nmp_app_switch_active, nmp_app_remove_account) directly. Android also fixed the relay format from [{url:…,role:…},…] to the correct [[url,role],…]. [^ecf13-11]
## Batch 3 — 15 Parallel Agents (In Progress)

Batch 3 scales up to 15 parallel agents across Android (8), Desktop (3), and Architecture (4):

### Android (8 agents)
- `android-model-updates` (Sonnet) — all KernelModel DM/relay/zap/account/follow methods (owns `KernelModel.kt`)
- `android-sign-in-screen` — new `ui/SignInScreen.kt`
- `android-relay-screen` — new `ui/RelayScreen.kt`
- `android-dm-screen` — new `ui/DmScreen.kt`
- `android-profile-screen` — new `ui/ProfileScreen.kt`
- `android-wallet-screen` — new `ui/WalletScreen.kt`
- `android-navigation` (Sonnet) — wires all screens into `MainActivity.kt` (owns that file)
- `android-zap` — adds ⚡ button to `TimelineScreen.kt`

### Desktop (3 agents)
- `desktop-dm-tab` — adds DM tab + `dm_panel()`
- `desktop-thread-author-ui` — thread + author view rendering
- `desktop-dm-register` — ensures DM inbox projection is registered in bridge

### Architecture (4 agents)
- `nmp-testing-cross-platform` — new parity smoke test in `nmp-testing`
- `A1-action-envelopes` — pure envelope builder fns in `nmp-app-chirp/typed_api.rs`
- `tui-use-shared-types` (Sonnet) — migrate TUI to use A2 types from `nmp-app-chirp`
- `desktop-use-chirpclient` (Sonnet) — migrate `chirp-desktop` bridge to use `ChirpClient` [^f3d8d-9]

## ChirpClient Typed API

The `ChirpClient` typed API struct lives in `nmp-app-chirp` and provides typed methods for all Chirp actions: publish, react, follow, DM, zap, and account operations. Shells call `chirp.publish_note(content)` instead of hand-rolling `json!({"PublishNote":…})` string literals. This backs all C-ABI symbols and eliminates the per-shell action-envelope hand-rolling that caused divergence. [^f3d8d-10]

## Shared Snapshot Types

Snapshot types (`RelayStatus`, `ProfileCard`, `ActionResult`, and others) are public types in `nmp-app-chirp`. Rust shells consume them directly; FFI shells (iOS, Android) will eventually get them via codegen. Shell-local duplicate structs (desktop's 18, TUI's parallel set, iOS's ~40 hand-rolled `Decodable`s) are deleted after migration. [^f3d8d-11]

## Desktop OP-Feed Render Cutover

Desktop is already on FlatBuffers transport (`app.rs:54` calls `nmp_core::decode_update_frame`). The actual bug is that it deserializes into local structs and renders `snap.items`, ignoring `nmp.feed.home`/NOFS OP-feed data. The fix uses `decode_snapshot_with_typed` + `nmp_nip01::OP_FEED_SCHEMA_ID` to extract the typed sidecar, following the TUI pattern. [^f3d8d-12]


The symptom of the pre-cutover desktop feed is: events are received (9 events shown in diagnostics) but 0 notes are rendered. This is because the desktop deserializes into local structs and renders snap.items, ignoring the nmp.feed.home / NOFS OP-feed typed sidecar. The fix — decode_snapshot_with_typed with OP_FEED_SCHEMA_ID — extracts the typed sidecar following the TUI pattern. [^ecf13-13]
## Plan File Location

The cross-platform plan was initially written to `docs/plan-chirp-cross-platform.md`. Per AGENTS.md planning discipline, temporal plans belong in `docs/plan.md` / `docs/BACKLOG.md` / `WIP.md` / `docs/plan/m*.md`. The plan's ownership location should be corrected to conform to this discipline. [^f3d8d-13]


Batch 3 — 15 Parallel Agents (In Progress)

## Batch 3 — 15 Parallel Agents (Completed)

Batch 3 scaled to 15 parallel agents across Android (8), Desktop (3), and Architecture (4). 11/15 merged on first pass; 4 failures were addressed in a fix batch (3/4 merged). The final desktop-chirpclient migration was fixed inline by the orchestrator.

### Android (7 of 8 merged)
- `android-model-updates` (Sonnet) — all KernelModel DM/relay/zap/account/follow methods in one agent (owns `KernelModel.kt`)
- `android-sign-in-screen` — new `ui/SignInScreen.kt`
- `android-relay-screen` — new `ui/RelayScreen.kt`
- `android-dm-screen` — new `ui/DmScreen.kt`
- `android-profile-screen` — new `ui/ProfileScreen.kt`
- `android-wallet-screen` — new `ui/WalletScreen.kt`
- `android-navigation` (Sonnet) — wires all screens into `MainActivity.kt` (owns that file). Fix round: wired all 5 new screens plus Diagnostics for 6-tab navigation.
- `android-zap` — ⚡ button on `TimelineScreen.kt`

### Desktop (2 of 3 merged)
- `desktop-dm-tab` — DM tab + `dm_panel()`
- `desktop-thread-author-ui` — thread + author view rendering
- `desktop-dm-register` — DM inbox projection registered in bridge. Fix round: verified `nmp_app_chirp_register_dm_inbox` is called and `dm_conversations` field is in `Snapshot`.

### Architecture (3 of 4 merged)
- `nmp-testing-cross-platform` — parity smoke test. First attempt was correctly rejected as vacuous (no production code called). Fix round produced real tests calling `publish_note_action()`, `react_action()`, etc. from production code and asserting on actual namespace + parsed JSON fields.
- `A1-action-envelopes` — pure envelope builder fns in `nmp-app-chirp/typed_api.rs`
- `tui-use-shared-types` (Sonnet) — migrated TUI to use A2 types from `nmp-app-chirp`
- `desktop-use-chirpclient` (Sonnet) — failed twice on `create_account` signature mismatch. Fixed inline by orchestrator: use pure free functions (`publish_note_action`, `react_action`, etc.) instead of `ChirpClient` field on `AppRuntime` to avoid raw pointer lifetime issue.

## Desktop ChirpClient Migration — Pure Free Functions Pattern

The `chirp-desktop` bridge migration to typed actions uses pure free functions from `nmp-app-chirp/typed_api.rs` (`publish_note_action`, `react_action`, etc.) rather than a `ChirpClient` struct field on `AppRuntime`. A `ChirpClient` field on `AppRuntime` creates a raw pointer lifetime problem because FFI callback registration stores raw pointers that must outlive the registration. Pure free functions have no state and avoid this issue entirely — shells call them to build action JSON and dispatch through the existing generic `dispatch_action` path.

<!-- citations: [^f3d8d-35] [^f3d8d-36] -->

What Shipped — Fix Batch (Sonnet + Haiku)

The `nmp-testing-cross-platform` task's first attempt was correctly rejected by the Sonnet reviewer as vacuous — the test asserted on literal JSON that wasn't calling any production code. The fix round produced real parity tests in `nmp-testing` that call production action builders (`publish_note_action()`, `react_action()`, etc.) and assert on actual namespace + parsed JSON fields. No more tautological literals. [^f3d8d-46]

Platform State (Honest Feature Matrix)

Desktop has a post-V80 projection backfill gap: KernelSnapshot top-level fields (active_account, profile, accounts, items) are always empty defaults because V-80 moved all data into the projections map. The timeline permanently renders Connecting to relays… until these fields are backfilled from projections after deserialization. [^ecf13-28]
## See Also
- [[chirp-client-typed-api|ChirpClient Typed API — Single Action Facade for All Shells]] — related guide
- [[shared-snapshot-types|Shared Snapshot Types — Public Types in nmp-app-chirp]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[op-centric-home-feed|OP-Centric Home Feed (V-80) — Architecture and Status]] — related guide
- [[cross-platform-qa-code-review-workflow|Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize]] — related guide
- [[chirp-cross-platform-feature-parity-testing|Chirp Cross-Platform Feature Parity — Mandated Testing Across All Clients]] — related guide

