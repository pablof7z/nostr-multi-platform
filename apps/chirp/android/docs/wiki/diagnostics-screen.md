---
title: Android Diagnostics Screen
slug: diagnostics-screen
summary: "Android Diagnostics screen: relay status display and LazyColumn key uniqueness"
tags:
  - android
  - diagnostics
  - relay
  - compose
  - lazycolumn
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# Android Diagnostics Screen

> Android Diagnostics screen: relay status display and LazyColumn key uniqueness

## Relay status listing

The Diagnostics screen displays relay status for each RelayRole. relay.primal.net is registered as 'both,indexer', which means it serves both the Content role and the Indexer role. The relay_statuses() function in kernel/status.rs generates status entries for each role separately, causing relay.primal.net to appear twice — once for Content and once for Indexer. [^855be-11]


The Diagnostics screen is accessed via a button positioned at the bottom-left of the app. [^855be-23]
## LazyColumn keying

The LazyColumn in DiagnosticsScreen.kt must use unique keys for each item. When the same relay URL appears for multiple roles, using key = { it.relayUrl } causes a duplicate key crash. The fix keys on '${role}:${url}' instead, making each role+URL combination unique. [^855be-12]


This fix was delivered in PR #786 alongside the NOFS EventGate kind filter, merged to master as commit 59874e5c. [^855be-20]
## See Also
- [[cross-platform-architecture|Cross-Platform Architecture]] — related guide

