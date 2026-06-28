import { createSignal, type Accessor } from "solid-js";
import type { WireNostrUriKind } from "../generated/nmp/content/wire-nostr-uri-kind";
import { WireNodeKind } from "../generated/nmp/content/wire-node-kind";
import { WireNostrUriKind as WireNostrUriKindValue } from "../generated/nmp/content/wire-nostr-uri-kind";
import type { ContentTreeWire } from "../generated/nmp/content/content-tree-wire";
import type { WireNode } from "../generated/nmp/content/wire-node";
import type { EmbeddedEventModel } from "../content-kind-registry/NostrKindRegistry";
import type { ProfileWire } from "../user-avatar/ProfileWire";
import type { NostrProfileHost } from "../user-avatar/NostrProfileHost";
import type { EventRefResolver, EventRefTarget } from "./EventRefResolver";
import type { ResolvedEventEmbeds } from "./ResolvedEventEmbeds";

export const COMPONENT_HOST_FIXTURE_KEYS = {
  refsProfile: "refs.profile",
  refsEvent: "refs.event",
  refsEventEnvelopes: "refs.event.envelopes",
} as const;

export const COMPONENT_HOST_FIXTURE_PUBKEY =
  "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
export const COMPONENT_HOST_FIXTURE_EVENT_ID =
  "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
export const COMPONENT_HOST_FIXTURE_EVENT_URI = "nostr:nevent1componenthost";

export const COMPONENT_HOST_FIXTURE_PROFILE: ProfileWire = {
  pubkey: COMPONENT_HOST_FIXTURE_PUBKEY,
  displayName: "Conformance Alice",
  about: "Profile row supplied by refs.profile.",
  pictureUrl: "https://example.invalid/alice.png",
  nip05: "alice@example.invalid",
  npubShort: "npub1component...fixture",
};

export const COMPONENT_HOST_FIXTURE_EMBED: EmbeddedEventModel = {
  uri: COMPONENT_HOST_FIXTURE_EVENT_URI,
  primaryId: COMPONENT_HOST_FIXTURE_EVENT_ID,
  collapsed: false,
  projection: {
    variant: "shortNote",
    data: {
      id: COMPONENT_HOST_FIXTURE_EVENT_ID,
      authorPubkey: COMPONENT_HOST_FIXTURE_PUBKEY,
      createdAt: 1_700_000_000,
      contentTree: {
        nodes: [
          { kind: "text", text: "Event render data supplied by refs.event.envelopes." },
        ],
        roots: [0],
      },
      mediaUrls: [],
    },
  },
};

export type ComponentHostConformanceFixture = {
  profileHost: NostrProfileHost;
  eventRefResolver: EventRefResolver;
  resolvedEventEmbeds: Accessor<ResolvedEventEmbeds>;
  resolvedEventRefs: EventRefTarget[];
  releasedEventRefs: EventRefTarget[];
  resolvedProfiles: Array<{ pubkey: string; consumerId: string }>;
  releasedProfiles: Array<{ pubkey: string; consumerId: string }>;
};

export function createComponentHostConformanceFixture(): ComponentHostConformanceFixture {
  const [profiles] = createSignal(
    new Map<string, ProfileWire>([
      [COMPONENT_HOST_FIXTURE_PUBKEY, COMPONENT_HOST_FIXTURE_PROFILE],
    ]),
  );
  const [embeds] = createSignal<ResolvedEventEmbeds>(
    new Map<string, EmbeddedEventModel>([
      [COMPONENT_HOST_FIXTURE_EVENT_ID, COMPONENT_HOST_FIXTURE_EMBED],
      [COMPONENT_HOST_FIXTURE_EVENT_URI, COMPONENT_HOST_FIXTURE_EMBED],
    ]),
  );

  const resolvedProfiles: Array<{ pubkey: string; consumerId: string }> = [];
  const releasedProfiles: Array<{ pubkey: string; consumerId: string }> = [];
  const resolvedEventRefs: EventRefTarget[] = [];
  const releasedEventRefs: EventRefTarget[] = [];

  return {
    profileHost: {
      profile(pubkey) {
        return profiles().get(pubkey);
      },
      resolveProfileRef(pubkey, consumerId) {
        resolvedProfiles.push({ pubkey, consumerId });
      },
      releaseProfileRef(pubkey, consumerId) {
        releasedProfiles.push({ pubkey, consumerId });
      },
    },
    eventRefResolver: {
      resolveEventRef(target) {
        resolvedEventRefs.push(target);
      },
      releaseEventRef(target) {
        releasedEventRefs.push(target);
      },
    },
    resolvedEventEmbeds: embeds,
    resolvedEventRefs,
    releasedEventRefs,
    resolvedProfiles,
    releasedProfiles,
  };
}

export function componentHostEventRefTree(): ContentTreeWire {
  const nostrUri = {
    uri: () => COMPONENT_HOST_FIXTURE_EVENT_URI,
    kind: () => WireNostrUriKindValue.Event as WireNostrUriKind,
    primaryId: () => COMPONENT_HOST_FIXTURE_EVENT_ID,
    relays: () => null,
    relaysLength: () => 0,
    author: () => COMPONENT_HOST_FIXTURE_PUBKEY,
    eventKind: () => 1,
  };
  const node = {
    kind: () => WireNodeKind.EventRef,
    text: () => COMPONENT_HOST_FIXTURE_EVENT_URI,
    nostrUri: () => nostrUri,
    childrenLength: () => 0,
  } as unknown as WireNode;
  return {
    rootsLength: () => 1,
    roots: () => 0,
    nodes: (index: number) => (index === 0 ? node : null),
  } as unknown as ContentTreeWire;
}
