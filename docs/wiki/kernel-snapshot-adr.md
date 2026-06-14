---
title: Kernel Snapshot ADR
slug: kernel-snapshot-adr
topic: kernel-snapshot
summary: FullState/full snapshot is the correctness path; granular ViewBatch or delta variants are added only when profiling proves the snapshot path is the bottleneck a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:019ec57a-fb01-7081-80c8-d7107f302049
---

# Kernel Snapshot ADR

## Kernel Snapshot Architecture

FullState/full snapshot is the correctness path; granular ViewBatch or delta variants are added only when profiling proves the snapshot path is the bottleneck and the delta is lossless. The ADR-0037 typed sidecar made each re-encode cheaper but did not make it incremental. This is a deliberate documented architectural bet (ADR-0037) and the highest-risk performance decision; per-projection revision gating is the minimum acceptable improvement and the correct middle path: re-emit only changed projections while keeping the snapshot/rev correctness invariant. The false binary of full-snapshots versus fragile hand-written deltas ignores this option. Existing generic-JSON projections already have a change-gate (snapshot_registry.rs); typed sidecars and kernel built-ins lack any unchanged-reuse-prior-buffer mechanism.

The kernel-authored snapshot via Kernel::make_update must be the single source of truth for all UI state; no hardcoded 'configured' relay statuses or empty typed-projection sidecars.

The KCEV FlatBuffer is deliberately protocol-agnostic — kind rides as an opaque uint with no protocol branching in the kernel-owned buffer, as documented in claimed_events.fbs:31-34.

ADR-0036 is superseded to document that the kernel owns follow-to-interest expansion (not the composition root), replacing the misleading accepted version. No delta protocol (WireDelta) will be added; the snapshot model is empirically validated and the delta alternative shipped zero consumers and was deleted.

ADR-0039's rejection of host-declared projection subscriptions is a category error: declaring which projections a host consumes is static resource ownership (the output-side sibling of push_interest), not view-state leakage; relay_diagnostics shipping 4×/sec to every host is unjustified permanent waste.

Relay diagnostics projections must ship raw timestamps over the wire; shells format relative-time strings at render time (aim.md §62 forbids format_ago_* inside projection builders).

The NMP Inspector diagnostics dock decodes the live Tier-3 snapshot envelope for relays, subscriptions, cache, routing, and signer data — no app-side diagnostic logic re-derivation is allowed (doctrine: thin shell, kernel owns tone/health logic).

GAP-5 (negentropy session stats) must be emitted as a Tier-3 field with rounds, have/need, and transfer_avoided_bytes computed kernel-side per doctrine, with honest omission of 'ranges compared'.

The Android KernelProfileHost uses remember(model, profiles) where profiles is a new Map object on every snapshot tick, causing the host to be recreated every tick and triggering a claim/release churn loop in DisposableEffect (same bug class as chirp-web commit 4d1888f9a). The fix for Android profile-claim churn is to remove profileHost from the DisposableEffect key in NostrAvatar and NostrProfileName, and stabilize KernelProfileHost by keying remember on model only with rememberUpdatedState for the profiles map.

The DmConversationListScreen double-collects model.state independently of its parent, causing profiles and conversations to potentially reflect different snapshot generations.

The encode_snapshot_with_envelope function reuses a per-tick FlatBufferBuilder via reset() and to_vec() before return, eliminating per-tick allocation; use-after-reset is structurally prevented because to_vec() is the sole return path. The FlatBufferBuilder for kernel encoding is held in the Kernel struct and reset()d at the start of each encode, with to_vec() copying finished bytes out before any subsequent reset. FlatBufferBuilder reset() clears written_vtable_revpos, strings_pool, nested, finished, min_align, and resets head to capacity-end for allocation reuse; finished_data() panics if finished==false, so a malformed encode panics rather than returning corrupt bytes.

The kernel is !Send via PhantomData<*const ()>, ensuring no two threads can hold it simultaneously; the shared snapshot_builder field is therefore re-entrancy-proof because the &mut self borrow is exclusive for the whole make_update call.

The capability gate for incremental apply uses a single Arc<AtomicBool> source of truth stored in SnapshotRegistry, read lock-free by both the kernel and producer closures. (Previously: SnapshotRegistry incremental_apply_state() reads is_incremental_apply_enabled + take_incremental_apply_baseline_pending under a single lock acquisition, eliminating a potential double-lock race.)

The kernel publishes FrameIdentity(session_id, snapshot_epoch) at the top of make_update before any projection closure runs, and host-registered producers rebaseline on the same two-axis signal the host ProjectionCache resets on.

iOS session_id and snapshot_epoch are threaded out of the single existing FlatBuffers decode pass in KernelUpdateFrameDecoder, with no second buffer re-parse, avoiding both O(buffer) per-tick waste and an unwanted FlatBuffers import in KernelBridge.

The encode timer scope captures the full path including run_typed_projections, manifest stamping, omit_unchanged, and FlatBufferBuilder encode, so the gate is not blind to the CPU cost of the omit pass.

The feed re-serializes a byte-identical ~58.8 KB payload on every idle 4Hz tick, which is ~6× the entire rest of the frame. Feed gating uses exact byte-equality (memcmp of retained Vec<u8>), not a hash, because a hash collision would freeze the timeline permanently. The byte-identity oracle in the S6 capstone is fail-closed: only the two whitelisted Tier-1 keys (claimed_event_embeds, nip46_onboarding) may be absent; any other dropped key or payload mismatch hard-fails the capstone. The S6 serialize_us gate uses a 20% tolerance band (threshold = p50_A × 1.20) to accommodate OS scheduling noise across two independent kernel instances, rather than strict equality; the omit cost is more than offset by encoding fewer rows so Phase B is consistently faster.

Rung 6 delivers a 97.6% idle frame-byte reduction (45,440 B → 1,104 B) with zero data loss, measured with the feed registered in the harness. The R6-S4 false-resend gate tests a followed author's reply to an unknown root (which enters the engine but leaves the 80-card window snapshot byte-identical), not just a stranger event rejected by the follow predicate. Option B (feed row-deltas) is not warranted: it does nothing for the idle case (already fixed by omission), and on a mutating frame the List must re-render because a card genuinely changed; new events are human-paced, not 4Hz.

R3's .equatable() List boundary was already the dominant idle-jank lever, short-circuiting the expensive timeline re-render on idle ticks before Rung 6 existed; Rung 6 trims the residual (FFI bytes + @Published invalidation + array allocation). The release encode cost is ~129µs at 4Hz (~516µs/s), which is negligible; the Debug build is ~17.6× slower, so a meaningful chunk of felt jank may be the Debug build itself rather than serialization.

The feed reaches the host through exactly one wire path — the typed sidecar; the generic payload:Value slot is intentionally dropped before the wire (ADR-0044), so the generic snapshot_json path is CPU-only and not a correctness surface. The op_feed engine's snapshot() function remains untouched by the feed gating; the emission decision lives entirely in the producer closure layer (Seam A).

<!-- citations: [^02745-86] [^02745-87] [^78c8e-21] [^2e544-29] [^78c8e-49] [^78c8e-66] [^02745-103] [^78c8e-84] [^78c8e-102] [^bf035-165] [^019ec-17] [^78c8e-454] [^78c8e-471] [^2e544-410] [^78c8e-486] -->
