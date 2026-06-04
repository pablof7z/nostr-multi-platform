---
title: iOS 26 Beta .onAppear & Rendering Defect
slug: ios-26-beta-onappear-defect
summary: On iOS 26 beta, .onAppear may not fire reliably before the first render, causing views that depend on an appeared state flag for visibility (such as .opacity(ap
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:72c67ab3-f61f-4546-a79c-17728a2a0f12
---

# iOS 26 Beta .onAppear & Rendering Defect

## iOS 26 Beta .onAppear Defect

On iOS 26 beta, .onAppear may not fire reliably before the first render, causing views that depend on an appeared state flag for visibility (such as .opacity(appeared ? 1 : 0)) to remain invisible. Adding .task as a fallback alongside .onAppear does not resolve this invisible rendering issue. Additionally, iOS 26 beta exhibits a systemic accessibility system issue where the accessibility tree reports 0x0 frames with no children, even though the app is rendering content visible in screenshots. [^72c67-3]

## See Also

