import * as flatbuffers from "flatbuffers";
import { createSignal, type Accessor } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

import { FrameKind, UpdateFrame } from "@nmp/wire-ts/nmp/transport";
import { ResolvedProfilesSnapshot } from "@nmp/wire-ts/nmp/kernel/resolved-profiles-snapshot";
import { ClaimedEventsSnapshot } from "@nmp/wire-ts/nmp/kernel/claimed-events-snapshot";
import { ContentTreeWire } from "@nmp/wire-ts/nmp/content/content-tree-wire";
import type { SnapshotFrame } from "@nmp/wire-ts/nmp/transport/snapshot-frame";
import {
  eventCorrelationId,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
  encodeNpub,
} from "@nmp/runtime-web";
import type { ProfileWire } from "@nmp/components";
import type { NostrProfileHost } from "@nmp/components";

const KRPR_FILE_IDENTIFIER = "KRPR";
const KRPR_PROJECTION_KEY = "resolved_profiles";
const KCEV_FILE_IDENTIFIER = "KCEV";
const KCEV_PROJECTION_KEY = "claimed_events";

export type RelayStatusRow = {
  url: string;
  role: string;
  connection: string;
};

/** One resolved+enriched event from the kernel's `claimed_events` projection.
 *  `contentTree` is the kernel-parsed NFCT tree (`nmp-content` behind the
 *  content-parser seam); `content` is the raw NIP-01 string fallback. */
export type ClaimedEventWire = {
  primaryId: string;
  id: string;
  kind: number;
  content: string;
  createdAt: number;
  /** Author pubkey (hex). */
  authorPubkey: string;
  /** Author's resolved kind:0 display name, when the kernel enriched it. */
  authorDisplayName?: string;
  /** Author's resolved kind:0 picture URL, when the kernel enriched it. */
  authorPictureUrl?: string;
  /** Raw event tags (array of tag rows). Embed cards read `title`/`image`/
   *  `summary`/`context`/source tags from here. */
  tags: string[][];
  /** Decoded NFCT tree, present iff the kernel emitted non-empty NFCT bytes. */
  contentTree?: ContentTreeWire;
};

/** First value of the first tag row whose tag name (index 0) matches `name`. */
export function tagValue(ev: ClaimedEventWire, name: string): string | undefined {
  const row = ev.tags.find((t) => t[0] === name);
  return row && row.length > 1 ? row[1] : undefined;
}

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
  /** Reactive — true once a connected relay carries the `indexer` role. kind:0
   *  profile REQs route to the indexer lane, so claims must wait for this. */
  anyIndexerConnected: Accessor<boolean>;
  /** Reactive — true once a connected relay carries the `content` role. Event-id
   *  fetches (claim_event) route to the content lane, so event claims must wait
   *  for this — claiming before the content socket is open drops the REQ (the
   *  wasm transport has no retry/on-demand dial). */
  anyContentConnected: Accessor<boolean>;
  /** Reactive — number of resolved profiles currently held. */
  resolvedCount: Accessor<number>;
  /** Claim a single event by `nostr:` URI (nevent/naddr). Routes through the
   *  content-relay lane (Discovery seed), independent of the indexer lane. */
  claimEvent(uri: string, consumerId: string): void;
  /** Release an event claim. Dropping the last consumer clears the kernel's
   *  "already requested" dedupe state, so a subsequent `claimEvent` re-issues a
   *  fresh REQ — the basis of the cold-start re-claim retry. */
  releaseEvent(uri: string, consumerId: string): void;
  /** Reactive — a claimed event keyed by its `primary_id`, or undefined until
   *  the kernel resolves it. */
  claimedEvent: (primaryId: string) => ClaimedEventWire | undefined;
  /** Encode a hex pubkey to its NIP-19 bech32 npub (pure-TS, no actor round-trip).
   *  Returns undefined for invalid pubkeys. aim.md §6.9 — canonical format. */
  encodeNpub: (pubkey: string) => { npub: string; npubShort: string } | null;
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

