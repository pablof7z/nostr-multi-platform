// ADR-0063 Lane A twin (#2722) — vitest for the GENERATED `KeyedRefCache`
// merge engine. Ported from `web/nmp-gallery/src/nmp/refRowCache.test.ts`
// (the pre-#2722 hand-maintained implementation this generator replaces) —
// same invariants, adjusted to the generated class's `merge(projectionKey,
// ...)` / `payload(projectionKey, key)` / `snapshot(projectionKey)` API.
//
// Asserts the correctness-critical invariants that MUST match the Rust
// reference (crates/nmp-core/src/refs/cache.rs) and the generated Swift/
// Kotlin caches:
//   - clear DROPS the row (REPLACE-don't-accumulate);
//   - the NEGATIVE: an absent row is RETAINED (Unchanged != Cleared);
//   - decode-before-commit retains the prior row + latches needsResync;
//   - fail CLOSED on an unknown row-state discriminant (reject batch, retain cache);
//   - a baseline rebuilds the full set + drops ghosts;
//   - a session/epoch identity change rebuilds from the new baseline, and a
//     malformed first baseline after the bump RETAINS the prior cache;
//   - an unregistered projectionKey no-ops (routing gate).
import { describe, expect, it } from "vitest";
import * as flatbuffers from "flatbuffers";

import { RefRow } from "./generated/nmp/refs/ref-row";
import { RefRowDeltaBatch } from "./generated/nmp/refs/ref-row-delta-batch";
import { RefRowState } from "./generated/nmp/refs/ref-row-state";
import { KeyedRefCache } from "./keyedRefCache.generated";

// ── NRRD batch builders ──────────────────────────────────────────────────────

type RowSpec = { key: string; rev: number; state: RefRowState; payload?: Uint8Array };

function encodeBatch(baseline: boolean, rows: RowSpec[]): Uint8Array {
  const builder = new flatbuffers.Builder();
  const rowOffsets = rows.map((r) => {
    const keyOff = builder.createString(r.key);
    const payloadOff =
      r.payload && r.payload.length > 0 ? RefRow.createPayloadVector(builder, r.payload) : 0;
    RefRow.startRefRow(builder);
    RefRow.addKey(builder, keyOff);
    RefRow.addRev(builder, BigInt(r.rev));
    RefRow.addState(builder, r.state);
    if (payloadOff) RefRow.addPayload(builder, payloadOff);
    return RefRow.endRefRow(builder);
  });
  const rowsVec = RefRowDeltaBatch.createRowsVector(builder, rowOffsets);
  RefRowDeltaBatch.startRefRowDeltaBatch(builder);
  RefRowDeltaBatch.addBaseline(builder, baseline);
  RefRowDeltaBatch.addRows(builder, rowsVec);
  const root = RefRowDeltaBatch.endRefRowDeltaBatch(builder);
  RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, root);
  return builder.asUint8Array().slice();
}

// `refs.event` has no typed TS decoder (`ts: None` in the registry, #2722
// scope), so its decode-before-commit falls back to the generic
// non-empty-payload default — the same shape the old hand-supplied `decodeOk`
// callback modeled. This keeps the merge-engine tests independent of a real
// KPRF/KCEV codec, exactly like the pre-generator suite.
const PROJECTION_KEY = "refs.event";

function payloadFor(key: string): Uint8Array {
  return new TextEncoder().encode(`payload:${key}`);
}

function changed(key: string, rev: number): RowSpec {
  return { key, rev, state: RefRowState.Changed, payload: payloadFor(key) };
}
function cleared(key: string, rev: number): RowSpec {
  return { key, rev, state: RefRowState.Cleared };
}

function decode(bytes: Uint8Array | undefined): string | undefined {
  return bytes ? new TextDecoder().decode(bytes) : undefined;
}

