---
title: iOS Logging
slug: ios-logging
topic: ffi-runtime
summary: kbLog.fault() with static string literals appears verbatim in Console.app (not as <private>), unlike NSLog with dynamic content or kbLog.info() which is filtere
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
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
---

# iOS Logging

## Console Visibility

kbLog.fault() with static string literals appears verbatim in Console.app (not as <private>), unlike NSLog with dynamic content or kbLog.info() which is filtered. <!-- [^fe79b-1] -->

Debug NSLog and print calls were stripped from KernelModel.swift, KernelBridge.swift, and OnboardingView+Components.swift hot paths. <!-- [^c4b2e-2] -->

Debug and history surfaces must use log-safe action tags and correlation ids; they must never record secrets, raw nsecs, plaintext DMs, or bearer tokens. <!-- [^019ed-22] -->
