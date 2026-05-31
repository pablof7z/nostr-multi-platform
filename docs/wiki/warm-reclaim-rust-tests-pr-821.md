---
title: PR #821 — Warm-Reclaim Rust Unit Tests
slug: warm-reclaim-rust-tests-pr-821
summary: PR #821 Rust unit tests confirm claim_profile uses zero relay REQ for already-resident pubkeys; the profile flicker gap is 100% Swift-side lifecycle churn.
tags:
  - rust
  - testing
  - profile
  - kernel
  - claim-expansion
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-18
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:42252c03-76ca-449c-9cfd-ed5949b2bb9d
---

# PR #821 — Warm-Reclaim Rust Unit Tests

> PR #821 Rust unit tests confirm claim_profile uses zero relay REQ for already-resident pubkeys; the profile flicker gap is 100% Swift-side lifecycle churn.

## Overview

PR #821 added three Tier 0 Rust unit tests to crates/nmp-core/src/kernel/profile_claim_tests.rs that definitively confirm the kernel's warm-reclaim behavior. These tests establish that re-claiming an already-resident profile requires zero relay REQ and re-emits the display name on the very next tick. The Rust changes were committed as 4dd68f5.

<!-- citations: [^4edd4-73] [^42252-2] -->
## warm_reclaim_reemits_profile_next_tick_with_no_req

The flagship test. It claims a pubkey, ingests its kind:0 metadata event, releases the claim, then re-claims the same pubkey. It asserts that: (a) the next tick after re-claim has display_name non-null — the profile is immediately available; and (b) there are zero pending REQ to relays — the kernel satisfied the re-claim from its resident store without issuing any network request. This definitively proves the 1–2 tick flicker gap is pure lifecycle churn, not a slow relay round-trip. [^4edd4-74]

## claimed_profiles_present_iff_claim_held

Pins the release lifecycle invariant: a profile must be present in the claimed_profiles projection when and only when it is currently claimed by at least one consumer. When the last claim is released, the profile must be absent from the next snapshot. When re-claimed, it must reappear. This test prevents regressions in the claim/release lifecycle that could cause profiles to leak (stay claimed after release) or be prematurely dropped (removed while still claimed). [^4edd4-75]

## multi_consumer_release_does_not_drop_resident_profile

Multi-consumer guard: if consumer Y still holds a claim on a pubkey while consumer X releases its claim, the profile must remain in claimed_profiles. This simulates the real-world scenario where one view (e.g., home feed) holds a claim on an author while another view (e.g., thread view) is dismissed. The profile must not flicker just because one of multiple consumers released. [^4edd4-76]

## Critical Finding

The test suite confirmed that claim_profile short-circuits with zero relay REQ for already-resident pubkeys. The kernel's resident store retains all previously fetched kind:0 metadata indefinitely. The gap between release and re-claim producing shortHex is 100% a Swift-side lifecycle churn issue — the kernel itself handles warm re-claims correctly with no network cost. This finding shifts the fix effort entirely to the SwiftUI layer: the kernel is already correct. [^4edd4-77]

## See Also
- [[profile-flicker-warm-reclaim-gap|Profile Name Flicker — Warm-Reclaim Lifecycle Gap]] — related guide

