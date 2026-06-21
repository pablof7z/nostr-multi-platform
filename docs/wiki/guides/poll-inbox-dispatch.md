---
title: Poll Inbox Dispatch
slug: poll-inbox-dispatch
topic: marmot
summary: "Polling is forbidden at every layer of the stack: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task loops with"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-06-18
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:cb671af9-5784-4174-9c3d-d10151d9fb01
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc10-1fb3-7752-ab3e-7f5b969da686
---

# Poll Inbox Dispatch

## Poll Inbox Dispatch

Polling is forbidden at every layer of the stack: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task loops with sleep and checkState. Blocking primitives or event-driven patterns must be used instead: Rust channels must block with recv()/recv_timeout() or drain with try_recv() (not in a sleep loop); iOS must consume ViewBatch snapshots pushed by the kernel and use AVFoundation/NWPathMonitor/NotificationCenter callbacks for OS events; background persistence must piggy-back on an existing event tick with a wall-clock gate, not spawn a parallel sleep loop; cache-serve wakeups are event-driven, not polled. Cache invalidation and store wakeups use event-driven notification/replay rather than polling. Zero production polling violations exist; thread::sleep calls are confined to test fixtures and relay worker infrastructure. DispatchQueue.main.asyncAfter wait-then-check kludges (ThreadScreen.swift:135, ComposeView.swift:55, DmConversationView.swift:80-81) violate the no-polling rule; the correct pattern is snapshot-driven .onChange observers. Concrete examples of the mandated patterns include: nmp-signer-broker/relay_client.rs uses try_recv() instead of a recv_timeout(0ms) drain loop; nmp-signers/nip46/handle.rs uses SignerOp::wait() instead of a poll() + sleep(10ms) loop; nmp-repl/fanout.rs uses blocking recv() instead of a try_recv() + sleep(50ms) worker spin; NetworkSettingsStore.swift uses an applyStatus() event hook only instead of a 2-second task polling loop for refreshDiagnostics(); BookScannerModel.swift accumulates data in a metadata delegate with a 500ms debounced clear instead of a 0.25s Timer polling detectedBoxes; and PodcastPlayerStore.swift uses a wall-clock gate inside an existing 0.25s time observer instead of a 5-second sleep loop for position persistence.

<!-- citations: [^fe79b-6] [^c4b2e-6] [^cb671-1] [^1c093-28] [^45fcf-12] [^129d2-12] [^019ed-13] [^019ed-53] [^129d2-128] -->
## Dual-Path Publishing

The dual-path publish in publish_key_package sends key package events both through the kernel's fire-and-forget publish AND via direct WebSocket send_event(), because the kernel publish was silently dropping events in the iOS simulator. <!-- [^fe79b-7] -->

## Test Scaffold and Production Delivery

MLS group creation and messaging must work via the MLS REPL with zero polling. A full Marmot MLS round-trip was proven: Alice (nmp-repl) sent 'hello chirp! this is alice speaking from the repl' to ChirpLiveTest4 group, and Bob's Chirp app received and decrypted it via the event-driven tap path (badge appeared without explicit poll).

<!-- citations: [^fe79b-8] [^c4b2e-7] -->
## Event-Driven Welcome Delivery

Event-driven Welcome delivery uses ActorCommand::PushInterest(LogicalInterest) to register a tailing kind:1059 #p:<pubkey> subscription scoped to the user's NIP-65 inbox relays, so Marmot Welcomes arrive automatically without Swift-side polling. The giftwrap inbox interest ID is a deterministic hash of 'marmot.giftwrap' + pubkey, following the same pattern as follow_feed_interest_id in contacts.rs. <!-- [^fe79b-9] -->

## Anti-Polling Doctrine and Enforcement

The no-polling rule is codified across multiple repo documents to ensure it is visible to any agent or contributor. AGENTS.md contains a top-level 'No polling — ever' section covering all three layers with concrete patterns. The anti-pattern section in 06-reactivity-contract.md explicitly covers all layers (Rust, iOS, test helpers), not just UI-to-kernel polling. The D8 row in 03-doctrine-d0-d8.md explicitly lists sleep+poll loops as forbidden alongside allocations and false wakes. A memory note (feedback_no_polling.md) is indexed in MEMORY.md so future conversations start with the no-polling rule already loaded. <!-- [^cb671-2] -->
