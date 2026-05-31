---
title: iOS KernelBridge & KernelModel Divergence
slug: ios-kernel-bridge-divergence
summary: The three iOS apps (Chirp, NmpPulse, NmpStress) each carry separate diverging copies of KernelBridge.swift and KernelModel.swift with no shared Swift package.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:42252c03-76ca-449c-9cfd-ed5949b2bb9d
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# iOS KernelBridge & KernelModel Divergence

## Diverging Kernel Bridge Copies

The three iOS apps (Chirp, NmpPulse, NmpStress) each carry separate diverging copies of KernelBridge.swift and KernelModel.swift with no shared Swift package. KernelTypes.generated.swift is a dev-time generated file that lives in git and has precedent for manual field additions. iOS Swift bridges register capabilities and projections with the Rust kernel via FFI using one-off registration functions calling `nmp_app_chirp_register_*` C-ABI symbols. KernelModel is a pure mirror of the kernel snapshot and must never use SwiftUI-derivable logic; however, it currently contains 15 @Published projections constituting the biggest thin-shell violation. It must collapse to one @Published snapshot: KernelSnapshot with computed getters, eliminating roughly 400 LoC. KernelModel must conform to the NostrProfileHost protocol, providing profile(forPubkey:) reading the claimed_profiles projection. In KernelBridge.swift, the relayUrl and testNpub fields are declared as non-optional, creating a decoder crash risk if the kernel omits them. Additionally, KernelBridge.swift:172-175 uses four force-try (try!) calls in createAccount, causing a crash on any JSON hiccup. KernelBridge.swift:244-256 discards the correlation_id from the dispatch_action return, introducing 100-200ms lag on every user action. KernelBridge.swift:570 parses correlation_id first and treats the envelope as accepted, ignoring the error field, making sync-Err vs panic-recorded-Failed look identical to the host. DispatchQueue.main.async is not a SwiftUI render barrier; the real fix for the render race requires a view-driven ACK protocol. Combine `.sink` subscribers on individual properties are explicitly forbidden in tests because they cause use-after-free with the long-lived shared kernel. claimProfile/releaseProfile and claimEvent/releaseEvent FFI are wired in KernelBridge but currently have zero call sites in any Swift view; every view rendering a pubkey must call claimProfile on appear and releaseProfile on disappear, and every view rendering an event must call claimEvent on appear and releaseEvent on disappear. Swift codegen should expand from the ActionOutcome pilot to TimelineBlock Swift Decodables to reduce KernelBridge.swift. Building the Rust library for the iOS simulator requires running just rust-ios-sim before rebuilding in Xcode, otherwise the app runs the old binary.

<!-- citations: [^42252-4] [^09da8-3] [^57528-7] [^cc7dc-4] [^1c093-8] [^2c4ad-3] [^86221-5] [^54ae9-6] [^9a2c7-11] [^38935-6] -->
## See Also

