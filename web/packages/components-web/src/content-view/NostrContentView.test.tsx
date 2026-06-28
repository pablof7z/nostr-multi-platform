import { renderToString } from "solid-js/web";
import { describe, expect, it } from "vitest";
import { NmpComponentHostProvider } from "../component-host/NmpComponentHostProvider";
import type { EventRefResolver } from "../component-host/EventRefResolver";
import type { EmbeddedEventModel } from "../content-kind-registry/NostrKindRegistry";
import type { ContentTreeWire } from "../generated/nmp/content/content-tree-wire";
import type { WireNode } from "../generated/nmp/content/wire-node";
import { WireNodeKind } from "../generated/nmp/content/wire-node-kind";
import { WireNostrUriKind } from "../generated/nmp/content/wire-nostr-uri-kind";
import type { NostrProfileHost } from "../user-avatar/NostrProfileHost";
import { NostrContentView } from "./NostrContentView";

const TEST_URI = "nostr:nevent1qqstest";
const TEST_PRIMARY_ID = "event-primary-id";
const TEST_AUTHOR = "f".repeat(64);

function eventRefTree(): ContentTreeWire {
  const nostrUri = {
    uri: () => TEST_URI,
    kind: () => WireNostrUriKind.Event,
    primaryId: () => TEST_PRIMARY_ID,
    relays: () => "wss://relay.example",
    relaysLength: () => 1,
    author: () => TEST_AUTHOR,
    eventKind: () => 1,
  };
  const node = {
    kind: () => WireNodeKind.EventRef,
    text: () => TEST_URI,
    nostrUri: () => nostrUri,
  } as unknown as WireNode;
  return {
    rootsLength: () => 1,
    roots: () => 0,
    nodes: (index: number) => (index === 0 ? node : null),
  } as unknown as ContentTreeWire;
}

function shortNoteEmbed(): EmbeddedEventModel {
  return {
    uri: TEST_URI,
    primaryId: TEST_PRIMARY_ID,
    collapsed: false,
    projection: {
      variant: "shortNote",
      data: {
        id: TEST_PRIMARY_ID,
        authorPubkey: TEST_AUTHOR,
        createdAt: 500,
        contentTree: { nodes: [{ kind: "text", text: "resolved note body" }], roots: [0] },
        mediaUrls: [],
      },
    },
  };
}

const profileHost: NostrProfileHost = {
  profile(pubkey) {
    return pubkey === TEST_AUTHOR
      ? { pubkey, displayName: "Alice", pictureUrl: "https://example.test/a.png" }
      : undefined;
  },
  resolveProfileRef() {},
  releaseProfileRef() {},
};

const eventRefResolver: EventRefResolver = {
  resolveEventRef() {},
  releaseEventRef() {},
};

describe("NostrContentView event refs", () => {
  it("renders a raw nostr link when no component host is mounted", () => {
    const html = renderToString(() => (
      <NostrContentView tree={eventRefTree()} nowSeconds={1_000} />
    ));

    expect(html).toContain('class="nostr-event-ref"');
    expect(html).toContain(TEST_URI);
    expect(html).not.toContain("nostr-quote-card");
  });

  it("keeps the raw link fallback when the host has no resolved embed", () => {
    const html = renderToString(() => (
      <NmpComponentHostProvider
        profileHost={profileHost}
        resolvedEventEmbeds={new Map()}
        eventRefResolver={eventRefResolver}
      >
        <NostrContentView tree={eventRefTree()} nowSeconds={1_000} />
      </NmpComponentHostProvider>
    ));

    expect(html).toContain('class="nostr-event-ref"');
    expect(html).toContain(TEST_URI);
    expect(html).not.toContain("nostr-quote-card");
  });

  it("renders a resolved embed card from the host map", () => {
    const html = renderToString(() => (
      <NmpComponentHostProvider
        profileHost={profileHost}
        resolvedEventEmbeds={new Map([[TEST_PRIMARY_ID, shortNoteEmbed()]])}
        eventRefResolver={eventRefResolver}
      >
        <NostrContentView tree={eventRefTree()} nowSeconds={1_000} />
      </NmpComponentHostProvider>
    ));

    expect(html).toContain("nostr-event-ref-embed");
    expect(html).toContain("nostr-quote-card");
    expect(html).toContain("resolved note body");
    expect(html).toContain("Alice");
  });
});
