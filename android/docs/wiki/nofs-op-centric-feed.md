---
title: NOFS OP-Centric Feed Engine
slug: nofs-op-centric-feed
summary: "NOFS OP-centric feed engine: root cards, attribution, EventGate kind filtering, and relay echo handling"
tags:
  - nofs
  - feed
  - event-gate
  - kind-filter
  - root-indexed
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# NOFS OP-Centric Feed Engine

> NOFS OP-centric feed engine: root cards, attribution, EventGate kind filtering, and relay echo handling

## OP-centric feed model

The NOFS (Nostr OP-centric Feed System) home feed is OP-centric: it shows the thread root (the person who authored the original post) as the primary card, with attribution indicating who replied to or reposted it. For example, if fiatjaf replies to someone else's post, the card shows the original author with 'Replied by fiatjaf' underneath. [^855be-5]

## EventGate kind filter

The RootIndexedFeed engine includes an EventGate predicate in its Capabilities — a caller-supplied closure that determines whether an event is a feed-eligible kind. The gate runs at the very top of ingest(), before any state mutation. This prevents protocol-sourced non-feed events (kind:3 contacts, kind:10002 relay lists, and kind:0 profiles) from becoming phantom root cards when a relay echoes them back. The current NIP-01 wiring layer accepts only primary kind:1 notes and NIP-18-derived kind:6 repost wrappers. Profile kind:0 is intentionally excluded: profile acquisition and rendering are owned by mounted profile components through their own profile claims, not by the feed declaration. Kind:3 and kind:10002 remain blocked. [^855be-6]


The EventGate fix was delivered in PR #786 alongside the Android Diagnostics LazyColumn duplicate key fix, merged to master as commit 59874e5c. [^855be-21]
## Architecture D0 doctrine

The nmp-feed engine itself remains kind-agnostic (D0 doctrine). The EventGate is a caller-supplied predicate, passed in at construction time by the composition root (nmp-nip01's register_op_feed). The engine does not hardcode any kind logic; it only applies the gate the caller provides. This is the same pattern used by profile_detector. [^855be-7]

## EventGate type definition

EventGate is defined as Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>. It lives as a field in the Capabilities struct (between follow and event_lookup). The RootIndexedFeed::new() constructor accepts event_gate as a parameter and packages it into Capabilities. [^855be-8]

## Root card attribution

The RootCardRow in Android includes 'Replied by $label' attribution logic. If the attribution list is empty in the NOFS data, the label will not appear. The root event content may be empty if the original author's note was not fetched (because the subscription only covers followed users' events). [^855be-9]

## Relay echo and event storage

run_publish_engine never pre-stores events in the kernel store — events are only stored on relay echo. When kind:3 and kind:10002 are published during account creation and echoed back by relay.primal.net, they notify observers. Without the EventGate, the NOFS engine unconditionally calls self.ingest(event) on every KernelEvent from on_kernel_event. [^855be-10]

## See Also
- [[account-creation-autofollow|Account Creation & Autofollow]] — related guide
- [[cross-platform-architecture|Cross-Platform Architecture]] — related guide
