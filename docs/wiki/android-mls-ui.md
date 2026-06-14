---
title: Android MLS UI
slug: android-mls-ui
topic: mls
summary: Android Marmot parity ops (leave, invite, remove, clear_pending) are thin dispatch shells with zero Kotlin protocol logic, available in the UI via typed seriali
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:b925f8c0-91f1-4d90-90d6-4a362bbaee79
---

# Android MLS UI

## Missing UI Operations

Android Marmot parity ops (leave, invite, remove, clear_pending) are thin dispatch shells with zero Kotlin protocol logic, available in the UI via typed serialized action envelopes.

<!-- citations: [^78c8e-13] [^78c8e-38] [^78c8e-60] [^78c8e-77] -->
## KernelModel.kt Split

KernelModel.kt must stay under 500 LOC; Marmot ops were extracted into MarmotActions.kt and MarmotActionEnvelopes.kt to clear the ceiling.

<!-- citations: [^78c8e-14] [^78c8e-39] [^78c8e-78] -->
## Marmot Op Sheet Behavior

iOS and Android shells stay open after Marmot op dispatch and dismiss only on terminal accepted, not on submission. MarmotOpResult retains the correlationId. Android's Marmot group dialogs never dismissed and the signer badge showed null because five projections (signer_state, action_lifecycle, action_stages, action_results, relay_diagnostics) had model fields in SnapshotProjections but no typed decoder and no Kotlin FlatBuffer binding on Android, silently resolving to null.

<!-- citations: [^78c8e-15] [^02745-21] [^78c8e-41] [^78c8e-61] [^02745-113] [^78c8e-94] -->
## KernelProfileHost Stability

Android `KernelProfileHost` must be stabilized by keying `remember(model)` only and threading the latest profiles map via `rememberUpdatedState`, so `DisposableEffect` in NostrAvatar/NostrProfileName does not key on `profileHost` (which changes identity on every snapshot tick when `rememberKernelProfileHost` uses `remember(model, profiles)` on a per-tick-fresh map, causing an infinite claim/release churn loop).

<!-- citations: [^02745-22] [^02745-114] -->
## Vendored Profile-Component Drift Gate

The Compose profile-component family is vendored under a byte-identical drift gate, so any fix must edit both the registry canonical files and the vendored Android copies. <!-- [^02745-23] -->

## Marmot Envelope Serialization

escapeJson is eliminated; all Marmot op JSON encoding uses kotlinx.serialization typed envelopes matching the Rust wire shapes byte-for-byte. Android Marmot action envelopes use @Serializable DTOs with @SerialName op discriminator and encodeDefaults=false/explicitNulls=false, matching Rust #[serde(default)] semantics.

<!-- citations: [^78c8e-40] [^78c8e-79] [^78c8e-95] -->
## Audit Issue #1303

Issue #1303 was filed for the two lower Android findings (DmConversationListScreen double-collect, ThreadScreen missing LocalProfileClaimer) from the #1294 audit. <!-- [^02745-75] -->

## Marmot Registration Gate

The "Create group" button for private (MLS) groups is disabled when the user is not registered with Marmot (i.e., no local nsec is active, as with bunker/NIP-46 connections), and displays the Marmot keyPackage subtitle as a hint below the disabled button. MarmotKeyPackage.empty.subtitle is set to "Sign in with an nsec to enable" (mirroring the Rust SUBTITLE_NOT_REGISTERED constant), rather than being left blank. <!-- [^b925f-1] -->

## Marmot UI Platform Divergences

Android leave action adds a confirm dialog that iOS lacks; iOS MarmotGroupChatView dispatches leave directly with no confirmation. Android MarmotMembersDialog wires per-member Remove, which iOS MarmotBridge exposes but does not wire into its read-only MembersSheet. <!-- [^78c8e-96] -->
