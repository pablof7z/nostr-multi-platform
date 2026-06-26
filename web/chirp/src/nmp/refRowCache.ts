// ADR-0063 Lane A — TypeScript host-side per-key reference cache.
//
// `RefRowCache` is the TypeScript mirror of the canonical Rust `RefRowCache`
// (crates/nmp-core/src/refs/cache.rs) and the generated Swift / Kotlin per-key
// caches. The merge algorithm is byte-for-byte equivalent across all four
// implementations so a web host materialises the SAME full set the producer's
// ground-truth snapshot carries. The Rust invariant property harness is the
// shared oracle; this file must not diverge from it.
//
// DRIFT NOTE: this is the canonical copy of the RefRowCache implementation.
// It is exercised by `refRowCache.test.ts` and serves as the authoritative
// reference implementation across web platforms.
//
// It decodes the `refs.profile` / `refs.event` projection's opaque
// `TypedPayload.payload` (an NRRD `RefRowDeltaBatch`, schema/ref_rowdelta.fbs)
// and merges each tick's row deltas into a persistent `namespace -> key -> row`
// cache. Row payloads stay raw bytes; the consumer decodes them with its
// namespace typed decoder (e.g. ProfileSnapshot/KPRF) via the `decodeOk`
// preflight wired at the call site.
//
// The five ADR-0063 invariants enforced at ROW grain:
//   1. an absent row is Unchanged (retained), never Cleared;
//   2. decode-before-commit: a `Changed` row commits only after its payload
//      decodes; a malformed row leaves the prior cached row intact + latches
//      `needsResync` (D6, fail-closed);
//   3. a `baseline` batch / session-or-epoch change reconstructs the full set;
//   4. payloads are namespace-typed bytes;
//   5. the cache is host-side read-model only — truth stays kernel-owned.

import * as flatbuffers from "flatbuffers";

import { RefRowDeltaBatch as RefRowDeltaBatchFb } from "./generated/nmp/refs/ref-row-delta-batch";
import { RefRowState } from "./generated/nmp/refs/ref-row-state";

/** Decoded row state. Mirrors Rust `RefRowState` — Unchanged is ABSENCE, never
 *  an enumerator. */
type DecodedRowState = "changed" | "cleared";

/** One decoded NRRD row. */
type DecodedRow = {
  key: string;
  rev: bigint;
  state: DecodedRowState;
  /** Empty for a Cleared row. */
  payload: Uint8Array;
};

/** A fully-decoded NRRD `RefRowDeltaBatch`. */
type DecodedBatch = {
  namespace: string;
  baseline: boolean;
  rows: DecodedRow[];
};

/** One cached row: the last committed per-key rev + raw typed payload bytes. */
type CachedRow = {
  rev: bigint;
  payload: Uint8Array;
};

/** Outcome of applying one batch. */
export type RefRowApplyOutcome = {
  /** Keys whose cached row changed this batch (committed `Changed` or removed
   *  `Cleared`). The host re-renders exactly these per-key observable slots. */
  changedKeys: string[];
  /** True iff at least one `Changed` row failed decode-before-commit this batch
   *  (prior row retained; host is known-degraded until resync/baseline). */
  decodeFailed: boolean;
};

/** Decode-before-commit preflight: `(key, payload) -> bool`. A `Changed` row
 *  commits only when this returns `true`. */
export type DecodeOk = (key: string, payload: Uint8Array) => boolean;

const EMPTY_OUTCOME = (): RefRowApplyOutcome => ({ changedKeys: [], decodeFailed: false });

/**
 * Decode an NRRD `RefRowDeltaBatch` from finished FlatBuffers bytes, FAILING
 * CLOSED on any malformation. Returns `undefined` when the buffer is too short,
 * lacks the `NRRD` file identifier, is missing a required row key, OR carries an
 * unknown `state` discriminant.
 *
 * The unknown-discriminant guard is the load-bearing fail-closed step (D6): the
 * generated `RefRow.state()` reads a raw `uint8`, so a corrupt / future-versioned
 * producer can emit `state = 255`. A naive `state === Cleared ? Cleared : Changed`
 * would coerce EVERY unknown value to `Changed` and commit a bogus row
 * (fail-open). Instead only the two defined discriminants (0 = Changed, 1 =
 * Cleared) are accepted; anything else rejects the WHOLE batch so the host
 * retains its prior cache (mirrors Rust `RefRowState::try_from_wire`).
 */
