---
title: ChirpTests XCTest Kernel Startup Bug — Auto-Boot Starves Test Runner
slug: chirp-tests-xctest-kernel-startup-bug
summary: The ChirpTests XCTest target was auto-booting the kernel, starving the test runner. Fixed with an XCTestConfigurationFilePath guard in ChirpApp.swift.
tags:
  - ios
  - testing
  - xctest
  - kernel
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# ChirpTests XCTest Kernel Startup Bug — Auto-Boot Starves Test Runner

> The ChirpTests XCTest target was auto-booting the kernel, starving the test runner. Fixed with an XCTestConfigurationFilePath guard in ChirpApp.swift.

## Overview

The ChirpTests target was auto-booting the kernel under XCTest, starving the test runner. When the XCTest runner launches, ChirpApp.swift initializes and starts the kernel — but the kernel's actor thread competes with the test runner for resources. This caused test hangs and timeouts. The fix adds an XCTestConfigurationFilePath guard in ChirpApp.swift that skips kernel initialization when running under XCTest. [^4edd4-95]

## Fix Location

The guard is placed in ChirpApp.swift, checking whether the XCTestConfigurationFilePath environment variable is set. When set (indicating XCTest is the launcher), kernel initialization and all background service startup is skipped. Unit tests that need the kernel must start it explicitly in their setUp phase. [^4edd4-96]

## See Also

