---
title: iOS Crash Log Retrieval & Debugging Tools
slug: ios-crash-log-retrieval
summary: Do not use the start_device_log_cap tool; it blocks the assistant irrecoverably
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
---

# iOS Crash Log Retrieval & Debugging Tools

## Device Crash Log Retrieval

Do not use the start_device_log_cap tool; it blocks the assistant irrecoverably. To retrieve device crash logs, use `xcrun devicectl device copy from --domain-type systemCrashLogs` instead of live log streaming. [^d27a4-5]


The `poll_inbox` function collects ingest errors into a Vec and returns them in the JSON result (with total_events count) so Swift can log them via structured os_log instead of using eprintln which is invisible in iOS structured log capture. The `nmpUpdateCallback` C function pointer closure uses `kbLog.fault()` with static string literals for diagnostics because NSLog (variadic) is unavailable in @convention(c) closures and print() output is suppressed on device in Console.app. [^fe79b-3]
## See Also