function decodeRefRowDeltaBatch(bytes: Uint8Array): DecodedBatch | undefined {
  // Guard the minimum length before the identifier probe (the FlatBuffers
  // reader asserts on a buffer shorter than the root uoffset + 4-byte id).
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
    // A row missing its key violates the carrier contract — reject the batch.
    if (key === null) return undefined;
    // Fail closed: an unknown `state` discriminant is a decode failure, NOT a
    // silent fall-through to Changed. `state()` returns the raw uint8.
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
    // payloadArray() is a VIEW over the batch buffer; copy so the cached row
    // outlives this decode (the buffer may be reused / GC'd).
    const payload = payloadArray ? payloadArray.slice() : new Uint8Array(0);
    rows.push({ key, rev: row.rev(), state, payload });
  }
  return { namespace: batch.namespace() ?? "", baseline: batch.baseline(), rows };
}

/** The host-side per-namespace row cache. */
export class RefRowCache {
  /** `namespace -> (key -> CachedRow)`. */
  private rows = new Map<string, Map<string, CachedRow>>();
  private appliedSession = 0n;
  private appliedEpoch = 0n;
  /** False until the first batch is applied after a (re)baseline. UI gates on
   *  this (ADR-0055 D3-5). */
  private baselinedFlag = false;
  /** Sticky: latches on any decode-before-commit failure. Cleared on
   *  session/epoch re-baseline. */
  private needsResyncFlag = false;

  /** Whether the cache has applied a baseline (UI-gating flag). */
  baselined(): boolean {
    return this.baselinedFlag;
  }

  /** Whether a decode-before-commit failure is latched. */
  needsResync(): boolean {
    return this.needsResyncFlag;
  }

  /** The cached payload for one key, or `undefined` if absent. */
  get(namespace: string, key: string): Uint8Array | undefined {
    return this.rows.get(namespace)?.get(key)?.payload;
  }

  /** The full materialized state of a namespace: `key -> payload`. */
  snapshot(namespace: string): Map<string, Uint8Array> {
    const out = new Map<string, Uint8Array>();
    const ns = this.rows.get(namespace);
    if (ns) for (const [k, v] of ns) out.set(k, v.payload);
    return out;
  }

  /**
   * Apply one frame's `refs.*` sidecar payload (an encoded NRRD batch) under the
   * frame's `(session_id, snapshot_epoch)` identity.
   *
   * A malformed sidecar payload (not a valid NRRD batch / unknown row-state /
   * missing key) is a fail-closed no-op: the prior cache is retained and an
   * empty outcome returned (D6). Decode-before-commit of each ROW payload is
   * enforced via the `decodeOk` preflight.
   */
  applySidecar(
    payload: Uint8Array,
    sessionId: bigint,
    epoch: bigint,
    decodeOk: DecodeOk,
  ): RefRowApplyOutcome {
    const batch = decodeRefRowDeltaBatch(payload);
    if (!batch) {
      // Fail closed (D6): a garbage sidecar never empties or corrupts the live
      // cache — retain prior state, signal "nothing changed".
      return EMPTY_OUTCOME();
    }
    return this.apply(batch, sessionId, epoch, decodeOk);
  }

  /**
   * Apply one decoded keyed-projection batch under the frame's `sessionId` /
   * `epoch`.
   *
   * D4: a changed `sessionId` or `epoch` resets identity and rebuilds the full
   * set from the new baseline — DECODE-BEFORE-COMMIT: the identity flip + cache
   * clear happen only AFTER the new baseline fully decodes, so a malformed first
   * baseline after an epoch/session bump RETAINS the prior cache and latches
   * `needsResync` (fail-closed, D6).
   */
  private apply(
    batch: DecodedBatch,
    sessionId: bigint,
    epoch: bigint,
    decodeOk: DecodeOk,
  ): RefRowApplyOutcome {
    const identityChanged = sessionId !== this.appliedSession || epoch !== this.appliedEpoch;

    if (batch.baseline) {
      return this.applyBaseline(batch, identityChanged, sessionId, epoch, decodeOk);
    }

    // A non-baseline batch under a changed identity cannot rebuild the full set
    // (it carries only deltas). Fail closed: adopt the new identity but mark
    // un-baselined + needs-resync and retain the prior cache rather than merging
    // deltas onto a stale-epoch base or emptying it. The producer always follows
    // an identity bump with a baseline frame.
    if (identityChanged) {
      this.appliedSession = sessionId;
      this.appliedEpoch = epoch;
      this.baselinedFlag = false;
      this.needsResyncFlag = true;
      return EMPTY_OUTCOME();
    }

    return this.applyIncremental(batch, decodeOk);
  }

