import { createContext, useContext, type JSX } from "solid-js";

import type { ClaimedEventWire } from "../../nmp/refEventStore";

export interface NostrEventHost {
  event(primaryId: string): ClaimedEventWire | undefined;
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
