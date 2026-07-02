// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache --platform ts \
//       --out web/packages/runtime-web/src/keyedRefCache.generated.ts
//
// Source of truth: KEYED_PROJECTIONS in
// `crates/nmp-codegen/src/keyed_projection_row_payload.rs`.
// The CI gate (`.github/workflows/codegen-drift.yml`) fails any PR whose
// generated TypeScript differs.
//
// ADR-0070 Lane A twin (#2722): per-key row cache for keyed reference
// projections (`refs.profile` / `refs.event`) — byte-for-byte semantically
// identical to `KeyedRefCache.generated.swift`, `KeyedRefCache.kt`, and
// `nmp_core::refs::RefRowCache`.
// ─────────────────────────────────────────────────────────────────────────────

import * as flatbuffers from "flatbuffers";

import { RefRowDeltaBatch as RefRowDeltaBatchFb } from "./generated/nmp/refs/ref-row-delta-batch.js";
import { RefRowState } from "./generated/nmp/refs/ref-row-state.js";
import { ProfileSnapshot } from "./generated/nmp/kernel/profile-snapshot.js";
import { refRowProfile, type ProfileWire } from "./refRowDecoders.js";

/** Decoded row state. Mirrors the Rust/Swift/Kotlin `RefRowState` — Unchanged
 *  is represented by ABSENCE of the row, never an enum value. */
type DecodedRowState = "changed" | "cleared";

/** One decoded NRRD row (pre-commit — decode-before-commit runs on this). */
type DecodedRow = { key: string; rev: bigint; state: DecodedRowState; payload: Uint8Array };

/** A fully-decoded NRRD `RefRowDeltaBatch`. The wire's own `namespace` field
 *  is intentionally NOT extracted — exactly like the Swift/Kotlin generators,
 *  routing is derived from the caller's `projectionKey`, never the payload's
 *  self-reported namespace. */
type DecodedBatch = { baseline: boolean; rows: DecodedRow[] };

/** One cached row: the last committed per-key rev + raw typed payload bytes. */
type CachedRow = { rev: bigint; payload: Uint8Array };

/** Outcome of one `merge()` call. */
export type RefRowApplyOutcome = { changedKeys: string[]; decodeFailed: boolean };

const EMPTY_OUTCOME = (): RefRowApplyOutcome => ({ changedKeys: [], decodeFailed: false });

/**
 * Decode an NRRD `RefRowDeltaBatch` from finished FlatBuffers bytes, FAILING
 * CLOSED on any malformation. Returns `undefined` when the buffer is too
 * short, lacks the `NRRD` file identifier, is missing a required row key, OR
 * carries an unknown `state` discriminant (mirrors the Swift/Kotlin whole-
 * batch fail-closed decode).
 */
function decodeRefRowDeltaBatch(bytes: Uint8Array): DecodedBatch | undefined {
  if (bytes.length < 8) return undefined;
  let batch: RefRowDeltaBatchFb;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!RefRowDeltaBatchFb.bufferHasIdentifier(bb)) return undefined;
    batch = RefRowDeltaBatchFb.getRootAsRefRowDeltaBatch(bb);
  } catch {
    return undefined;
  }
  const rows: DecodedRow[] = [];
  const len = batch.rowsLength();
  for (let i = 0; i < len; i += 1) {
    const row = batch.rows(i);
    if (!row) return undefined;
    const key = row.key();
    if (key === null) return undefined;
    const rawState = row.state() as number;
    let state: DecodedRowState;
    if (rawState === RefRowState.Changed) {
      state = "changed";
    } else if (rawState === RefRowState.Cleared) {
      state = "cleared";
    } else {
      return undefined;
    }
    const payloadArray = row.payloadArray();
    const payload = payloadArray ? payloadArray.slice() : new Uint8Array(0);
    rows.push({ key, rev: row.rev(), state, payload });
  }
  return { baseline: batch.baseline(), rows };
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

/** Decode one `KPRF` row payload buffer into `ProfileWire` (ADR-0070 Lane G twin). */
function decodeProfileRow(bytes: Uint8Array): ProfileWire | undefined {
  if (bytes.length < 8) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ProfileSnapshot.bufferHasIdentifier(bb)) return undefined;
    const reader = ProfileSnapshot.getRootAsProfileSnapshot(bb);
    // Hand-written glue (NOT generated): reader -> domain. See `refRowDecoders.refRowProfile`.
    return refRowProfile(reader);
  } catch {
    return undefined;
  }
}

/**
 * NMP-owned per-key row cache for keyed reference projections (ADR-0070).
 *
 * Thread-safety: fed only from the worker's `update_bytes` handler on the
 * single main-thread event loop — no concurrent `merge()` calls.
 */
