---
title: Zap Send Flow
slug: zap-send-flow
summary: The zap send flow uses `KernelModel.zap()`, which returns a `DispatchResult` so callers can capture the correlation ID.  `HomeFeedView` stores the zap correlati
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-01
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:89070aba-0e77-4da3-99e1-322addb1c747
---

# Zap Send Flow

## Zap Send Flow

The zap send flow uses `KernelModel.zap()`, which returns a `DispatchResult` so callers can capture the correlation ID.

`HomeFeedView` stores the zap correlation ID in a `@State var pendingZapCid: String?` and observes `model.actionLifecycle` via `.onChange` to detect terminal stages. When the NWC payment completes and the action reaches the `.accepted` stage, the flow shows success feedback. When the action reaches the `.failed` stage, an error toast is shown. Success toasts use `UINotificationFeedbackGenerator().notificationOccurred(.success)` haptic feedback. <!-- [^89070-1] -->

## Toast Presentation

`KernelModel` exposes `lastSuccessToast` as a `@Published private(set) var String?` with `showSuccessToast()` / `clearSuccessToast()` methods. `RootShell.swift` renders the success toast as green with a 3-second duration, displayed above the existing error toast (gray, 4 seconds). <!-- [^89070-2] -->
