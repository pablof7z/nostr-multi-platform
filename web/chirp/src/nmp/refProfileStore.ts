// ADR-0063 Lane F — host-side `refs.profile` consumption helper (TypeScript).
//
// The kernel emits `refs.profile` as a per-KEY row-delta projection: each tick's
// sidecar payload is an NRRD `RefRowDeltaBatch` carrying only the changed/cleared
// rows (or a full baseline on identity change / first attach). A consumer that
// wants `profile(pubkey)` therefore CANNOT decode one frame in isolation — it
// must maintain the stateful per-key cache the row-deltas merge into. `RefRowCache`
// is that canonical merge engine; this wrapper specialises it to the `"profile"`
// namespace and decodes each cached row payload (a KPRF `ProfileSnapshot`
// carrying one `ProfileCard`) into a `ProfileWire`.
//
// This is the ONLY app-side mirror of hydrated profile facts (D4 / invariant v):
// the web host holds one `RefProfileStore` (the `RefRowCache` mirror), never a
// second `Map<pubkey, ProfileWire>` of its own. It replaces the whole-map
// `resolved_profiles` (KRPR) decode that previously rebuilt the entire profile
// map every frame.

import * as flatbuffers from "flatbuffers";

import type { ProfileWire } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { ProfileSnapshot } from "./generated/nmp/kernel/profile-snapshot";
import { RefRowCache, type RefRowApplyOutcome } from "./refRowCache";

/** The kernel-emitted projection key for the profile resolver. The sidecar entry
 *  is keyed by `refs.profile`; the NRRD batch inside carries the bare `"profile"`
 *  namespace token (the cache keys on that). */
export const REFS_PROFILE_KEY = "refs.profile";
const REFS_PROFILE_NAMESPACE = "profile";

/** Decode a KPRF `ProfileSnapshot` row payload into a `ProfileWire`, or
 *  `undefined` when the bytes are not a well-formed ProfileSnapshot. Mirrors the
 *  Rust `decode_profile` used as the host_store decode-before-commit preflight. */
function decodeProfileRow(bytes: Uint8Array): ProfileWire | undefined {
  if (bytes.length < 8) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ProfileSnapshot.bufferHasIdentifier(bb)) return undefined;
    const snap = ProfileSnapshot.getRootAsProfileSnapshot(bb);
    const card = snap.card();
    if (!card) return undefined;
    const key = card.pubkey();
    if (key === null) return undefined;
    const wire: ProfileWire = { pubkey: key };
    if (card.hasDisplayName()) {
      const v = card.displayName();
      if (v) wire.displayName = v;
    }
    if (card.hasPictureUrl()) {
      const v = card.pictureUrl();
      if (v) wire.pictureUrl = v;
    }
    const nip05 = card.nip05();
    if (nip05) wire.nip05 = nip05;
    const about = card.about();
    if (about) wire.about = about;
    if (card.hasLnurl()) {
      const v = card.lnurl();
      if (v) wire.lnurl = v;
    }
    return wire;
  } catch {
    return undefined;
  }
}

/** Host-side consumer of the `refs.profile` row-delta projection. Holds the
 *  persistent `RefRowCache` the per-key deltas merge into and exposes typed
 *  `profile(pubkey)` / `profiles()` lookups. One instance lives for the lifetime
 *  of the client's update loop (NOT rebuilt per frame — the cache is stateful). */
export class RefProfileStore {
  private cache = new RefRowCache();

  /** Apply one frame's `refs.profile` sidecar payload (an encoded NRRD batch)
   *  under the frame's `(sessionId, snapshotEpoch)` identity. A malformed payload
   *  is a fail-closed no-op (D6); decode-before-commit of each KPRF row payload is
   *  enforced by the cache via the `decodeProfileRow` preflight. */
  applySidecar(payload: Uint8Array, sessionId: bigint, snapshotEpoch: bigint): RefRowApplyOutcome {
    return this.cache.applySidecar(
      payload,
      sessionId,
      snapshotEpoch,
      (_key, bytes) => decodeProfileRow(bytes) !== undefined,
    );
  }

  /** The decoded `ProfileWire` for `pubkey`, or `undefined` if no live ref is
   *  cached. Reads the kernel-pushed typed row directly — there is no second
   *  app-side cache (D4 / invariant v). */
  profile(pubkey: string): ProfileWire | undefined {
    const payload = this.cache.get(REFS_PROFILE_NAMESPACE, pubkey);
    if (!payload) return undefined;
    return decodeProfileRow(payload);
  }

  /** The full materialised `pubkey -> ProfileWire` set currently cached. Rows
   *  whose payload fails to decode are skipped (they cannot be in the cache:
   *  decode-before-commit gates entry). */
  profiles(): Map<string, ProfileWire> {
    const out = new Map<string, ProfileWire>();
    for (const [key, payload] of this.cache.snapshot(REFS_PROFILE_NAMESPACE)) {
      const wire = decodeProfileRow(payload);
      if (wire) out.set(key, wire);
    }
    return out;
  }

  /** Whether the underlying cache has applied a baseline (UI-gating flag). */
  baselined(): boolean {
    return this.cache.baselined();
  }
}