export class KeyedRefCache {
  // projectionKey -> (rowKey -> entry). Keyed by the CALLER's projectionKey
  // (e.g. "refs.profile"), never the payload's self-reported namespace —
  // mirrors the Swift/Kotlin twins exactly.
  private rows = new Map<string, Map<string, CachedRow>>();
  private appliedSession = 0n;
  private appliedEpoch = 0n;
  private baselinedFlag = false;
  private needsResyncFlag = false;

  /** D3-5: false until the first post-baseline frame is applied. */
  baselined(): boolean {
    return this.baselinedFlag;
  }

  /** D3-4: latches on any per-row decode-before-commit failure. */
  needsResync(): boolean {
    return this.needsResyncFlag;
  }

  /** Hard-reset so the next frame is a full baseline. */
  reset(): void {
    this.rows.clear();
    this.appliedSession = 0n;
    this.appliedEpoch = 0n;
    this.baselinedFlag = false;
    this.needsResyncFlag = false;
  }

  /**
   * The cached raw payload bytes for one (projectionKey, rowKey), or
   * `undefined`. Low-level merge primitive — NOT the public refs API.
   * Prefer the typed per-namespace accessor below (invariant #4: no
   * dishonest raw surface for a namespace that HAS a typed decoder); this
   * stays exported only for namespaces with `ts: None` (raw-bytes-only).
   */
  payload(projectionKey: string, rowKey: string): Uint8Array | undefined {
    return this.rows.get(projectionKey)?.get(rowKey)?.payload;
  }

  /** The full cached raw `rowKey -> payload` set for one projectionKey. */
  snapshot(projectionKey: string): Map<string, Uint8Array> {
    const out = new Map<string, Uint8Array>();
    const ns = this.rows.get(projectionKey);
    if (ns) for (const [k, v] of ns) out.set(k, v.payload);
    return out;
  }

  /** Whether `projectionKey` is a registered keyed reference projection.
   *  Mirrors the Swift/Kotlin `namespace(forProjectionKey:)` gate. */
  private static isKeyedProjection(projectionKey: string): boolean {
    switch (projectionKey) {
      case "refs.profile":
      case "refs.event":
        return true;
      default:
        return false;
    }
  }

  /** ADR-0070 invariant #2: a `Changed` row commits only after its
   *  payload decodes to the namespace's concrete type. Projection keys
   *  with no typed decoder accept any non-empty payload (raw-bytes-only
   *  namespaces, e.g. `refs.event` until #2722 scopes it). */
  private rowDecoder(projectionKey: string, payload: Uint8Array): boolean {
    switch (projectionKey) {
      case "refs.profile":
        return decodeProfileRow(payload) !== undefined;
      default:
        return payload.length > 0;
    }
  }


  /**
   * Merge one keyed-projection payload (`nmp.refs.RefRowDeltaBatch` bytes)
   * under the frame's `(sessionId, snapshotEpoch)` identity. Returns the row
   * keys whose cached row changed (committed or cleared) this frame.
   *
   * Invariants: absent row == Unchanged (retained); explicit Cleared removes
   * (rev-safe); decode-before-commit per row (malformed row keeps prior +
   * latches needsResync); session/epoch change or `baseline` rebuilds the full
   * set. A baseline commits atomically (scratch-then-commit). A garbage batch
   * fails closed — prior cache retained, resync latched.
   */
  merge(projectionKey: string, payload: Uint8Array, sessionId: bigint, snapshotEpoch: bigint): RefRowApplyOutcome {
    if (!KeyedRefCache.isKeyedProjection(projectionKey)) {
      return EMPTY_OUTCOME();
    }
    const batch = decodeRefRowDeltaBatch(payload);
    if (!batch) {
      this.needsResyncFlag = true;
      return EMPTY_OUTCOME();
    }

    const identityChanged = sessionId !== this.appliedSession || snapshotEpoch !== this.appliedEpoch;

    if (batch.baseline) {
      return this.applyBaseline(projectionKey, batch, identityChanged, sessionId, snapshotEpoch);
    }

    // A non-baseline batch under a changed identity cannot rebuild the full
    // set; fail closed (adopt identity, latch resync, retain prior cache)
    // rather than merge deltas onto a stale-epoch base. The producer always
    // follows an identity bump with a baseline frame.
    if (identityChanged) {
      this.appliedSession = sessionId;
      this.appliedEpoch = snapshotEpoch;
      this.baselinedFlag = false;
      this.needsResyncFlag = true;
      return EMPTY_OUTCOME();
    }

    return this.applyIncremental(projectionKey, batch);
  }

