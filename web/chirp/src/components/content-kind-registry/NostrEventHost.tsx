import { createContext, useContext, type JSX } from "solid-js";
import type { EmbeddedEventModel } from "./NostrKindRegistry";

// Host bridge for embedded/quoted Nostr events (the content-view EventRef →
// embed-card seam). The host app (Chirp) owns the kernel: it claims interest in
// a referenced event on `claimEvent` (the kernel sends a REQ and resolves +
// kind:0-enriches the event into the `claimed_events` / KCEV projection),
// releases interest on `releaseEvent`, and exposes the resolved
// `EmbeddedEventModel` reactively through `claimedEvent`.
//
// This is the event twin of `NostrProfileHost` (avatar/name components) — the
// EventRef node claims on mount, releases on cleanup, and renders whatever
// `claimedEvent(primaryId)` currently returns. The lookup key is the EventRef's
// `WireNostrUri.primaryId()` (hex event id for nevent/note, "{kind}:{pubkey}:
// {d_tag}" coordinate for naddr) — the kernel keys the KCEV map by the same
// value, so the lookup hits without an alias map.
//
// Mirrors the SwiftUI/Compose embed-host contract so the seam is identical
// across platforms.
export interface NostrEventHost {
  /** Reactive accessor — returns the resolved+enriched embed model for an
   *  event's `primaryId`, or `undefined` until the kernel has resolved it.
   *  Must be called inside a tracking scope to update on resolution. */
  claimedEvent(primaryId: string): EmbeddedEventModel | undefined;
  /** Register interest in a `nostr:` event URI. The kernel sends a REQ on the
   *  first claim (refcounted by `consumerId`) and resolves the event into the
   *  KCEV projection. `uri` is the full `nostr:nevent1…` / `nostr:naddr1…`. */
  claimEvent(uri: string, consumerId: string): void;
  /** Drop interest. The kernel can garbage-collect the subscription once every
   *  consumer releases. `uri` must be the same URI passed to `claimEvent`. */
  releaseEvent(uri: string, consumerId: string): void;
}

const NostrEventHostContext = createContext<NostrEventHost>();

export function NostrEventHostProvider(props: {
  host: NostrEventHost;
  children: JSX.Element;
}): JSX.Element {
  return (
    <NostrEventHostContext.Provider value={props.host}>
      {props.children}
    </NostrEventHostContext.Provider>
  );
}

/** Read the ambient event host, or `undefined` when no provider is mounted.
 *
 *  Unlike `useNostrProfileHost` (which throws), this is intentionally optional:
 *  `NostrContentView` is a pure tree walker reusable without an event host, and
 *  an EventRef with no host degrades to the honest raw-URI affordance rather
 *  than crashing the render. Wrap the feed in `<NostrEventHostProvider>` to
 *  enable embed cards. */
export function useNostrEventHost(): NostrEventHost | undefined {
  return useContext(NostrEventHostContext);
}
