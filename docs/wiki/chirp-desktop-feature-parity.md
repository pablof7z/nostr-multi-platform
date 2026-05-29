---
title: Chirp Desktop Feature Parity — What Landed and Remaining Gaps
slug: chirp-desktop-feature-parity
summary: "Desktop feature parity status: DM, zap, wallet, bunker login, profile edit, account management, relay management, diagnostics, outbox, keyring, and OP-feed cutover all landed."
tags:
  - desktop
  - chirp
  - parity
  - features
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# Chirp Desktop Feature Parity — What Landed and Remaining Gaps

> Desktop feature parity status: DM, zap, wallet, bunker login, profile edit, account management, relay management, diagnostics, outbox, keyring, and OP-feed cutover all landed.

## What Landed Across All Batches

Desktop gained the following features across the cross-platform parity push:


The chirp-desktop linter automatically reverts edits to app.rs if the build is not run immediately after the edit. To persist changes, the edit and build must be chained atomically in a single shell invocation. [^ecf13-35]

Desktop UI testing cannot use Xcode MCP screenshot tools — those require an iOS simulator UUID. For the egui-based desktop binary, use macOS screencapture or other native screenshot tools to inspect the running UI. [^ecf13-40]

Final commit history from this session: f63dcfda (PR #796: chirp-desktop file-based session + projection backfill) and 984599bb (Android direct C-ABI symbols fix, squashed from earlier commit b8615b07). The Android fix covers both the dispatch_action→bespoke symbols change and the relay format correction from object-array to tuple-array. [^ecf13-41]
### Batch 1
- **Relay config**: Desktop uses `nmp-chirp-config` — no more hardcoded relay URLs
- **Profile edit**: Edit profile (publish kind:0) UI
- **Account management**: `switch_account` + `remove_account` bridge + settings UI
- **Relay management**: Remove relay button in relay editor
- **Zap**: ⚡ Zap button on note cards
- **DM infrastructure**: DM conversations infrastructure
- **Bunker login**: NIP-46 bunker/nostrconnect login flow

### Batch 2
- **Wallet**: NWC wallet connect/disconnect in desktop settings
- **Diagnostics**: Routing & relay diagnostics tab

### Fix Batch
- **OP-feed cutover**: `decode_snapshot_with_typed` + `nmp_nip01::OP_FEED_SCHEMA_ID` sidecar
- **Keyring**: OS keychain (`nmp_app_set_capability_callback`) wired into chirp-desktop
- **Outbox**: Retry/cancel publish (`nmp_app_retry_publish` / `nmp_app_cancel_publish`) [^f3d8d-31]

## Runtime/Session Duplication

`chirp-desktop/bridge.rs:1` literally documents itself as mirroring TUI. A typed action API alone doesn't fix duplicated boot/register/start/drop/update-bridge boilerplate. This is a remaining gap identified by Codex. [^f3d8d-32]

## Capability Bridge Gap

TUI installs a keyring capability at `runtime.rs:62`; desktop and Android don't. This is a prerequisite for account persistence and write parity (`aim.md:52`). The desktop-keyring fix batch task addressed this for desktop by wiring `nmp_app_set_capability_callback`. [^f3d8d-33]


What Landed Across All Batches

Batch 3 added: DM conversations tab with `dm_panel()` UI, thread and author view rendering, and DM inbox projection registration in the bridge (ensuring `dm_conversations` field is in `Snapshot` and `nmp_app_chirp_register_dm_inbox` is called). [^f3d8d-49]

Account operations bridge fix: create_account, sign_in_nsec, switch_account, and remove_account now call the bespoke C-ABI symbols (nmp_app_create_new_account, nmp_app_signin_nsec, nmp_app_switch_active, nmp_app_remove_account) instead of the non-functional dispatch_action path. These operations were silently failing because no ActionModule is registered for the nmp.create_account / nmp.sign_in_nsec / nmp.switch_account / nmp.remove_account namespaces. [^ecf13-10]

Post-V80 projection backfill gap identified: chirp-desktop reads active_account, profile, accounts, and items from KernelSnapshot top-level struct fields, but these are all in the projections map after V-80. The fields are always empty defaults, causing the timeline to permanently render Connecting to relays… and the identity panel to show nothing. The fix requires backfilling these fields from the projections map after deserialization. [^ecf13-21]

Desktop session storage must use file-based storage matching chirp-tui's approach, not the OS keychain. The keychain path causes macOS to present a system access prompt on launch when a saved session exists, and the user explicitly directed that desktop use the same file-based storage as chirp-tui. [^ecf13-22]

Desktop UI testing cannot use the Xcode MCP screenshot tool — it requires a simulator UUID and is iOS-only. For the egui-based desktop binary, use macOS screencapture or other native screenshot tools instead. [^ecf13-26]
## See Also
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide
- [[chirp-ffi-boot-and-callback-lifetime|Chirp FFI Boot Sequence & Callback Object Lifetimes]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[account-operations-c-abi-symbols|Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action]] — related guide
- [[desktop-session-storage-file-based|Desktop Session Storage Must Be File-Based — Not OS Keychain]] — related guide
- [[desktop-kernel-snapshot-projection-backfill|Desktop KernelSnapshot Projection Backfill — Fields Are in projections, Not Top-Level]] — related guide

