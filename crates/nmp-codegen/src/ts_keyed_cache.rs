//! ADR-0063 Lane A twin (#2722) — generated per-key (row-keyed) reference cache
//! for TypeScript/web. The THIRD platform twin of [`crate::swift_keyed_cache`] /
//! [`crate::kotlin_keyed_cache`]: decodes the `nmp.refs.RefRowDeltaBatch`
//! payload and merges row deltas under the five ADR-0063 invariants,
//! byte-for-byte semantically identical to the Swift/Kotlin caches and the
//! Rust reference model (`nmp_core::refs::RefRowCache`).
//!
//! Generates `web/packages/runtime-web/src/keyedRefCache.generated.ts` from
//! [`KEYED_PROJECTIONS`]. Before this generator existed, the algorithm was
//! hand-maintained separately in hl's `refRowCache.ts` / `refProfileStore.ts`
//! (#2722) and in `web/nmp-gallery/src/nmp/refRowCache.ts` — every web
//! consumer re-implementing (and risking drift on) the same merge logic the
//! Swift/Kotlin generators already derive from this registry. This generator
//! collapses both hand-written files into one class: `KeyedRefCache` owns the
//! raw per-namespace row merge (was `RefRowCache`) AND the typed per-namespace
//! accessors (was `RefProfileStore`), exactly mirroring the Swift/Kotlin shape
//! where `KeyedRefCache.profile(pubkey)` is a method on the ONE cache class.
//!
//! Unlike Swift/Kotlin (native, per-row `PassthroughSubject`/listener
//! reactivity), the TS twin keeps the proven pull-based API
//! (`merge(...) -> RefRowApplyOutcome`) the existing hand-written
//! implementations established — that shape is what Solid/React-style stores
//! want (derive-then-`reconcile` each frame), not a push event bus. The
//! row-grain algorithm itself (scratch-then-commit baseline, decode-before-
//! commit, rev-safe clears, deferred identity reset) is otherwise identical.

use std::path::Path;

use crate::swift_projections_registry::{KeyedProjectionEntry, KEYED_PROJECTIONS};

// ADR-0063 Lane G twin (#2722): the TYPED row-payload rendering (accessors +
// decode-before-commit routing table) lives in a sibling file so neither
// source exceeds the 500-LOC cap (the Swift/Kotlin `*_keyed_cache_typed.rs` twins).
#[path = "ts_keyed_cache_typed.rs"]
mod typed;
use typed::{render_accessors, render_row_decoder_table, render_typed_decode_fns};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache --platform ts \\
//       --out web/packages/runtime-web/src/keyedRefCache.generated.ts
//
// Source of truth: KEYED_PROJECTIONS in
// `crates/nmp-codegen/src/keyed_projection_row_payload.rs`.
// The CI gate (`.github/workflows/codegen-drift.yml`) fails any PR whose
// generated TypeScript differs.
//
// ADR-0063 Lane A twin (#2722): per-key row cache for keyed reference
// projections (`refs.profile` / `refs.event`) — byte-for-byte semantically
// identical to `KeyedRefCache.generated.swift`, `KeyedRefCache.kt`, and
// `nmp_core::refs::RefRowCache`.
// ─────────────────────────────────────────────────────────────────────────────

import * as flatbuffers from \"flatbuffers\";

import { RefRowDeltaBatch as RefRowDeltaBatchFb } from \"./generated/nmp/refs/ref-row-delta-batch.js\";
import { RefRowState } from \"./generated/nmp/refs/ref-row-state.js\";
import { ProfileSnapshot } from \"./generated/nmp/kernel/profile-snapshot.js\";
import { refRowProfile, type ProfileWire } from \"./refRowDecoders.js\";

/** Decoded row state. Mirrors the Rust/Swift/Kotlin `RefRowState` — Unchanged
 *  is represented by ABSENCE of the row, never an enum value. */
type DecodedRowState = \"changed\" | \"cleared\";

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
      state = \"changed\";
    } else if (rawState === RefRowState.Cleared) {
      state = \"cleared\";
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

";

/// Outcome of a `--check` run. Mirrors [`crate::swift_keyed_cache::KeyedRefCacheCheckOutcome`].
#[derive(Debug)]
pub struct TsKeyedRefCacheCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the TypeScript `KeyedRefCache` source from the keyed-projection registry.
#[must_use]
pub fn render_ts_keyed_ref_cache(entries: &[KeyedProjectionEntry]) -> String {
    let mut out = String::from(HEADER);
    out.push_str(&render_typed_decode_fns(entries));
    out.push_str(&render_class_open());
    out.push_str(&render_routing(entries));
    out.push_str(&render_row_decoder_table(entries));
    out.push_str(STATIC_MERGE);
    out.push_str(&render_accessors(entries));
    out.push_str("}\n");
    out
}

/// A `projectionKey` not in the registry is not a keyed reference projection —
/// `merge()` no-ops rather than polluting the row cache with an unrecognized
/// key. Mirrors the Swift/Kotlin `namespace(forProjectionKey:)` gate exactly,
/// just without the projectionKey -> namespace translation (this generator
/// keys rows by projectionKey directly — see the module doc).
fn render_routing(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "  /** Whether `projectionKey` is a registered keyed reference projection.\n\
         \x20\x20 *  Mirrors the Swift/Kotlin `namespace(forProjectionKey:)` gate. */\n\
         \x20\x20private static isKeyedProjection(projectionKey: string): boolean {\n\
         \x20\x20\x20\x20switch (projectionKey) {\n",
    );
    for e in entries {
        s.push_str(&format!("      case {:?}:\n", e.projection_key));
    }
    s.push_str(
        "        return true;\n\
         \x20\x20\x20\x20\x20\x20default:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return false;\n\
         \x20\x20\x20\x20}\n\
         \x20\x20}\n\n",
    );
    s
}

