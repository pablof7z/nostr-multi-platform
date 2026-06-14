---
title: Projection Registry
slug: projection-registry
topic: projection-registry
summary: The projection registry contains 34 total keys (28 with Swift typed decode stubs) after the removal of KAVW and KTVW.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Projection Registry

## Projection Registry

The projection registry contains 34 total keys (28 with Swift typed decode stubs) after the removal of KAVW and KTVW.

The projection key for signer state is 'signer_state' with FlatBuffer file identifier KSST, replacing the former 'bunker_connection_state'/KBCS across all four surfaces (codegen registry, Swift, iOS bridges, Android) to express awaiting_approval and unavailable states. Keeping KBCS would make these states inexpressible in the typed sidecar — the exact fragmentation D6 exists to remove.

NMP's permanent codegen-registry key gate requires every codegen json_key to be a subset of live registry keys ∪ KERNEL_BUILTIN_PROJECTION_KEYS, closing the #1084 defect class where producer-side key renames silently broke consumer decoders.

All registered projections ride every snapshot because ADR-0039 explicitly rejected letting the host specify which projections it wants, to avoid leaking view state into the kernel.

Swift runs the FlatBuffers Verifier (getCheckedRoot) on every decode of in-process Rust-produced snapshots; the unchecked getRoot would skip this unnecessary verification of trusted bytes.

The NMMS FlatBuffers schema is bumped to v2 with PendingOpRow and LastOpError tables; Rust/Swift/Kotlin bindings are regenerated.

Android's Marmot group dialogs never dismissed and the signer badge was dead because typed projections decoded on iOS (signer_state, bunker_handshake, nip46_onboarding) had no typed decoders wired on Android — silently resolving to null/empty, hidden by green CI. PR #1286 wires five typed projections on Android (signer_state KSST, action_lifecycle KALC, action_stages KAST, action_results KARS, relay_diagnostics KRDG), fixing Marmot dialog dismissal and the signer badge.

PR #1287 adds a Swift flatc drift gate (ci/check-swift-flatc-drift.sh) that pins flatc 25.12.19, maps all 34 schemas, and byte-diffs generated bindings; it also fixes snake_to_camel leading-underscore preservation, first_diff_line reporting for length-only diffs, and adds nmp.feed.home to the key-presence assertion.

F-CR-12 (#1225) built the iOS typed EmbedKindProjection structs plus 11 Rust and 20 Swift golden tests, making the #1283 consumer side half-built — the shells need to decode instead of construct, turning the goldens into a real parity gate.

Cross-platform typed-decoder parity gates enforce that every projection key decoded on one platform has a decoder on all platforms, preventing the class of bug where iOS wires a projection that Android silently drops.

The Compose profile-component family (NostrAvatar, NostrProfileName, etc.) is vendored under a byte-identical drift gate; any fix must be applied to both the registry canonical and the Android vendored copies.

The claimed_profiles decode cluster was promoted to the public typed-projections surface (pub + facade entry) because the v0.3.x example rewrite referenced decode_claimed_profiles/CLAIMED_PROFILES_SCHEMA_ID which were pub(crate) and #[cfg(test)]-gated, causing a compilation failure in examples not caught by workspace builds. The public surface now exposes decode_claimed_profiles, ClaimedProfilesModel, and CLAIMED_PROFILES_SCHEMA_ID as pub.

The registry export test committed_registry_json_matches_generated_output must stay green; it caught the registry.json staleness that broke master after PR #1302.

<!-- citations: [^da6b1-34] [^da6b1-35] [^da6b1-36] [^78c8e-30] [^02745-16] [^02745-46] [^02745-64] [^da6b1-59] [^da6b1-91] -->