  /**
   * Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
   * WHOLE batch): decode every required row into a scratch map first and
   * replace the projection only after all rows decode. One bad row fails the
   * entire baseline closed — prior cache preserved, needsResync latched.
   *
   * When `identityChanged` is set this is the FIRST baseline at a new
   * session/epoch: on a SUCCESSFUL decode it flips identity and drops every
   * OTHER projection's prior-epoch rows as part of the atomic commit; on a
   * decode FAILURE it touches nothing (deferred reset, fail-closed).
   */
  private applyBaseline(
    projectionKey: string,
    batch: DecodedBatch,
    identityChanged: boolean,
    sessionId: bigint,
    snapshotEpoch: bigint,
  ): RefRowApplyOutcome {
    const scratch = new Map<string, CachedRow>();
    for (const row of batch.rows) {
      if (row.state === "cleared") {
        scratch.delete(row.key);
        continue;
      }
      if (!this.rowDecoder(projectionKey, row.payload)) {
        this.needsResyncFlag = true;
        return { changedKeys: [], decodeFailed: true };
      }
      const existing = scratch.get(row.key);
      if (!existing || row.rev > existing.rev) {
        scratch.set(row.key, { rev: row.rev, payload: row.payload });
      }
    }

    // Decode succeeded → now (and ONLY now) it is safe to mutate state.
    if (identityChanged) {
      for (const key of [...this.rows.keys()]) {
        if (key !== projectionKey) this.rows.delete(key);
      }
      this.appliedSession = sessionId;
      this.appliedEpoch = snapshotEpoch;
      this.needsResyncFlag = false;
    }

    const prior = identityChanged ? undefined : this.rows.get(projectionKey);
    const changed = new Set<string>();
    for (const [key, row] of scratch) {
      const prev = prior?.get(key);
      if (!prev || !bytesEqual(prev.payload, row.payload)) {
        changed.add(key);
      }
    }
    if (prior) {
      for (const key of prior.keys()) {
        if (!scratch.has(key)) changed.add(key);
      }
    }
    this.rows.set(projectionKey, scratch);
    this.baselinedFlag = true;
    return { changedKeys: [...changed].sort(), decodeFailed: false };
  }

  /**
   * Steady-state incremental merge with rev-safe clears and the per-row
   * decode-before-commit seam.
   */
  private applyIncremental(projectionKey: string, batch: DecodedBatch): RefRowApplyOutcome {
    let ns = this.rows.get(projectionKey);
    if (!ns) {
      ns = new Map<string, CachedRow>();
      this.rows.set(projectionKey, ns);
    }
    const changed = new Set<string>();
    let decodeFailed = false;

    for (const row of batch.rows) {
      if (row.state === "cleared") {
        // Rev-safe clear: remove only if the clear's rev is NEWER than the
        // cached row, so a stale reordered clear can never delete a newer
        // live row. A clear for an absent key is a no-op.
        const existing = ns.get(row.key);
        if (existing && row.rev > existing.rev) {
          ns.delete(row.key);
          changed.add(row.key);
        }
        continue;
      }
      // Changed. Reorder/duplicate guard: skip a row not newer than cached.
      const existing = ns.get(row.key);
      if (existing && row.rev <= existing.rev) continue;
      // Decode-before-commit per row (invariant #2): empty OR invalid bytes
      // keep the prior row, latch needsResync.
      if (row.payload.length === 0 || !this.rowDecoder(projectionKey, row.payload)) {
        this.needsResyncFlag = true;
        decodeFailed = true;
        continue;
      }
      ns.set(row.key, { rev: row.rev, payload: row.payload });
      changed.add(row.key);
    }

    this.baselinedFlag = true;
    return { changedKeys: [...changed].sort(), decodeFailed };
  }

  // ADR-0070 Lane G twin (#2722): per-key + full-snapshot TYPED accessors.
  // Each decodes the cached row-payload buffer through the namespace's
  // typed reader (the SAME buffer the kernel's `ref_*_row_payload` encoder
  // emits) into the concrete domain type — never a raw `Uint8Array`
  // surface (invariant #4). A decode miss returns `undefined` / omits the key.
  profile(key: string): ProfileWire | undefined {
    const bytes = this.payload("refs.profile", key);
    if (!bytes) return undefined;
    return decodeProfileRow(bytes);
  }

  profiles(): Map<string, ProfileWire> {
    const out = new Map<string, ProfileWire>();
    for (const [key, bytes] of this.snapshot("refs.profile")) {
      const wire = decodeProfileRow(bytes);
      if (wire !== undefined) out.set(key, wire);
    }
    return out;
  }

}