describe("KeyedRefCache.merge (ADR-0063 twin, #2722)", () => {
  it("a baseline seeds rows; a Changed delta updates one key", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    expect(cache.baselined()).toBe(true);
    expect([...cache.snapshot(PROJECTION_KEY).keys()].sort()).toEqual(["a", "b"]);

    const out = cache.merge(PROJECTION_KEY, encodeBatch(false, [changed("a", 2)]), 1n, 1n);
    expect(out.changedKeys).toEqual(["a"]);
    expect(decode(cache.payload(PROJECTION_KEY, "a"))).toBe("payload:a");
    expect(cache.payload(PROJECTION_KEY, "b")).toBeDefined();
  });

  it("clear DROPS the row (REPLACE, not accumulate)", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    const out = cache.merge(PROJECTION_KEY, encodeBatch(false, [cleared("a", 2)]), 1n, 1n);
    expect(out.changedKeys).toEqual(["a"]);
    expect(cache.payload(PROJECTION_KEY, "a")).toBeUndefined();
    expect(cache.payload(PROJECTION_KEY, "b")).toBeDefined();
  });

  it("the NEGATIVE — an ABSENT row is RETAINED (Unchanged != Cleared)", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    cache.merge(PROJECTION_KEY, encodeBatch(false, [changed("a", 2)]), 1n, 1n);
    expect(decode(cache.payload(PROJECTION_KEY, "b"))).toBe("payload:b");
  });

  it("a stale (older-rev) clear can NOT delete a newer live row", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 5)]), 1n, 1n);
    const out = cache.merge(PROJECTION_KEY, encodeBatch(false, [cleared("a", 3)]), 1n, 1n);
    expect(out.changedKeys).toEqual([]);
    expect(cache.payload(PROJECTION_KEY, "a")).toBeDefined();
  });

  it("decode-before-commit: a malformed Changed ROW RETAINS the prior row + latches resync", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1)]), 1n, 1n);
    expect(cache.needsResync()).toBe(false);
    const out = cache.merge(
      PROJECTION_KEY,
      encodeBatch(false, [{ key: "a", rev: 2, state: RefRowState.Changed }]),
      1n,
      1n,
    );
    expect(out.decodeFailed).toBe(true);
    expect(cache.needsResync()).toBe(true);
    expect(decode(cache.payload(PROJECTION_KEY, "a"))).toBe("payload:a");
  });

  it("fail CLOSED on an unknown row-state discriminant — reject WHOLE batch, retain cache, latch resync", () => {
    // A batch-level decode failure (unlike the old hand-written `RefRowCache`
    // this generator replaces) DOES latch `needsResync` here — matching the
    // Swift/Kotlin twins, which flag every decode failure (empty payload,
    // missing NRRD id, malformed buffer, bad state discriminant) the same way,
    // so a host always has one signal to force a resync on ANY wire garbage.
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1)]), 1n, 1n);
    expect(cache.needsResync()).toBe(false);
    const bogus = encodeBatch(false, [
      { key: "a", rev: 2, state: 255 as RefRowState, payload: payloadFor("a") },
    ]);
    const out = cache.merge(PROJECTION_KEY, bogus, 1n, 1n);
    expect(out.changedKeys).toEqual([]);
    expect(out.decodeFailed).toBe(false);
    expect(cache.needsResync()).toBe(true);
    expect(decode(cache.payload(PROJECTION_KEY, "a"))).toBe("payload:a");
  });

  it("a baseline rebuilds the full set and drops ghost rows", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    const out = cache.merge(
      PROJECTION_KEY,
      encodeBatch(true, [changed("a", 2), changed("c", 1)]),
      1n,
      1n,
    );
    expect([...cache.snapshot(PROJECTION_KEY).keys()].sort()).toEqual(["a", "c"]);
    expect(cache.payload(PROJECTION_KEY, "b")).toBeUndefined();
    expect(out.changedKeys.sort()).toEqual(["b", "c"]);
  });

  it("an identity (epoch) change rebuilds from the new baseline", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("x", 1)]), 1n, 2n);
    expect([...cache.snapshot(PROJECTION_KEY).keys()]).toEqual(["x"]);
    expect(cache.payload(PROJECTION_KEY, "a")).toBeUndefined();
  });

  it("an identity-change baseline that rebuilds to EMPTY drops every prior row", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1), changed("b", 1)]), 1n, 1n);
    expect(cache.snapshot(PROJECTION_KEY).size).toBe(2);
    cache.merge(PROJECTION_KEY, encodeBatch(true, []), 1n, 2n);
    expect(cache.snapshot(PROJECTION_KEY).size).toBe(0);
  });

  it("a MALFORMED first baseline after an identity bump RETAINS the prior cache (fail-closed)", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1)]), 1n, 1n);
    const out = cache.merge(
      PROJECTION_KEY,
      encodeBatch(true, [{ key: "a", rev: 9, state: RefRowState.Changed }]),
      1n,
      2n,
    );
    expect(out.decodeFailed).toBe(true);
    expect(cache.needsResync()).toBe(true);
    expect(decode(cache.payload(PROJECTION_KEY, "a"))).toBe("payload:a");
  });

  it("a garbage (non-NRRD) sidecar is a fail-closed no-op that latches resync", () => {
    const cache = new KeyedRefCache();
    cache.merge(PROJECTION_KEY, encodeBatch(true, [changed("a", 1)]), 1n, 1n);
    const out = cache.merge(PROJECTION_KEY, new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]), 1n, 1n);
    expect(out.changedKeys).toEqual([]);
    expect(cache.needsResync()).toBe(true);
    expect(cache.payload(PROJECTION_KEY, "a")).toBeDefined();
  });

  it("an unregistered projectionKey no-ops (routing gate)", () => {
    const cache = new KeyedRefCache();
    const out = cache.merge("nmp.not.a.keyed.projection", encodeBatch(true, [changed("a", 1)]), 1n, 1n);
    expect(out.changedKeys).toEqual([]);
    expect(cache.payload("nmp.not.a.keyed.projection", "a")).toBeUndefined();
  });
});
