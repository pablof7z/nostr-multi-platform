---
title: Podcast iOS Bridge — FlatBuffers Migration
slug: podcast-ios-bridge-flatbuffers
summary: The NmpUpdateCallback in the podcast iOS bridge header must receive binary FlatBuffers bytes (const uint8_t *bytes, uintptr_t len) instead of a JSON string (con
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:c066a9a0-1c78-4b21-8511-4be986a736de
---

# Podcast iOS Bridge — FlatBuffers Migration

## Podcast iOS Bridge FlatBuffers

The NmpUpdateCallback in the podcast iOS bridge header must receive binary FlatBuffers bytes (const uint8_t *bytes, uintptr_t len) instead of a JSON string (const char *json). Per-app snapshot functions (nmp_app_chirp_snapshot, nmp_app_podcast_snapshot) currently return JSON strings via serde_json::to_string. The Notes app avoids this per-app snapshot format issue by consuming raw events and kernel projections only, having no per-app snapshot symbol. The podcast NmpCore.h must be updated to include newer symbols present in Chirp's header, including nmp_app_claim_event, nmp_app_open_uri, nmp_app_register_action_result_observer, nmp_app_ack_action_stage, nmp_app_recent_routing_decisions, nmp_app_load_older_feed, nmp_app_create_new_account, and nmp_app_switch_active. [^c066a-3]

## See Also

