---
title: Swift 6 @MainActor Protocol Conformance for KernelModel
slug: swift6-mainactor-protocol-conformance
summary: The `EventClaimSinkProtocol` must be annotated with `@MainActor` to resolve Swift 6 concurrency errors that arise when `@MainActor KernelModel` conforms to it.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-19
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:ec51ad49-af31-4415-aab4-e9123eb63eab
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
---

# Swift 6 @MainActor Protocol Conformance for KernelModel

## MainActor Annotation for EventClaimSinkProtocol

The `EventClaimSinkProtocol` must be annotated with `@MainActor` to resolve Swift 6 concurrency errors that arise when `@MainActor KernelModel` conforms to it. [^4edd4-233]


## NmpMediaRenderer Sendable Conformance

NmpMediaRenderer must be marked @unchecked Sendable since its closures are only ever called from @MainActor SwiftUI views. [^ec51a-14]

## KernelBridge Field Optionality

`KernelBridge.swift:226-227` must make `relayUrl` and `testNpub` optional to prevent decoder crashes when the kernel omits them. [^57528-26]

## Race-Free Active Nsec Availability

The Rust actor writes the active nsec to an `Arc<Mutex<Option<String>>>` slot synchronously before emitting identity-change snapshots, ensuring race-free availability when Swift's `apply()` runs. [^fe79b-15]

## KernelModel.apply() Threading Strategy

KernelModel.apply() must use DispatchQueue.main.async with MainActor.assumeIsolated for calling MainActor-isolated methods from the Rust listener thread callback, rather than Task { @MainActor }. [^fe79b-16]
## See Also

