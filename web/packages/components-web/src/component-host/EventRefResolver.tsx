import { createContext, useContext, type JSX } from "solid-js";
import type { WireNostrUriKind } from "../generated/nmp/content/wire-nostr-uri-kind";

export type EventRefTarget = {
  uri: string;
  primaryId: string;
  kind: WireNostrUriKind;
  relays: string[];
  author?: string;
  eventKind?: number;
  consumerId: string;
};

export interface EventRefResolver {
  resolveEventRef(target: EventRefTarget): void;
  releaseEventRef(target: EventRefTarget): void;
}

const EventRefResolverContext = createContext<EventRefResolver>();

export function EventRefResolverProvider(props: {
  resolver: EventRefResolver;
  children: JSX.Element;
}): JSX.Element {
  return (
    <EventRefResolverContext.Provider value={props.resolver}>
      {props.children}
    </EventRefResolverContext.Provider>
  );
}

export function useOptionalEventRefResolver(): EventRefResolver | undefined {
  return useContext(EventRefResolverContext);
}

export function useEventRefResolver(): EventRefResolver {
  const resolver = useOptionalEventRefResolver();
  if (!resolver) {
    throw new Error(
      "EventRefResolver is missing - wrap your tree in <EventRefResolverProvider resolver={...}>.",
    );
  }
  return resolver;
}
