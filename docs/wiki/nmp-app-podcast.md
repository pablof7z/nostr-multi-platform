---
title: NMP App Podcast Crates & Directory Layout
slug: nmp-app-podcast
summary: The `apps/podcast/` directory contains the Rust/cross-platform crates for podcast business logic (nmp-app-podcast, podcast-core, podcast-feeds, podcast-llm, pod
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-06-04
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:bf32731e-1fd0-4e22-86d3-26ecf7294bf3
  - session:c066a9a0-1c78-4b21-8511-4be986a736de
  - session:f8eb6e59-19f0-4591-a9b4-47453c051d45
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# NMP App Podcast Crates & Directory Layout

## Directory Layout

The `apps/podcast/` directory contains the Rust/cross-platform crates for podcast business logic (nmp-app-podcast, podcast-core, podcast-feeds, podcast-llm, podcast-rag, podcast-audio). The `ios/NmpPodcast/` directory is an iOS Xcode project providing a SwiftUI UI shell whose Views are copied verbatim from Podcastr (`~/Work/podcast`) with only data-source bindings swapped to point at the NMP Rust bridge. The NmpPodcast iOS project exists to prove the M11 kernel-boundary concept: the same UI runs on both Podcastr (pure Swift) and NmpPodcast (Swift + Rust backend). The podcast app compiles against the same current nostrmultiplatform symlink (and thus the same current nmp-core/nmp-ffi crates) as Chirp, but the podcast NmpCore.h is missing several newer symbols present in Chirp's header, including nmp_app_claim_event, nmp_app_open_uri, nmp_app_register_action_result_observer, nmp_app_ack_action_stage, nmp_app_recent_routing_decisions, nmp_app_load_older_feed, nmp_app_create_new_account, and nmp_app_switch_active. The repo's canonical app directory structure uses the pattern `apps/<app-name>/{ios,android,desktop,tui,web}` for platform shells and `apps/<app-name>/crates` for core and assisting Rust crates. HTTP transport for Blossom uploads remains in the sibling podcast crate (nmp-app-podcast), not in nmp-core, which has no HTTP client.

The full app migration stack (feat/nostr-signing-to-nmp + v0.2.5 + KernelSigner) is pushed and landed on podcast-player main. The podcast-player app is upgraded to v0.2.5, adding KernelSigner to un-degrade uploads and migrating to nmp.blossom.upload where applicable.

<!-- citations: [^83b5d-13] [^bf327-1] [^c066a-1] [^16ca6-6] [^f1b74-30] [^83b5d-19] [^83b5d-26] -->
## App-First vs Technology-First Structure

In an app-first directory layout, each app (e.g., `apps/podcast/`, `apps/chirp/`) contains its own `rust/`, `ios/`, and `android/` subdirectories rather than grouping all platform code under top-level technology directories. For a multi-platform repo, the app-first layout is objectively cleaner than the technology-first layout because it allows understanding the whole app from a single directory. <!-- [^bf327-2] -->

## Migration Considerations

Moving `ios/NmpPodcast/` to `apps/podcast/ios/` requires updating absolute/relative paths in Xcode schemes, `project.yml` for XcodeGen, `Info.plist` refs, and Cargo workspace members. The android/podcast artifact is deleted from Cargo.toml and android/settings.gradle.kts. The podcast-app migration pins to nmp-v0.1.0 using `nmp init --nmp-version 0.1.0`.

An agent will be launched to make changes to the podcast-blossom codebase so it works as expected with the next version of NMP. [^83b5d-14]

<!-- citations: [^bf327-3] [^f8eb6-1] [^42908-11] -->
