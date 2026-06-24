import * as flatbuffers from "flatbuffers";
import { createSignal, type Accessor } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

import { FrameKind, UpdateFrame } from "./generated/nmp/transport";
import { ClaimedEventsSnapshot } from "./generated/nmp/kernel/claimed-events-snapshot";
import { ContentTreeWire } from "./generated/nmp/content/content-tree-wire";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import {
  eventCorrelationId,
  encodeNpub,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
} from "@nmp/runtime-web";
import { RefProfileStore } from "./refProfileStore";
import type { ProfileWire } from "@nmp/components-web/src/user-avatar/ProfileWire";
import type { NostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import type { EmbeddedEventModel } from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";

// ADR-0063 Lane D wire codes (mirror the wasm `resolve_dispatch_from_action`
// recognizer):
//   namespace: 0 = profile, 1 = event
//   shape:     profile → 0 = ref, 1 = card;  event → 0 = embed, 1 = raw
//   liveness:  0 = CacheOk, 1 = Live
const REF_NS_PROFILE = 0;
const REF_NS_EVENT = 1;
const REF_SHAPE_PROFILE_REF = 0;
const REF_SHAPE_EVENT_EMBED = 0;
const REF_LIVENESS_CACHE_OK = 0;

const NRRD_FILE_IDENTIFIER = "NRRD";
const REFS_PROFILE_PROJECTION_KEY = "refs.profile";
const KCEV_FILE_IDENTIFIER = "KCEV";
const KCEV_PROJECTION_KEY = "claimed_events";
// #1767 — the JSON embed-projection sidecar key. The payload is UTF-8
// `serde_json` of `{ [primaryId]: EmbeddedEventEnvelope }`, the `nmp-content`
// resolver output / `EmbeddedEventEnvelope` serde shape this web TS decodes.
// (iOS decodes a DIFFERENT wire format — the native `claimed_event_embeds`
// NEMB FlatBuffer — which shares the resolution logic but not this JSON.) The
// kernel/composition root has already kind-dispatched each embed
// (`projection.variant`), so the web renders from the resolved projection
// instead of re-parsing NIP-23 / NIP-84 tags.
const EMBED_PROJECTION_KEY = "claimed_event_embeds_json";

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
   *  fetches route to the content lane, so event claims must wait
   *  for this — claiming before the content socket is open drops the REQ (the
   *  wasm transport has no retry/on-demand dial). */
  anyContentConnected: Accessor<boolean>;
  /** Reactive — number of resolved profiles currently held. */
  resolvedCount: Accessor<number>;
  /** Claim a single event by raw event key. `hints` are optional relay hints
   *  decoded by the app boundary from a NIP-19/NIP-21 URI. */
  claimEvent(key: string, consumerId: string, hints?: string[]): void;
  /** Release an event claim. Dropping the last consumer clears the kernel's
   *  "already requested" dedupe state, so a subsequent `claimEvent` re-issues a
   *  fresh REQ — the basis of the cold-start re-claim retry. */
  releaseEvent(key: string, consumerId: string): void;
  /** Reactive — a claimed event keyed by its `primary_id`, or undefined until
   *  the kernel resolves it. */
  claimedEvent: (primaryId: string) => ClaimedEventWire | undefined;
  /** Reactive — the kernel-RESOLVED embed envelope for a claimed event, keyed
   *  by `primary_id` (#1767). The `projection` is already kind-dispatched in
   *  Rust; the registry renders from it without re-parsing tags. Undefined
   *  until the embed sidecar surfaces the resolved entry. */
  claimedEventEmbed: (primaryId: string) => EmbeddedEventModel | undefined;
  /** Request the Rust-encoded npub for a pubkey (idempotent; fires once per
   *  pubkey). The result lands reactively in `npub(pubkey)`. */
  requestNpub: (pubkey: string) => void;
  /** Reactive — the Rust-encoded `{ npub, npubShort }` for a pubkey, or
   *  undefined until `requestNpub` resolves. */
  npub: (pubkey: string) => { npub?: string; npubShort?: string } | undefined;
};

// ── Profile decode ───────────────────────────────────────────────────────────

/** Extract the raw `refs.profile` (NRRD) sidecar payload bytes from a snapshot
 *  frame, or `undefined` when the projection is absent / empty / wrong file id.
 *  The bytes are an NRRD `RefRowDeltaBatch` the stateful `RefProfileStore`
 *  (`RefRowCache`) merges per-key — never decoded in isolation. A frame with no
 *  `refs.profile` entry returns `undefined` and the caller leaves the persistent
 *  cache untouched (keep-last-good). */
function findRefsProfileSidecar(snapshot: SnapshotFrame): Uint8Array | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== REFS_PROFILE_PROJECTION_KEY) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== NRRD_FILE_IDENTIFIER) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    return payloadBytes;
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

/** Decode the `claimed_event_embeds_json` typed projection (#1767) into a
 *  primary_id→EmbeddedEventModel map. The payload is UTF-8 `serde_json` of the
 *  resolved embed map (NOT a FlatBuffer — Option A / JSON parity with iOS), so
 *  it is `JSON.parse`d directly; the kernel has already kind-dispatched each
 *  `projection`. Returns `undefined` on a missing/corrupt projection so the
 *  caller keeps the last good map (D6 — never blank-reset). */
