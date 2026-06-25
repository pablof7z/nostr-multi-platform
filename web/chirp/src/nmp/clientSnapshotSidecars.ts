import type { EmbeddedEventModel } from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";
import type { ProfileWire } from "../components/user-avatar/ProfileWire";
import {
  decodeClaimedEventEmbeds,
  decodeHomeFeed,
  findRefsEventSidecar,
  findRefsProfileSidecar,
  type FeedItem,
} from "./feedProjection";
import { claimedEventsEqual, RefEventStore, type ClaimedEventWire } from "./refEventStore";
import { profileCardsEqual } from "./profileCards";
import { RefProfileStore } from "./refProfileStore";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";

/** Mutable sidecar state bucket threaded into/out of applySnapshotSidecars. */
export type SidecarState = {
  latestFeedItems: FeedItem[] | undefined;
  latestProfileCards: Map<string, ProfileWire> | undefined;
  latestEventCards: Map<string, ClaimedEventWire> | undefined;
  latestEventEmbeds: Map<string, EmbeddedEventModel> | undefined;
};

/**
 * Second-pass over a decoded Snapshot FlatBuffer: materialises feed items,
 * profile-ref cards, event-ref cards, and embed envelopes.  Keep-last-good
 * semantics: fields not present in this frame leave the prior value intact.
 *
 * Extracted from BaseClient.record() so that client.ts stays below the
 * 500-LOC ceiling (#1998).
 */
export function applySnapshotSidecars(
  snap: SnapshotFrame,
  refProfiles: RefProfileStore,
  refEvents: RefEventStore,
  state: SidecarState,
): void {
  const feedResult = decodeHomeFeed(snap);
  if (feedResult !== undefined) {
    state.latestFeedItems = feedResult.items;
  }

  const refsPayload = findRefsProfileSidecar(snap);
  if (refsPayload !== undefined) {
    refProfiles.applySidecar(refsPayload, snap.sessionId(), snap.snapshotEpoch());
    const next = refProfiles.profiles();
    if (!profileCardsEqual(state.latestProfileCards, next)) {
      state.latestProfileCards = next;
    }
  }

  const eventPayload = findRefsEventSidecar(snap);
  if (eventPayload !== undefined) {
    refEvents.applySidecar(eventPayload, snap.sessionId(), snap.snapshotEpoch());
    const next = refEvents.events();
    if (!claimedEventsEqual(state.latestEventCards, next)) {
      state.latestEventCards = next;
    }
  }

  // #1767 — kernel-resolved embed envelopes (kind-dispatched in Rust).
  // Keep-last-good: a frame without the sidecar leaves the prior map intact.
  const embeds = decodeClaimedEventEmbeds(snap);
  if (embeds !== undefined) {
    state.latestEventEmbeds = embeds;
  }
}
