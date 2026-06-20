---
title: Marmot Swift Bridge
slug: marmot-swift-bridge
topic: marmot
summary: MarmotStore accesses relayEditRows via a closure injected from KernelModel rather than directly on KernelHandle
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-21
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:3ed0a030-6daf-4680-9172-992f98deb328
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# Marmot Swift Bridge

## Architecture

MarmotStore accesses relayEditRows via a closure injected from KernelModel rather than directly on KernelHandle. NmpApp shares relay_edit_rows with MarmotProjection via an Arc<Mutex<Vec<RelayEditRow>>> handle pattern, mirroring the existing event_observers/raw_event_observers slots. ActorCommand::Reset preserves relay_edit_rows_handle across kernel re-creation. Kernel::set_relay_edit_rows syncs relay_edit_rows to the shared Arc<Mutex> handle so that NmpApp::write_relay_urls sees current data. The Rust→Swift callback pipeline uses DispatchQueue.main.async with MainActor.assumeIsolated instead of Task { @MainActor } to call apply() from the listener thread. Marmot's publish_signed_event_to dependency uses an internal kernel API (Kernel::publish_signed_explicit), migrating off the FFI dance.

<!-- citations: [^fe79b-3] [^27a9c-5] [^3ed0a-2] [^1c093-25] -->
## Relay Resolution

Marmot publish_key_package and create_group resolve write relays from kernel relay_edit_rows on the Rust side, not from a relays parameter passed from Swift. Swift MarmotBridge no longer passes a relays parameter in the publish_key_package or create_group dispatch JSON; the defaultRelays property was removed. Relay resolution logic must be Rust-owned exclusively; Swift is only responsible for dispatching button taps. <!-- [^3ed0a-3] -->


PR #207 added a Marmot runtime guard refusing kind:1059 dispatch when relays is empty, and extended D10 banned list to include publish_signed_event. <!-- [^1c093-26] -->
## Write Relay Filtering

NmpApp::write_relay_urls filters relay_edit_rows for rows with role 'write' or 'both' (case-insensitive) and returns their URLs. has_role is case-insensitive and treats 'indexer' as semantically including 'write'. normalize_role accepts 'indexer' as a valid relay role. <!-- [^3ed0a-4] -->

## Key Package Publishing

The publish_key_package operation uses publish_explicit with the relays resolved from relay_edit_rows, not publish_author_outbox (which routes through NIP-65 Auto and fails with 'no write-relays declared' when kind:10002 is absent). <!-- [^3ed0a-5] -->

## UI Behavior

MarmotGroupsView shows 'Sign in with an nsec to enable Marmot encrypted groups' when store.isRegistered is false, and MarmotKeyPackageRow disables the publish button with the same message. <!-- [^3ed0a-6] -->

## Default Relays

On the Swift side, KernelModel seeds wss://r.f7z.io (both roles) and wss://purplepag.es (indexer role) before kernel.start() if relayEditRows is empty. MarmotBridge must not hardcode relay arrays such as [damus, nos.lol, primal]; it must read from the app's relay config. The Chirp thin-shell rule requires zero DOMAIN logic in Swift (aim.md §2 #4); ephemeral presentation state is permitted, but only C-ABI symbols may make protocol/domain decisions — those must delegate to NMP crates.

<!-- citations: [^fd809-1] [^fe79b-5] -->
## Swift Interop

Swift tuples [(String, String)] cannot be passed to NSJSONSerialization; they must be converted to [[String]] via .map { [$0.0, $0.1] } before serialization. <!-- [^fe79b-4] -->