function decodeClaimedEventEmbeds(snapshot: SnapshotFrame): Map<string, EmbeddedEventModel> | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== EMBED_PROJECTION_KEY) continue;
    const payload = proj.payload();
    if (!payload) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    try {
      const text = new TextDecoder().decode(payloadBytes);
      const parsed = JSON.parse(text) as Record<string, EmbeddedEventModel>;
      const out = new Map<string, EmbeddedEventModel>();
      for (const [key, envelope] of Object.entries(parsed)) {
        if (envelope && typeof envelope === "object" && envelope.projection) {
          out.set(key, envelope);
        }
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
  // #1767 — kernel-resolved embed envelopes, keyed by primary_id. Plain JSON
  // objects (decoded from the JSON sidecar), so a plain signal is fine.
  const [claimedEventEmbeds, setClaimedEventEmbeds] = createSignal<Map<string, EmbeddedEventModel>>(
    new Map(),
  );
  const [status, setStatus] = createSignal<RuntimeStatus>("ready");
  const [relays, setRelays] = createSignal<RelayStatusRow[]>([]);
  const [resolvedCount, setResolvedCount] = createSignal(0);

  if (typeof Worker === "undefined") {
    throw new Error(
      "Web Worker API is unavailable — the gallery requires a real kernel worker (no degraded mode in the showcase).",
    );
  }

  // Rust-encoded npub/npubShort per pubkey (aim.md §6.9 — never browser-encoded).
  const [npubs, setNpubs] = createStore<Record<string, { npub?: string; npubShort?: string }>>({});
  const requestedNpubs = new Set<string>();

  // Stateful per-key `refs.profile` row-delta cache (ADR-0063). Lives for the
  // runtime's lifetime — row deltas merge into it across frames. Replaces the
  // whole-map `resolved_profiles` decode.
  const refProfiles = new RefProfileStore();

  const worker = new Worker(new URL("@nmp/runtime-web/worker", import.meta.url), { type: "module" });
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
      // Merge the `refs.profile` row-delta sidecar into the stateful store under
      // THIS frame's identity (session_id, snapshot_epoch). The store handles
      // baseline/incremental/identity-rebuild + decode-before-commit + fail-closed
      // internally (ADR-0063). After every applied sidecar we re-derive the FULL
      // set (like native desktop/tui) and feed it through `reconcile`, which keeps
      // per-key referential identity so only profiles that actually changed
      // trigger downstream re-renders — AND drops rows the kernel cleared (an
      // identity/epoch baseline that shrinks the set to empty is reflected, where
      // gating on `changedKeys` alone would leave stale cards visible).
      const refsPayload = findRefsProfileSidecar(snap);
      if (refsPayload !== undefined) {
        refProfiles.applySidecar(refsPayload, snap.sessionId(), snap.snapshotEpoch());
        const cards = refProfiles.profiles();
        const next: Record<string, ProfileWire> = {};
        for (const [k, v] of cards) next[k] = v;
        setProfiles(reconcile(next, { merge: true }));
        setResolvedCount(cards.size);
      }
      const events = decodeClaimedEvents(snap);
      if (events !== undefined) setClaimedEvents(events);
      // #1767 — decode the resolved embed sidecar (kind-dispatched in Rust).
      const embeds = decodeClaimedEventEmbeds(snap);
      if (embeds !== undefined) setClaimedEventEmbeds(embeds);
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
        type: "resolve_ref",
        namespace: REF_NS_PROFILE,
        key: pubkey,
        consumer_id: consumerId,
        shape: REF_SHAPE_PROFILE_REF,
        liveness: REF_LIVENESS_CACHE_OK,
        correlation_id: `resolve-${claimSeq++}`,
      });
    },
    releaseProfile(pubkey: string, consumerId: string): void {
      void request({
        type: "release_ref",
        namespace: REF_NS_PROFILE,
        key: pubkey,
        consumer_id: consumerId,
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
    claimEvent(key: string, consumerId: string, hints: string[] = []) {
      void request({
        type: "resolve_ref",
        namespace: REF_NS_EVENT,
        key,
        consumer_id: consumerId,
        shape: REF_SHAPE_EVENT_EMBED,
        liveness: REF_LIVENESS_CACHE_OK,
        hints,
        correlation_id: `claim-event-${claimSeq++}`,
      });
    },
    releaseEvent(key: string, consumerId: string) {
      void request({
        type: "release_ref",
        namespace: REF_NS_EVENT,
        key,
        consumer_id: consumerId,
        correlation_id: `release-event-${claimSeq++}`,
      });
    },
    claimedEvent: (primaryId: string) => claimedEvents().get(primaryId),
    claimedEventEmbed: (primaryId: string) => claimedEventEmbeds().get(primaryId),
    requestNpub(pubkey: string) {
      if (requestedNpubs.has(pubkey)) return;
      requestedNpubs.add(pubkey);
      // Calls the Rust NIP-19 encoder directly via the wasm free function
      // (no worker round-trip — encodeNpub loads the wasm module lazily on the
      // main thread; the binary is already cached from the worker's load).
      void encodeNpub(pubkey).then((result) => {
        if (result) setNpubs(pubkey, result);
      });
    },
    npub: (pubkey: string) => npubs[pubkey],
  };
}
