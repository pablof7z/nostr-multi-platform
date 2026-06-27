// feedStore.ts — reactive SolidJS store for the Chirp home feed + profile resolution.
//
// This is the single state manager for Item C. It:
//   1. Subscribes to snapshot bytes from `useSnapshot()`.
//   2. Decodes the `nmp.feed.home` FlatBuffers projection into typed FeedRow objects.
//   3. Applies the `refs.profile` NRRD sidecar into the persistent RefProfileStore.
//   4. Exposes a `NostrProfileHost` that components can mount via NostrProfileHostProvider.
//   5. Provides `resolveProfileRef` / `releaseProfileRef` dispatch helpers.
//
// Zero protocol logic — the decode comes from feedDecoder.ts (reusing gallery's
// FlatBuffers generated classes); all Nostr networking stays in the wasm kernel.

import { createSignal, createEffect } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import type { NostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import type { ProfileWire } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { decodeUpdateFrame, type FeedRow } from "./feedDecoder";
import { RefProfileStore } from "./refProfileStore";
import { resolveProfileCommand, releaseProfileCommand } from "./actions";
import { useSnapshot, useNmpClient } from "./context";

/** All reactive state exposed by the feed store. */
export type FeedStoreState = {
  /** Current feed rows (empty until the first frame arrives). */
  rows: FeedRow[];
  /** True once at least one home-feed frame has been decoded. */
  ready: boolean;
};

/** The return type of `createFeedStore`. */
export type FeedStore = {
  state: FeedStoreState;
  /** `NostrProfileHost` to mount via `<NostrProfileHostProvider host={...}>`. */
  profileHost: NostrProfileHost;
};

let _claimSeq = 0;
function nextConsumerId(prefix: string): string {
  return `${prefix}-${_claimSeq++}`;
}

function rowSignature(rows: FeedRow[]): string {
  return rows
    .map((row) =>
      [
        row.id,
        row.kind,
        row.createdAt,
        row.authorPubkey,
        row.authorDisplayName ?? "",
        row.content,
        row.relationCounts.replies,
        row.relationCounts.reactions,
        row.relationCounts.reposts,
        row.relationCounts.zaps,
        row.relationCounts.comments,
        row.replyAttributions
          .map((reply) =>
            [
              reply.authorPubkey,
              reply.authorDisplayName ?? "",
              reply.replyEventId,
              reply.replyCreatedAt,
            ].join(","),
          )
          .join(";"),
        row.relayProvenance.join(","),
      ].join(":"),
    )
    .join("|");
}

/** Create the Chirp feed store. Must be called inside a SolidJS reactive root
 *  (component body or createRoot). Uses NmpClientContext internally. */
export function createFeedStore(): FeedStore {
  const snapshot = useSnapshot();
  const { client } = useNmpClient();

  // Persistent profile ref cache (ADR-0063 — stateful across frames).
  const refProfiles = new RefProfileStore();

  // SolidJS store for profiles: keyed by pubkey, reactive per key.
  const [profiles, setProfiles] = createStore<Record<string, ProfileWire>>({});

  // Feed rows: plain signal — FeedRow objects are plain data, safe for signals.
  const [rows, setRows] = createSignal<FeedRow[]>([]);
  const [ready, setReady] = createSignal(false);
  let lastRowsSignature = "";

  // Reactive effect: run whenever the raw snapshot bytes change.
  createEffect(() => {
    const bytes = snapshot().latestUpdateBytes;
    if (!bytes) return;

    const decoded = decodeUpdateFrame(bytes);
    if (!decoded) return;

    // Apply the refs.profile sidecar if present (ADR-0063 stateful merge).
    if (decoded.refsProfileBytes) {
      refProfiles.applySidecar(
        decoded.refsProfileBytes,
        decoded.sessionId,
        decoded.snapshotEpoch,
      );
      const cards = refProfiles.profiles();
      const next: Record<string, ProfileWire> = {};
      for (const [k, v] of cards) next[k] = v;
      setProfiles(reconcile(next, { merge: true }));
    }

    // Update feed rows.
    const nextRowsSignature = rowSignature(decoded.rows);
    if (!ready() || (decoded.rows.length > 0 && nextRowsSignature !== lastRowsSignature)) {
      lastRowsSignature = nextRowsSignature;
      setRows(decoded.rows);
      setReady(true);
    }
  });

  // NostrProfileHost implementation for the components-web avatar/name components.
  const profileHost: NostrProfileHost = {
    profile(pubkey: string): ProfileWire | undefined {
      return profiles[pubkey];
    },
    resolveProfileRef(pubkey: string, consumerId: string): void {
      void client.dispatchCommand(resolveProfileCommand(pubkey, consumerId));
    },
    releaseProfileRef(pubkey: string, consumerId: string): void {
      void client.dispatchCommand(releaseProfileCommand(pubkey, consumerId));
    },
  };

  return {
    state: {
      get rows() {
        return rows();
      },
      get ready() {
        return ready();
      },
    },
    profileHost,
  };
}

export { nextConsumerId };
