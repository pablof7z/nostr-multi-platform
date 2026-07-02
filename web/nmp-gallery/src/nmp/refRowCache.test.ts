// ADR-0070 Lane A — vitest for the TypeScript `RefRowCache` merge engine.
//
// Asserts the correctness-critical invariants that MUST match the Rust reference
// (crates/nmp-core/src/refs/cache.rs) and the generated Swift/Kotlin caches:
//   - clear DROPS the row (REPLACE-don't-accumulate);
//   - the NEGATIVE: an absent row is RETAINED (Unchanged != Cleared);
//   - decode-before-commit retains the prior row + latches needsResync;
//   - fail CLOSED on an unknown row-state discriminant (reject batch, retain cache);
//   - a baseline rebuilds the full set + drops ghosts;
//   - a session/epoch identity change rebuilds from the new baseline, and a
//     malformed first baseline after the bump RETAINS the prior cache.
import { describe, expect, it } from "vitest";
import * as flatbuffers from "flatbuffers";

import { RefRow } from "./generated/nmp/refs/ref-row";
import { RefRowDeltaBatch } from "./generated/nmp/refs/ref-row-delta-batch";
import { RefRowState } from "./generated/nmp/refs/ref-row-state";
import { RefRowCache } from "./refRowCache";

// ── NRRD batch builders ──────────────────────────────────────────────────────

type RowSpec = { key: string; rev: number; state: RefRowState; payload?: Uint8Array };

function encodeBatch(namespace: string, baseline: boolean, rows: RowSpec[]): Uint8Array {
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
  const nsOff = builder.createString(namespace);
  RefRowDeltaBatch.startRefRowDeltaBatch(builder);
  RefRowDeltaBatch.addNamespace(builder, nsOff);
  RefRowDeltaBatch.addBaseline(builder, baseline);
  RefRowDeltaBatch.addRows(builder, rowsVec);
  const root = RefRowDeltaBatch.endRefRowDeltaBatch(builder);
  RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, root);
  return builder.asUint8Array().slice();
}

// A trivial namespace payload: the raw bytes of the key (UTF-8). decodeOk treats
// a non-empty payload as decodable; empty as a decode failure (models a malformed
// typed payload). This keeps the cache test independent of the KPRF profile codec.
function payloadFor(key: string): Uint8Array {
  return new TextEncoder().encode(`payload:${key}`);
}
const decodeOk = (_key: string, bytes: Uint8Array) => bytes.length > 0;

function changed(key: string, rev: number): RowSpec {
  return { key, rev, state: RefRowState.Changed, payload: payloadFor(key) };
}
function cleared(key: string, rev: number): RowSpec {
  return { key, rev, state: RefRowState.Cleared };
}

const NS = "profile";

function decode(bytes: Uint8Array | undefined): string | undefined {
  return bytes ? new TextDecoder().decode(bytes) : undefined;
}

