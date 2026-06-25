import { createContext, useContext, type JSX } from "solid-js";

import type { ClaimedEventWire } from "../../nmp/refEventStore";
import type { EmbeddedEventModel } from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry";

export interface NostrEventHost {
  event(primaryId: string): ClaimedEventWire | undefined;
  /** Kernel-resolved, kind-dispatched embed envelope for a claimed event
   *  (#1767 / #1998). Present once the `claimed_event_embeds_json` sidecar
   *  surfaces the entry; drives typed card dispatch (article / highlight /
   *  quote). Undefined falls back to the raw-event quote card. */
  embed(primaryId: string): EmbeddedEventModel | undefined;
  claimEvent(primaryId: string, consumerId: string, hints?: string[], author?: string): void;
  releaseEvent(primaryId: string, consumerId: string): void;
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

export function useNostrEventHost(): NostrEventHost {
  const host = useContext(NostrEventHostContext);
  if (!host) {
    throw new Error(
      "NostrEventHost is missing - wrap your tree in <NostrEventHostProvider host={...}>.",
    );
  }
  return host;
}
