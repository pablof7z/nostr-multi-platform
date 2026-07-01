import * as flatbuffers from "flatbuffers";
import { createSignal, type Accessor } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

import { FrameKind, UpdateFrame } from "./generated/nmp/transport";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import {
  eventCorrelationId,
  encodeNpub,
  protocolVersion,
  type RuntimeStatus,
  type WorkerEvent,
  type WorkerRequest,
} from "@nmp/runtime-web";
import {
  decodeEmbedSidecar,
  EMBED_SIDECAR_KEY,
  NEMB_FILE_IDENTIFIER,
} from "./embedSidecarStore";
import { RefEventStore, REFS_EVENT_KEY, type ClaimedEventWire } from "./refEventStore";
import { RefProfileStore } from "./refProfileStore";
import type { EventRefResolver } from "@nmp/components-web/component-host";
import type { EmbeddedEventModel } from "@nmp/components-web/content-kind-registry";
import type { NostrProfileHost, ProfileWire } from "@nmp/components-web/user-avatar";

export { tagValue, type ClaimedEventWire } from "./refEventStore";

// ADR-0063 Lane D wire codes (mirror the wasm structured `resolve_ref` /
// `release_ref` protocol):
//   namespace: 0 = profile, 1 = event
//   shape:     profile → 0 = ref, 1 = card;  event → 0 = embed, 1 = raw
//   liveness:  0 = CacheOk, 1 = Live
const REF_NS_PROFILE = 0;
const REF_NS_EVENT = 1;
const REF_SHAPE_PROFILE_REF = 0;
const REF_SHAPE_EVENT_EMBED = 0;
const REF_SHAPE_EVENT_RAW = 1;
const REF_LIVENESS_CACHE_OK = 0;

const NRRD_FILE_IDENTIFIER = "NRRD";
const REFS_PROFILE_PROJECTION_KEY = "refs.profile";

export type RelayStatusRow = {
  url: string;
  role: string;
  connection: string;
};

export type GalleryRuntime = {
  /** The profile host wired into the registry user-* components. */
  host: NostrProfileHost;
  /** The event-ref resolver wired into the component host provider. */
  eventRefResolver: EventRefResolver;
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
   *  fetches route to the content lane, so the gallery issues its demand after
   *  this edge and leaves buffering/lifecycle mechanics to the runtime. */
  anyContentConnected: Accessor<boolean>;
  /** Reactive — number of resolved profiles currently held. */
  resolvedCount: Accessor<number>;
  /** Claim a single event by raw event key. `hints` and `eventAuthor` are
   *  optional metadata decoded by the app boundary from a NIP-19/NIP-21 URI. */
  claimEvent(key: string, consumerId: string, hints?: string[], eventAuthor?: string): void;
  /** Release an event claim when the owning component/view no longer needs it. */
  releaseEvent(key: string, consumerId: string): void;
  /** Reactive — a claimed event keyed by its `primary_id`, or undefined until
   *  the kernel resolves it. */
  claimedEvent: (primaryId: string) => ClaimedEventWire | undefined;
  /** Reactive — a render-facing embed envelope derived from the authoritative
   *  `refs.event` row, keyed by `primary_id`. Undefined until the event row
   *  resolves. */
  refEventEnvelope: (primaryId: string) => EmbeddedEventModel | undefined;
  /** Reactive — all render-facing embed envelopes decoded from `refs.event.envelopes`. */
  refEventEmbeds: Accessor<Map<string, EmbeddedEventModel>>;
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
function findTypedSidecar(snapshot: SnapshotFrame, key: string, fileIdentifier: string): Uint8Array | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== key) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== fileIdentifier) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    return payloadBytes;
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
  const [refEventEnvelopes, setRefEventEnvelopes] = createSignal<Map<string, EmbeddedEventModel>>(new Map());
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
  const refEvents = new RefEventStore();

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
      const refsPayload = findTypedSidecar(snap, REFS_PROFILE_PROJECTION_KEY, NRRD_FILE_IDENTIFIER);
      if (refsPayload !== undefined) {
        refProfiles.applySidecar(refsPayload, snap.sessionId(), snap.snapshotEpoch());
        const cards = refProfiles.profiles();
        const next: Record<string, ProfileWire> = {};
        for (const [k, v] of cards) next[k] = v;
        setProfiles(reconcile(next, { merge: true }));
        setResolvedCount(cards.size);
      }
      const eventsPayload = findTypedSidecar(snap, REFS_EVENT_KEY, NRRD_FILE_IDENTIFIER);
      if (eventsPayload !== undefined) {
        refEvents.applySidecar(eventsPayload, snap.sessionId(), snap.snapshotEpoch());
        setClaimedEvents(refEvents.events());
      }
      const embedsPayload = findTypedSidecar(snap, EMBED_SIDECAR_KEY, NEMB_FILE_IDENTIFIER);
      if (embedsPayload !== undefined) {
        const embeds = decodeEmbedSidecar(embedsPayload);
        if (embeds !== undefined) setRefEventEnvelopes(embeds);
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
    resolveProfileRef(pubkey: string, consumerId: string): void {
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
    releaseProfileRef(pubkey: string, consumerId: string): void {
      void request({
        type: "release_ref",
        namespace: REF_NS_PROFILE,
        key: pubkey,
        consumer_id: consumerId,
        correlation_id: `release-${claimSeq++}`,
      });
    },
  };

  const eventRefResolver: EventRefResolver = {
    resolveEventRef(target): void {
      void request({
        type: "resolve_ref",
        namespace: REF_NS_EVENT,
        key: target.primaryId,
        consumer_id: target.consumerId,
        shape: REF_SHAPE_EVENT_EMBED,
        liveness: REF_LIVENESS_CACHE_OK,
        hints: target.relays,
        event_author: target.author ?? null,
        correlation_id: `resolve-event-embed-${claimSeq++}`,
      });
    },
    releaseEventRef(target): void {
      void request({
        type: "release_ref",
        namespace: REF_NS_EVENT,
        key: target.primaryId,
        consumer_id: target.consumerId,
        correlation_id: `release-event-embed-${claimSeq++}`,
      });
    },
  };

  return {
    host,
    eventRefResolver,
    async start(relayList) {
      await helloReady;
      await request({
        type: "start",
        app_id: "nmp-gallery",
        relays: relayList.map((relay) => relay.url),
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
    claimEvent(key: string, consumerId: string, hints: string[] = [], eventAuthor?: string) {
      void request({
        type: "resolve_ref",
        namespace: REF_NS_EVENT,
        key,
        consumer_id: consumerId,
        shape: REF_SHAPE_EVENT_RAW,
        liveness: REF_LIVENESS_CACHE_OK,
        hints,
        event_author: eventAuthor ?? null,
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
    refEventEnvelope: (primaryId: string) => refEventEnvelopes().get(primaryId),
    refEventEmbeds: refEventEnvelopes,
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
