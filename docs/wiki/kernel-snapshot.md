---
title: Kernel Snapshot
slug: kernel-snapshot
topic: kernel-snapshot
summary: The ADR-0055 Rung 3 producer omits Unchanged Tier-2 projection rows from the wire entirely (not as empty payloads), keeps an explicit Cleared marker for drained
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
---

# Kernel Snapshot

## Wire Format for Snapshot Deltas

The ADR-0055 Rung 3 producer omits Unchanged Tier-2 projection rows from the wire entirely (not as empty payloads), keeps an explicit Cleared marker for drained keys, and keeps full Changed rows.

<!-- citations: [^78c8e-485] [^78c8e-494] -->

## Gallery Curation in Rung 3

The gallery's curated generic-path subset is explicitly left untouched in Rung 3 (no blanket regen). <!-- [^78c8e-495] -->

## FlatBuffer Lifecycle for 4 Hz Kernel Encode

The FlatBufferBuilder for the 4 Hz kernel encode path is held in the Kernel struct and reset()d at the start of each encode tick, with finished bytes copied out via to_vec() before return, rather than allocating a fresh builder per tick. <!-- [^78c8e-496] -->

## Projection Manifest and Omit-Unchanged Inverse Pass

The projection manifest enumerates the FULL key universe (all 18 kernel builtin keys) every tick independent of the typed vector, not just rows present in typed. The omit_unchanged transform gains an inverse pass: for each manifest.states entry NOT already in the output, synthesize a payload-less TypedProjectionData with state=Cleared for manifest-Cleared keys; for Changed&&absent keys in {action_results, signed_events, action_stages, action_lifecycle}, synthesize defensively; for Changed-but-absent on any other Tier-2 key, debug_assert!+warn! and never synthesize. <!-- [^78c8e-497] -->

## Edge Machines for Action Stages and Lifecycle

action_stages and action_lifecycle use a note_copy_emit edge machine (analog of note_drain_emit) with a copy_prev_nonempty map to produce Cleared presence on the non-empty→empty edge, rather than a counter bump, because their presence is governed by rev-vs-last-emit (not drain semantics). The note_copy_emit non-empty arm must NOT park pending_presence=Changed; steady-state non-empty is left to the rev-vs-last-emit rule so genuinely-unchanged ticks resolve to Unchanged and are omitted. <!-- [^78c8e-498] -->

## Ack Action Stage Settlement Version

ack_action_stage must bump settlement_enqueue_ver so partial-ack legitimately advances the rev, keeping the StaleStamp oracle sharp once the perpetual-Changed override is removed. <!-- [^78c8e-499] -->

## Feed Change-Signal Detection

The R6-S1 feed change-signal uses exact byte equality (memcmp of the retained Vec<u8> last_emitted), not a hash, because a hash collision would cause a permanently frozen feed. The feed omit decision lives in the producer-closure layer (Seam A) via FeedEmissionState.should_emit returning None on omit, not in the engine; the engine's snapshot() method is untouched. <!-- [^78c8e-500] -->

## Feed Emission State Rebaseline and Frame Identity

The feed emission state rebaselines on the FrameIdentity(session_id, snapshot_epoch) tuple — the same two-axis signal the host ProjectionCache resets on — so that ActorCommand::Reset (which changes session_id and clears the host cache) forces a full baseline emit rather than omitting into an empty cache. Kernel::publish_frame_identity is called as the FIRST mutating call in make_update (before any projection closure runs), ensuring every closure reads fresh identity via lock-free Acquire loads. <!-- [^78c8e-501] -->

## TypedProjectionEmissionState Shared Helper

TypedProjectionEmissionState (in nmp-core) is the single shared byte-equality omit helper for all host-registered projections; FeedEmissionState in nmp-nip01 is a thin re-export type alias. <!-- [^78c8e-502] -->

## Projection Rev Oracle Build Exclusion

The ADR-0055 projection-rev oracle must not be compiled into shipping/production chirp-tui builds. <!-- [^418d5-8] -->