fn render_class_open() -> String {
    "/**\n\
     \x20* NMP-owned per-key row cache for keyed reference projections (ADR-0063).\n\
     \x20*\n\
     \x20* Thread-safety: fed only from the worker's `update_bytes` handler on the\n\
     \x20* single main-thread event loop — no concurrent `merge()` calls.\n\
     \x20*/\n\
     export class KeyedRefCache {\n\
     \x20\x20// projectionKey -> (rowKey -> entry). Keyed by the CALLER's projectionKey\n\
     \x20\x20// (e.g. \"refs.profile\"), never the payload's self-reported namespace —\n\
     \x20\x20// mirrors the Swift/Kotlin twins exactly.\n\
     \x20\x20private rows = new Map<string, Map<string, CachedRow>>();\n\
     \x20\x20private appliedSession = 0n;\n\
     \x20\x20private appliedEpoch = 0n;\n\
     \x20\x20private baselinedFlag = false;\n\
     \x20\x20private needsResyncFlag = false;\n\n\
     \x20\x20/** D3-5: false until the first post-baseline frame is applied. */\n\
     \x20\x20baselined(): boolean {\n\
     \x20\x20\x20\x20return this.baselinedFlag;\n\
     \x20\x20}\n\n\
     \x20\x20/** D3-4: latches on any per-row decode-before-commit failure. */\n\
     \x20\x20needsResync(): boolean {\n\
     \x20\x20\x20\x20return this.needsResyncFlag;\n\
     \x20\x20}\n\n\
     \x20\x20/** Hard-reset so the next frame is a full baseline. */\n\
     \x20\x20reset(): void {\n\
     \x20\x20\x20\x20this.rows.clear();\n\
     \x20\x20\x20\x20this.appliedSession = 0n;\n\
     \x20\x20\x20\x20this.appliedEpoch = 0n;\n\
     \x20\x20\x20\x20this.baselinedFlag = false;\n\
     \x20\x20\x20\x20this.needsResyncFlag = false;\n\
     \x20\x20}\n\n\
     \x20\x20/**\n\
     \x20\x20 * The cached raw payload bytes for one (projectionKey, rowKey), or\n\
     \x20\x20 * `undefined`. Low-level merge primitive — NOT the public refs API.\n\
     \x20\x20 * Prefer the typed per-namespace accessor below (invariant #4: no\n\
     \x20\x20 * dishonest raw surface for a namespace that HAS a typed decoder); this\n\
     \x20\x20 * stays exported only for namespaces with `ts: None` (raw-bytes-only).\n\
     \x20\x20 */\n\
     \x20\x20payload(projectionKey: string, rowKey: string): Uint8Array | undefined {\n\
     \x20\x20\x20\x20return this.rows.get(projectionKey)?.get(rowKey)?.payload;\n\
     \x20\x20}\n\n\
     \x20\x20/** The full cached raw `rowKey -> payload` set for one projectionKey. */\n\
     \x20\x20snapshot(projectionKey: string): Map<string, Uint8Array> {\n\
     \x20\x20\x20\x20const out = new Map<string, Uint8Array>();\n\
     \x20\x20\x20\x20const ns = this.rows.get(projectionKey);\n\
     \x20\x20\x20\x20if (ns) for (const [k, v] of ns) out.set(k, v.payload);\n\
     \x20\x20\x20\x20return out;\n\
     \x20\x20}\n\n"
        .to_string()
}

const STATIC_MERGE: &str = r#"
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

"#;

/// Write the generated TypeScript file to `out_path`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_ts_keyed_ref_cache(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_ts_keyed_ref_cache(KEYED_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_ts_keyed_ref_cache(out_path: &Path) -> std::io::Result<TsKeyedRefCacheCheckOutcome> {
    let rendered = render_ts_keyed_ref_cache(KEYED_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TsKeyedRefCacheCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(TsKeyedRefCacheCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(TsKeyedRefCacheCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "ts_keyed_cache_tests.rs"]
mod tests;
