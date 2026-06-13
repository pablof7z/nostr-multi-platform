import * as flatbuffers from "flatbuffers";
import { createSignal, type Accessor } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

import { FrameKind, UpdateFrame } from "./generated/nmp/transport";
import { ResolvedProfilesSnapshot } from "./generated/nmp/kernel/resolved-profiles-snapshot";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import {
  eventCorrelationId,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
} from "./protocol";
import type { ProfileWire } from "../components/user-avatar/ProfileWire";
import type { NostrProfileHost } from "../components/user-avatar/NostrProfileHost";

const KRPR_FILE_IDENTIFIER = "KRPR";
const KRPR_PROJECTION_KEY = "resolved_profiles";

export type RelayStatusRow = {
  url: string;
  role: string;
  connection: string;
};

export type GalleryRuntime = {
  /** The profile host wired into the registry user-* components. */
  host: NostrProfileHost;
  /** Boot the kernel against the given relays and return once Start is acked. */
  start(relays: { url: string; role: string }[]): Promise<void>;
  /** Reactive — current runtime status. */
  status: Accessor<RuntimeStatus>;
  /** Reactive — per-relay connection rows from the latest snapshot. */
  relays: Accessor<RelayStatusRow[]>;
  /** Reactive — true once at least one relay reports a connected state. */
  anyRelayConnected: Accessor<boolean>;
  /** Reactive — number of resolved profiles currently held. */
  resolvedCount: Accessor<number>;
};

// ── Profile decode ───────────────────────────────────────────────────────────

/** Decode the resolved_profiles (KRPR) typed projection from a snapshot frame
 *  into a full pubkey→ProfileWire map. Returns `undefined` on missing/corrupt
 *  projection so the caller can keep the last good map. */
function decodeProfileCards(snapshot: SnapshotFrame): Map<string, ProfileWire> | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== KRPR_PROJECTION_KEY) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== KRPR_FILE_IDENTIFIER) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    try {
      const bb = new flatbuffers.ByteBuffer(payloadBytes);
      if (!ResolvedProfilesSnapshot.bufferHasIdentifier(bb)) return undefined;
      const root = ResolvedProfilesSnapshot.getRootAsResolvedProfilesSnapshot(bb);
      const out = new Map<string, ProfileWire>();
      for (let j = 0; j < root.entriesLength(); j += 1) {
        const entry = root.entries(j);
        if (!entry) continue;
        const key = entry.key();
        const card = entry.value();
        if (!key || !card) continue;
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
        out.set(key, wire);
      }
      return out;
    } catch {
      return undefined;
    }
  }
  return undefined;
}

function decodeRelays(snapshot: SnapshotFrame): RelayStatusRow[] {
  const rows: RelayStatusRow[] = [];
  for (let i = 0; i < snapshot.relayStatusesLength(); i += 1) {
    const r = snapshot.relayStatuses(i);
    if (!r) continue;
    const url = r.relayUrl();
    if (!url) continue;
    rows.push({ url, role: r.role() ?? "", connection: r.connection() ?? "" });
  }
  return rows;
}

// ── Runtime ────────────────────────────────────────────────────────────────

export function createGalleryRuntime(): GalleryRuntime {
  // Reactive stores. `profiles` is a keyed store so user-* components only
  // re-render when their pubkey's ProfileWire actually changes.
  const [profiles, setProfiles] = createStore<Record<string, ProfileWire>>({});
  const [status, setStatus] = createSignal<RuntimeStatus>("ready");
  const [relays, setRelays] = createSignal<RelayStatusRow[]>([]);
  const [resolvedCount, setResolvedCount] = createSignal(0);

  if (typeof Worker === "undefined") {
    throw new Error(
      "Web Worker API is unavailable — the gallery requires a real kernel worker (no degraded mode in the showcase).",
    );
  }

  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  const pending = new Map<string, () => void>();
  let resolveHello: (() => void) | undefined;
  const helloReady = new Promise<void>((resolve) => {
    resolveHello = resolve;
  });

  function ingestBytes(bytes: Uint8Array) {
    try {
      const bb = new flatbuffers.ByteBuffer(bytes);
      if (!UpdateFrame.bufferHasIdentifier(bb)) return;
      const frame = UpdateFrame.getRootAsUpdateFrame(bb);
      if (frame.kind() !== FrameKind.Snapshot) return;
      const snap = frame.snapshot();
      if (!snap) return;
      if (snap.running()) setStatus("running");
      setRelays(decodeRelays(snap));
      const cards = decodeProfileCards(snap);
      if (cards !== undefined) {
        const next: Record<string, ProfileWire> = {};
        for (const [k, v] of cards) next[k] = v;
        // reconcile keeps referential identity per-key so only changed
        // profiles trigger downstream re-renders.
        setProfiles(reconcile(next, { merge: true }));
        setResolvedCount(cards.size);
      }
    } catch {
      // Keep last-good state on a corrupt frame (D6 — never blank-reset).
    }
  }

  function accept(event: WorkerEvent) {
    if (event.type === "hello_accepted") {
      setStatus(event.status);
      resolveHello?.();
    } else if (event.type === "runtime_status") {
      setStatus(event.status);
    } else if (event.type === "update_bytes") {
      const bytes = event.bytes instanceof Uint8Array ? event.bytes : new Uint8Array(event.bytes);
      ingestBytes(bytes);
    }
    const cid = eventCorrelationId(event);
    if (cid) {
      const resolve = pending.get(cid);
      if (resolve) {
        pending.delete(cid);
        resolve();
      }
    }
  }

  worker.onmessage = (message: MessageEvent<WorkerEvent>) => accept(message.data);

  worker.postMessage({
    type: "hello",
    app_id: "nmp-gallery",
    platform: "web",
    protocol_version: protocolVersion,
  } satisfies WorkerRequest);

  function request(req: WorkerRequest): Promise<void> {
    const cid = "correlation_id" in req ? req.correlation_id : undefined;
    if (!cid) {
      worker.postMessage(req);
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      pending.set(cid, resolve);
      worker.postMessage(req);
    });
  }

  let claimSeq = 0;

  const host: NostrProfileHost = {
    profile(pubkey: string): ProfileWire | undefined {
      return profiles[pubkey];
    },
    claimProfile(pubkey: string, consumerId: string): void {
      void request({
        type: "dispatch",
        action_type: "nmp.kernel.claim_profile",
        payload: { pubkey, consumer_id: consumerId },
        correlation_id: `claim-${claimSeq++}`,
      });
    },
    releaseProfile(pubkey: string, consumerId: string): void {
      void request({
        type: "dispatch",
        action_type: "nmp.kernel.release_profile",
        payload: { pubkey, consumer_id: consumerId },
        correlation_id: `release-${claimSeq++}`,
      });
    },
  };

  return {
    host,
    async start(relayList) {
      await helloReady;
      await request({
        type: "start",
        app_id: "nmp-gallery",
        database_name: "nmp-gallery-web",
        correlation_id: "gallery-start",
        relay_bootstrap: relayList,
      });
    },
    status,
    relays,
    anyRelayConnected: () => relays().some((r) => r.connection.toLowerCase() === "connected"),
    resolvedCount,
  };
}