  /**
   * Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
   * whole batch). Decodes every `Changed` row into a fresh scratch map; only when
   * ALL required rows decode does it atomically replace the namespace. On any
   * decode failure the prior cache is preserved and resync latches.
   *
   * When `identityChanged` is set this is the FIRST baseline at a new
   * session/epoch: on a SUCCESSFUL decode it flips identity and drops all OTHER
   * namespaces (so prior-epoch rows in unrelated namespaces can't linger); on a
   * decode FAILURE it touches nothing and latches resync.
   */
  private applyBaseline(
    batch: DecodedBatch,
    identityChanged: boolean,
    sessionId: bigint,
    epoch: bigint,
    decodeOk: DecodeOk,
  ): RefRowApplyOutcome {
    const scratch = new Map<string, CachedRow>();
    for (const row of batch.rows) {
      if (row.state === "cleared") {
        // A baseline carries only live rows as Changed. A defensive Cleared in a
        // baseline means the key is simply absent from the rebuilt set — drop it.
        scratch.delete(row.key);
        continue;
      }
      // Decode-before-commit: a single malformed row fails the WHOLE baseline
      // closed — prior cache untouched, resync latched. `this.rows` is not yet
      // mutated.
      if (!decodeOk(row.key, row.payload)) {
        this.needsResyncFlag = true;
        return { changedKeys: [], decodeFailed: true };
      }
      // Duplicate-key guard within one baseline: last-rev wins.
      const existing = scratch.get(row.key);
      if (!existing || row.rev > existing.rev) {
        scratch.set(row.key, { rev: row.rev, payload: row.payload });
      }
    }

    // Decode succeeded → now (and ONLY now) it is safe to mutate state. On an
    // identity change adopt the new identity, drop every OTHER namespace's
    // prior-epoch rows, and clear the prior resync latch.
    if (identityChanged) {
      for (const ns of [...this.rows.keys()]) {
        if (ns !== batch.namespace) this.rows.delete(ns);
      }
      this.appliedSession = sessionId;
      this.appliedEpoch = epoch;
      this.needsResyncFlag = false;
    }

    // Atomic commit: diff prior vs scratch so the host re-renders exactly the
    // slots that changed (added / updated / dropped ghosts), then swap.
    const prior = identityChanged ? undefined : this.rows.get(batch.namespace);
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
    this.rows.set(batch.namespace, scratch);
    this.baselinedFlag = true;
    return { changedKeys: [...changed].sort(), decodeFailed: false };
  }

  /**
   * Steady-state incremental merge. Per-row: an explicit `Cleared` removes the
   * cached row only if its rev is NEWER than the cached row (rev-safe clear — a
   * stale reordered clear can never delete a newer live row); a `Changed` row
   * commits only if its rev advanced AND it decodes (otherwise the prior row is
   * retained and resync latches).
   */
  private applyIncremental(batch: DecodedBatch, decodeOk: DecodeOk): RefRowApplyOutcome {
    let ns = this.rows.get(batch.namespace);
    if (!ns) {
      ns = new Map<string, CachedRow>();
      this.rows.set(batch.namespace, ns);
    }
    const changed = new Set<string>();
    let decodeFailed = false;

    for (const row of batch.rows) {
      if (row.state === "cleared") {
        // Rev-safe clear (reorder guard): a clear carries Lane B's monotonic
        // per-key release rev; it removes the row only when that rev is strictly
        // newer than the cached rev. A stale reordered clear (older rev) is
        // ignored, so it can never delete a newer cached row. A clear for an
        // absent key is a no-op (the final baseline repairs any lost-clear gap).
        const existing = ns.get(row.key);
        if (existing && row.rev > existing.rev) {
          ns.delete(row.key);
          changed.add(row.key);
        }
        continue;
      }
      // Reorder/duplicate guard: skip a row not newer than cached.
      const existing = ns.get(row.key);
      if (existing && row.rev <= existing.rev) continue;
      // Decode-before-commit (D6, invariant #2): on failure keep the prior row,
      // do NOT advance, latch resync.
      if (decodeOk(row.key, row.payload)) {
        ns.set(row.key, { rev: row.rev, payload: row.payload });
        changed.add(row.key);
      } else {
        decodeFailed = true;
        this.needsResyncFlag = true;
      }
    }

    this.baselinedFlag = true;
    return { changedKeys: [...changed].sort(), decodeFailed };
  }
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}