/** Decode the claimed_events (KCEV) typed projection from a snapshot frame into
 *  a primary_id→ClaimedEventWire map. Each event's `content_tree_bytes` (the
 *  kernel-parsed NFCT) is decoded into a `ContentTreeWire` when present; empty
 *  bytes leave `contentTree` undefined (the view then renders raw `content`).
 *  Returns `undefined` on missing/corrupt projection so the caller keeps the
 *  last good map (D6 — never blank-reset). */
function decodeClaimedEvents(snapshot: SnapshotFrame): Map<string, ClaimedEventWire> | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== KCEV_PROJECTION_KEY) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== KCEV_FILE_IDENTIFIER) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    try {
      const bb = new flatbuffers.ByteBuffer(payloadBytes);
      if (!ClaimedEventsSnapshot.bufferHasIdentifier(bb)) return undefined;
      const root = ClaimedEventsSnapshot.getRootAsClaimedEventsSnapshot(bb);
      const out = new Map<string, ClaimedEventWire>();
      for (let j = 0; j < root.entriesLength(); j += 1) {
        const entry = root.entries(j);
        if (!entry) continue;
        const key = entry.key();
        const ev = entry.value();
        if (!key || !ev) continue;
        const tags: string[][] = [];
        for (let t = 0; t < ev.tagsLength(); t += 1) {
          const row = ev.tags(t);
          if (!row) continue;
          const values: string[] = [];
          for (let v = 0; v < row.valuesLength(); v += 1) {
            values.push((row.values(v) as string) ?? "");
          }
          tags.push(values);
        }
        const wire: ClaimedEventWire = {
          primaryId: key,
          id: ev.id() ?? "",
          kind: ev.kind(),
          content: ev.content() ?? "",
          createdAt: Number(ev.createdAt()),
          authorPubkey: ev.authorPubkey() ?? "",
          authorDisplayName: ev.hasAuthorDisplayName() ? ev.authorDisplayName() ?? undefined : undefined,
          authorPictureUrl: ev.hasAuthorPictureUrl() ? ev.authorPictureUrl() ?? undefined : undefined,
          tags,
        };
        const ctBytes = ev.contentTreeBytesArray();
        if (ctBytes && ctBytes.length > 0) {
          try {
            const ctBb = new flatbuffers.ByteBuffer(ctBytes);
            if (ContentTreeWire.bufferHasIdentifier(ctBb)) {
              wire.contentTree = ContentTreeWire.getRootAsContentTreeWire(ctBb);
            }
          } catch {
            // Corrupt NFCT bytes — leave contentTree undefined (raw fallback).
          }
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
  // A plain signal, NOT a deep store: a ClaimedEventWire carries a flatbuffers
  // `ContentTreeWire` holding a live ByteBuffer + prototype accessors. Solid's
  // store proxy would wrap that object and break its `this.bb`-based accessors,
  // so the map is kept as an opaque signal value (replaced wholesale per frame).
  const [claimedEvents, setClaimedEvents] = createSignal<Map<string, ClaimedEventWire>>(new Map());
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
      const events = decodeClaimedEvents(snap);
      if (events !== undefined) setClaimedEvents(events);
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
    anyIndexerConnected: () =>
      relays().some(
        (r) => r.connection.toLowerCase() === "connected" && r.role.includes("indexer"),
      ),
    anyContentConnected: () =>
      relays().some(
        (r) => r.connection.toLowerCase() === "connected" && r.role.includes("content"),
      ),
    resolvedCount,
    claimEvent(uri: string, consumerId: string) {
      void request({
        type: "dispatch",
        action_type: "nmp.kernel.claim_event",
        payload: { uri, consumer_id: consumerId },
        correlation_id: `claim-event-${claimSeq++}`,
      });
    },
    releaseEvent(uri: string, consumerId: string) {
      void request({
        type: "dispatch",
        action_type: "nmp.kernel.release_event",
        payload: { uri, consumer_id: consumerId },
        correlation_id: `release-event-${claimSeq++}`,
      });
    },
    claimedEvent: (primaryId: string) => claimedEvents().get(primaryId),
    // Pure-TS NIP-19 encoder — no actor round-trip needed (aim.md §6.9).
    encodeNpub,
  };
}
