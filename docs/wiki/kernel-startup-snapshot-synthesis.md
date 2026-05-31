---
title: Kernel Startup Snapshot & Reactive Delivery
slug: kernel-startup-snapshot-synthesis
summary: The TUI must boot instantly using synthetic data and rely on the kernel's reactive snapshot delivery for updates
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:2175388c-275f-4b2e-b21b-6cc12a24f8de
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:47882225-939f-4978-bf5a-8feb9e5ef029
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Kernel Startup Snapshot & Reactive Delivery

## Startup Snapshot Synthesis

The TUI must boot instantly using synthetic data and rely on the kernel's reactive snapshot delivery for updates. The kernel MUST NOT wait on a subscription response, an EOSE, or any relay handshake before emitting its first snapshot. Blocking the UI on network activity is a framework defect. Data must be retrieved by the kernel reactively rather than prefetched during startup. The embed architecture complies with D8 by consuming kernel-pushed snapshots reactively with edge-triggered claim/release signals instead of polling, and follows component-owned reactivity where components signal their own data requirements via claim/release and the kernel never pre-fetches. App code must never handle 'waiting for connectivity' as a first-class state and must not implement timeouts around dispatch, subscription open, or snapshot subscription. The initial GalleryData must be constructed from synthetic data without any network calls. The WASM snapshot.rs must route snapshot synthesis through the canonical KernelReducer update path rather than hand-building a status-shaped snapshot independently. The kernel pushes a full JSON snapshot at 4 Hz via a C-ABI UpdateCallback into a shared Arc<Mutex<Option<Value>>> slot. An iced Subscription polls the shared snapshot slot every 250ms, draining the newest snapshot and refreshing the LiveProfileMap and EmbedHostState before incrementing a revision counter. The EmbedHostState reactive store replaces its entire envelope map from projections.claimed_events on each snapshot update. Components are stateless, immutable builder structs that are rebuilt from scratch on every frame from the latest app state, with no per-component signals, observers, or subscriptions. Serializing the full kernel snapshot to JSON four times per second is unmeasured under load and is the highest-risk source of potential UI jank. Desktop gallery components connect to the in-process kernel via spawn_actor and remain reactive through ActorCommand::ClaimEvent / ClaimProfile and EmbedHostState. SwiftUI holds a single `@Published var snapshot: KernelUpdate?` in an `@MainActor ObservableObject`, and every tick replaces the snapshot so SwiftUI diffs the entire view tree. V-87 D1 startup uses a pre-flight snapshot from a temporary bare kernel before the recv, then builds the real kernel post-recv with the correct storage path to preserve the storage-path init race. The V-87 rev-collision fix uses resume_rev_after_preflight(floor) to advance the real kernel so its Start frame carries rev=2 > 1, passing the iOS host's guard update.rev > rev.

<!-- citations: [^21753-1] [^594b7-4] [^9de49-6] [^54ae9-8] [^47882-1] [^38935-7] [^4edd4-11] -->
## See Also

