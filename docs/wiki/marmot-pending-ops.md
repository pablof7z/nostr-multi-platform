---
title: Marmot Pending Ops
slug: marmot-pending-ops
topic: mls
summary: When create_group or invite encounters key_package_unavailable, the MarmotMlsOpHandler parks a pending op (typed action + correlation_id + missing pubkey set) a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Marmot Pending Ops

## Marmot Pending Operations

When create_group or invite encounters key_package_unavailable, the MarmotMlsOpHandler parks a pending op (typed action + correlation_id + missing pubkey set) and retries on KP ingest arrival. Pending ops expiry fires on ingest and snapshot edges. The host-op dispatch arm skips terminal verdict recording when the envelope contains pending:true, preventing a spurious success from being recorded before the deferred path runs. iOS and Android shells must not dismiss sheets on dispatch-submission; they must stay open in a busy/pending state until recentTerminal settles or the snapshot shows pending_ops/last_op_error. ingest_signed_event_core takes a caller-supplied now_secs parameter so parked ops stored with synthetic test timestamps don't expire immediately against the real system clock. The NMMS FlatBuffers schema (version 1→2) adds PendingOpRow and LastOpError tables to MarmotSnapshot so hosts can render pending-ops and failure state from the projection without per-app plumbing; last_op_error must be wired (stored on terminal failure, cleared on next success), not shipped as a dead always-None field. PendingOpsStore is in-memory; on process restart every parked op silently vanishes with no last_op_error or re-park, and this silent-loss-on-relaunch contract must be documented in pending.rs. Key-package autopublish must fire on all local-key sign-in paths (nmp_app_signin_nsec, create_new_account, restore_local_nsec_from_keyring, sign_in_local_nsec_with_keyring), not just nmp_marmot_register_active; the pending_mls_autopublish flag is set via NmpApp::add_signer (single-writer) and consumed atomically at the shared register_with_keys tail. The set_pending_mls_autopublish function must remain pub(crate) (not pub); tests exercise it through the real nmp_app_signin_nsec entry point rather than calling the atomic setter directly. iOS capability handler registration (KernelModel.swift:266 registerCapabilityHandler) must precede identity restore (line 344) so the Marmot keyring probe succeeds on cold start. Capability socket teardown is safe via the capability_handler mutex (not via a quiescence gate); nmp_app_set_capability_callback(None) does not quiesce in-flight dispatches, and the GlobalRef must not be narrowed in scope. nmp_marmot_unregister does not withdraw per-group kind:445 interests (there is no remove_interest seam), so a switched-away account's group interests linger in the registry until process exit. No compat aliases: hard-renames apply everywhere, old API surfaces are deleted not deprecated, and pre-v1 data stores (including the apple-native-keyring-store MLS key coordinates) are not migrated. The Kotlin FlatBuffers bindings must be generated with flatc 25.2.10 (matching the pinned flatbuffers-java:25.2.10 runtime), not 25.12.19, to avoid link failures from missing Constants methods.

<!-- citations: [^78c8e-22] [^78c8e-50] [^78c8e-67] [^78c8e-85] [^78c8e-104] -->
