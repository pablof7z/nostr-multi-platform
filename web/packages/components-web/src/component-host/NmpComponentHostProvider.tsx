import type { Accessor, JSX } from "solid-js";
import {
  createDefaultNostrKindRegistry,
  NostrKindRegistryProvider,
  type NostrKindRegistry,
} from "../content-kind-registry/NostrKindRegistry";
import {
  NostrProfileHostProvider,
  type NostrProfileHost,
} from "../user-avatar/NostrProfileHost";
import {
  EventRefResolverProvider,
  type EventRefResolver,
} from "./EventRefResolver";
import {
  ResolvedEventEmbedsProvider,
  type ResolvedEventEmbeds,
} from "./ResolvedEventEmbeds";

export type NmpComponentHostProviderProps = {
  profileHost: NostrProfileHost;
  resolvedEventEmbeds: ResolvedEventEmbeds | Accessor<ResolvedEventEmbeds>;
  eventRefResolver: EventRefResolver;
  kindRegistry?: NostrKindRegistry;
  children: JSX.Element;
};

/**
 * One app-root binding point for the web component host. The app still owns the
 * browser runtime, snapshots, account lifecycle, projection merge, and reset
 * semantics; this provider only makes already-decoded host data available to
 * pure Solid components.
 */
export function NmpComponentHostProvider(props: NmpComponentHostProviderProps): JSX.Element {
  const registry = () => props.kindRegistry ?? createDefaultNostrKindRegistry();
  return (
    <NostrProfileHostProvider host={props.profileHost}>
      <ResolvedEventEmbedsProvider resolvedEventEmbeds={props.resolvedEventEmbeds}>
        <EventRefResolverProvider resolver={props.eventRefResolver}>
          <NostrKindRegistryProvider registry={registry()}>
            {props.children}
          </NostrKindRegistryProvider>
        </EventRefResolverProvider>
      </ResolvedEventEmbedsProvider>
    </NostrProfileHostProvider>
  );
}
