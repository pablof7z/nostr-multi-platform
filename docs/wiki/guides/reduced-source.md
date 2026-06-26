---
title: ReducedSource and Dynamic Feed Composition
slug: reduced-source
topic: reduced-source
summary: ReducedSource is a kernel-owned primitive that registers a source interest, runs a deterministic reducer on its results, diffs the output against prior reductio
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-26
updated: 2026-06-26
verified: 2026-06-26
compiled-from: conversation
sources:
  - session:019f009b-7333-7800-b50f-643c41dd3c51
---

# ReducedSource and Dynamic Feed Composition

## Overview

ReducedSource is a kernel-owned primitive that registers a source interest, runs a deterministic reducer on its results, diffs the output against prior reduction, closes removed targets, and opens new/changed targets through existing registries, triggering subscription recompilation. <!-- [^019f0-90154] -->

## Design Principles

nmp-core and nmp-planner must not contain NIP-specific nouns such as contact list, mute list, follow pack, kind:3, or kind:10000. These semantics belong in protocol crates with reducers that materialize ReducedSource output. <!-- [^019f0-1ae9d] -->

## Activation and Lifecycle

ReducedSource activation must be kernel-owned and event-driven. Account switch, logout, login, source replacement, relay reroute, and cache hydration all close stale targets and open current targets through the same mechanism. <!-- [^019f0-434e3] -->

## Reducer Semantics

Empty reducer output must fail closed and withdraw downstream targets; it must never become wildcard acquisition. <!-- [^019f0-88bae] -->

## Dependent Interests

Dependent interests must be implemented as ReducedSource instances with reducer outputs materialized through the existing interest/ref registry machinery, not as bespoke per-feed doors like the retired follow-feed path. <!-- [^019f0-41ba3] -->

## Feed API

Apps open dynamic feeds through typed `open_feed(FeedParams)` with `FeedScope` and `PubkeySetExpr` enums. Raw `open_interest` remains static, non-feed, and internal. <!-- [^019f0-29342] -->

## InterestShape

InterestShape is a query template with dynamic slots for `authors`, `event_ids`, `addresses`, and `tags[key]` that are filled by ReducedSource reducer output. <!-- [^019f0-f3294] -->

## FeedParams

FeedParams with `FeedScope` and `PubkeySetExpr` enums is the typed app-facing specification for dynamic feed composition, mapping to ReducedSource reducers and dependent-interest materialization. <!-- [^019f0-4bffd] -->

## Read Model Patterns

Observed-interest projections must activate with scope restricted to the declared `InterestShape`, not as unfiltered global fan-out. Relay-pinned projections must apply provenance gating to live delivery. Live event taps (`register_live_event_tap`) and hydrating observed projections (`ObservedProjectionRegistrar::open_observed_projection`) are distinct read-model patterns; late-joining or per-view hydration must use observed projections, not live taps plus app-side reconstruction. <!-- [^019f0-7f96a] -->