describe("RefRowCache merge (ADR-0070)", () => {
  it("a baseline seeds rows; a Changed delta updates one key", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    expect(cache.baselined()).toBe(true);
    expect([...cache.snapshot(NS).keys()].sort()).toEqual(["a", "b"]);

    const out = cache.applySidecar(encodeBatch(NS, false, [changed("a", 2)]), 1n, 1n, decodeOk);
    expect(out.changedKeys).toEqual(["a"]);
    expect(decode(cache.get(NS, "a"))).toBe("payload:a");
    // b is untouched.
    expect(cache.get(NS, "b")).toBeDefined();
  });

  it("clear DROPS the row (REPLACE, not accumulate)", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    const out = cache.applySidecar(encodeBatch(NS, false, [cleared("a", 2)]), 1n, 1n, decodeOk);
    expect(out.changedKeys).toEqual(["a"]);
    // The row is GONE, not retained with stale bytes.
    expect(cache.get(NS, "a")).toBeUndefined();
    expect(cache.get(NS, "b")).toBeDefined();
  });

  it("the NEGATIVE — an ABSENT row is RETAINED (Unchanged != Cleared)", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    // A delta touching only `a`. `b` is ABSENT from the batch → must be RETAINED.
    cache.applySidecar(encodeBatch(NS, false, [changed("a", 2)]), 1n, 1n, decodeOk);
    expect(cache.get(NS, "b")).toBeDefined();
    expect(decode(cache.get(NS, "b"))).toBe("payload:b");
  });

  it("a stale (older-rev) clear can NOT delete a newer live row", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 5)]), 1n, 1n, decodeOk);
    // Reordered stale clear at rev 3 < cached rev 5 → ignored.
    const out = cache.applySidecar(encodeBatch(NS, false, [cleared("a", 3)]), 1n, 1n, decodeOk);
    expect(out.changedKeys).toEqual([]);
    expect(cache.get(NS, "a")).toBeDefined();
  });

  it("decode-before-commit: a malformed Changed ROW RETAINS the prior row + latches resync", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1)]), 1n, 1n, decodeOk);
    expect(cache.needsResync()).toBe(false);
    // rev advances but payload is empty → decodeOk returns false. The BATCH
    // decodes fine (valid NRRD), so the cache's per-row decode-before-commit path
    // runs and latches resync (matches Rust cache.rs apply_incremental).
    const out = cache.applySidecar(
      encodeBatch(NS, false, [{ key: "a", rev: 2, state: RefRowState.Changed }]),
      1n,
      1n,
      decodeOk,
    );
    expect(out.decodeFailed).toBe(true);
    expect(cache.needsResync()).toBe(true);
    // The prior row survives unchanged.
    expect(decode(cache.get(NS, "a"))).toBe("payload:a");
  });

  it("fail CLOSED on an unknown row-state discriminant — reject WHOLE batch, retain cache", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1)]), 1n, 1n, decodeOk);
    expect(cache.needsResync()).toBe(false);
    // Hand-craft a batch with state = 255 (neither Changed=0 nor Cleared=1). The
    // generated RefRow.state() reads a raw uint8, so a naive `=== Cleared ?
    // Cleared : Changed` would COERCE 255 to Changed and commit a bogus rev-2
    // row (fail-open). decodeRefRowDeltaBatch must instead reject the WHOLE batch.
    const bogus = encodeBatch(NS, false, [{ key: "a", rev: 2, state: 255 as RefRowState, payload: payloadFor("a") }]);
    const out = cache.applySidecar(bogus, 1n, 1n, decodeOk);
    // The batch is rejected at DECODE (before apply), so this is the garbage-batch
    // no-op path — identical to Rust host_store.rs `garbage_sidecar_is_fail_closed_noop`
    // + tests/failclosed.rs `invalid_state_discriminant_rejects_batch`: no changes,
    // prior cache intact, and resync NOT latched (the per-row resync path is only
    // reached for a VALID batch carrying a malformed row payload — see the test
    // above). Latching here would diverge from the canonical Rust cache.
    expect(out.changedKeys).toEqual([]);
    expect(out.decodeFailed).toBe(false);
    expect(cache.needsResync()).toBe(false);
    // The bogus rev-2 row was NEVER committed — the rev-1 payload survives.
    expect(decode(cache.get(NS, "a"))).toBe("payload:a");
  });

  it("a baseline rebuilds the full set and drops ghost rows", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    // New baseline omits `b` → `b` is a ghost that must be dropped.
    const out = cache.applySidecar(encodeBatch(NS, true, [changed("a", 2), changed("c", 1)]), 1n, 1n, decodeOk);
    expect([...cache.snapshot(NS).keys()].sort()).toEqual(["a", "c"]);
    expect(cache.get(NS, "b")).toBeUndefined();
    expect(out.changedKeys.sort()).toEqual(["b", "c"]); // a unchanged-bytes? payload same → only b dropped + c added
  });

  it("an identity (epoch) change rebuilds from the new baseline", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    // Epoch bumps to 2 with a fresh baseline carrying only `x`.
    cache.applySidecar(encodeBatch(NS, true, [changed("x", 1)]), 1n, 2n, decodeOk);
    expect([...cache.snapshot(NS).keys()]).toEqual(["x"]);
    expect(cache.get(NS, "a")).toBeUndefined();
  });

  it("an identity-change baseline that rebuilds to EMPTY drops every prior row", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1), changed("b", 1)]), 1n, 1n, decodeOk);
    expect(cache.snapshot(NS).size).toBe(2);
    // Epoch bumps to 2 with an EMPTY baseline (the kernel cleared the whole set).
    // The materialised snapshot MUST become empty — a host that gates on
    // changedKeys alone would otherwise show stale cards (codex BLOCKING).
    cache.applySidecar(encodeBatch(NS, true, []), 1n, 2n, decodeOk);
    expect(cache.snapshot(NS).size).toBe(0);
    expect(cache.get(NS, "a")).toBeUndefined();
    expect(cache.get(NS, "b")).toBeUndefined();
  });

  it("a MALFORMED first baseline after an identity bump RETAINS the prior cache (fail-closed)", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1)]), 1n, 1n, decodeOk);
    // Epoch bumps but the baseline's only row fails decode (empty payload).
    const out = cache.applySidecar(
      encodeBatch(NS, true, [{ key: "a", rev: 9, state: RefRowState.Changed }]),
      1n,
      2n,
      decodeOk,
    );
    expect(out.decodeFailed).toBe(true);
    expect(cache.needsResync()).toBe(true);
    // The prior cache (epoch 1) survives — a garbage baseline never empties it.
    expect(decode(cache.get(NS, "a"))).toBe("payload:a");
  });

  it("a garbage (non-NRRD) sidecar is a fail-closed no-op", () => {
    const cache = new RefRowCache();
    cache.applySidecar(encodeBatch(NS, true, [changed("a", 1)]), 1n, 1n, decodeOk);
    const out = cache.applySidecar(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]), 1n, 1n, decodeOk);
    expect(out.changedKeys).toEqual([]);
    expect(cache.get(NS, "a")).toBeDefined();
  });
});
