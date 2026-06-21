---
title: Account Creation
slug: account-creation
topic: crate-architecture
summary: "ActorCommand::CreateAccount takes initial_follows: Vec<String> supplied by the app; an empty vector means no contacts are prepopulated and no cold-start kind:3"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc3e-b4a1-72a0-b791-9dcfdd615785
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc59-7035-7ba3-95cc-789d362adff2
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Account Creation

## Account Creation Overview

ActorCommand::CreateAccount takes initial_follows: Vec<String> supplied by the app; an empty vector means no contacts are prepopulated and no cold-start kind:3 is published. The builder uses type-state requiring .with_relays() or .without_initial_relays() before start() compiles—no silent fallback. nostrconnect_bootstrap_relay becomes Option<String> defaulting to None. Chirp gets a chirp-specific create_new_account_with_initial_follows; crate-boundaries.md §9 reclassifies nmp-defaults as a reusable library owning no operator policy. The native shell must not fetch opaque JSON from Rust and pass it into the generic create-account C-ABI, because that makes the shell a sequencer/carrier for operator policy and widens the host-agnostic ABI. The `nmp-chirp-config` crate must gain a `CHIRP_DEFAULT_FOLLOWS` const and a `chirp_default_follows()` function mirroring the relay bootstrap pattern. The generic `nmp-core` must delete `DEFAULT_FOLLOWS` and must not own any app-specific default pubkeys; core may own the generic mechanics (prepopulate contacts, build kind:3, sign, cold-start route) but not the data. Two call-sites still use the generic path and must be migrated: Chirp TUI (runtime_commands.rs:83) calls nmp_app_create_new_account and silently loses Chirp seed follows, and Chirp desktop routes through typed_api::create_account which builds a generic CreateAccount envelope with no initial_follows seam, also missing Chirp seed follows. Android must remove the imperative post-identity openTimeline from signInNsec/createAccount/switchAccount; the View layer already drives it via TimelineScreen.LaunchedEffect, matching iOS. P4 Finding 1 (Android openTimeline) unmasks a latent kernel bug: open_contact_feed in crates/nmp-core/src/actor/commands/publish.rs:708 drops host-declared follow_feed_kinds when no account is active, causing a fresh-launch sign-in to have no feed on both platforms. The fix requires the kernel to persist the kinds unconditionally; the lane was widened to own the kernel fix + native deletion in one PR. Issue #1556 (CreateAccount publish_bootstrap flag) belongs in NMP only for the generic account/create-key lifecycle and capability seam; the default bootstrap policy remains app-owned, and if it changes C ABI, all host binding updates must land in the same PR or an additive v2 entry point should be preferred. PR1b (nostrconnect perms: making sign_event:1,7 app-supplied) is sequenced after p5 #1547 because both edit broker/nostrconnect.rs. nmp-core owns generic account lifecycle state and replayable commands; leaf app Rust owns onboarding defaults; native only executes keyring storage and reports raw results. The account creation lifecycle ADR must make the future public API additive and versioned, with secret-bearing arguments kept out of generic JSON/action history, while request JSON remains an NMP-owned schema rather than app policy passthrough.

<!-- citations: [^019ed-78] [^019ed-79] [^019ed-80] [^019ed-81] [^11850-25] [^019ed-90] [^11850-46] [^11850-66] [^11850-91] [^019ed-115] [^019ed-123] [^11850-156] [^11850-180] [^11850-223] [^019ed-147] -->
