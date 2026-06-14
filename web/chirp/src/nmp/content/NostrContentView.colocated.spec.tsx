// @vitest-environment jsdom
//
// Regression test for the #1411 quoted-embed e2e failure (Assertion 6): the
// inline mention chip ("Bob Fixture") never rendered when a note's content tree
// co-located a `nostr:npub…` MENTION and a `nostr:nevent…` EVENT-REF in the
// SAME paragraph (a real-world shape — one note that both @-mentions someone and
// quotes another note). The mention chip rendered fine ALONE (commit 262aa337);
// adding the EventRef sibling broke it.
//
// The whole Rust path (tokenizer + NFCT wire round-trip) was proven correct
// end-to-end, so this isolates the TS render layer: feed `NostrContentView` the
// REAL kernel-generated NFCT bytes (produced by `nmp-content`'s
// `tokenize → to_wire → encode_content_tree` for the exact co-located note) plus
// a profile host that has already resolved the mentioned profile — exactly the
// state the live feed was in when the chip failed to appear.
import { describe, expect, it } from "vitest";
import { createSignal } from "solid-js";
import { render } from "@solidjs/testing-library";
import * as flatbuffers from "flatbuffers";
import { ContentTreeWire } from "../generated/nmp/content/content-tree-wire";
import { NostrContentView } from "./NostrContentView";
import {
  NostrProfileHostProvider,
  type NostrProfileHost,
} from "../../components/user-avatar/NostrProfileHost";
import {
  NostrEventHostProvider,
  type NostrEventHost,
} from "../../components/content-kind-registry/NostrEventHost";
import type { ProfileWire } from "../../components/user-avatar/ProfileWire";
import type { EmbeddedEventModel } from "../../components/content-kind-registry/NostrKindRegistry";
import {
  claimProfileCommand,
  releaseProfileCommand,
  claimEventCommand,
  releaseEventCommand,
  type RuntimeCommand,
} from "../actions";

// Real NFCT bytes for the note `"hello cc nostr:npub1<BOB> quoting Carol →
// nostr:nevent1<CAROL>"`, emitted by the nmp-content encode path. The mention's
// primary_id is the all-`1`s pubkey; the event-ref's is the all-`a`s event id.
const BOB_PK = "1".repeat(64);
const NFCT_HEX =
  "140000004e46435400000a00100008000c0007000a000000000000021c00000004000000040000000000000001000000020000000300000004000000e801000014010000e00000000400000008ffffff000000021400000010001800080007000c0010000000140010000000000000015c00000010000000080000000100000000000000400000006161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100000000530000006e6f7374723a6e6576656e74317171733234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234327372717371717171717078326c6b34720008ffffff04000000130000002071756f74696e67204361726f6c20e28692200010000c00070000000000000000000800100000000000000114000000100014000400000008000c0000001000100000005c0000001000000008000000ffffffff00000000400000003131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313100000000450000006e6f7374723a6e707562317a7967337a7967337a7967337a7967337a7967337a7967337a7967337a7967337a7967337a7967337a7967337a7967736534736c3368000000080008000000040008000000040000000900000068656c6c6f20636320000000";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function decodeTree(): ContentTreeWire {
  const bb = new flatbuffers.ByteBuffer(hexToBytes(NFCT_HEX));
  return ContentTreeWire.getRootAsContentTreeWire(bb);
}

/** A profile host with Bob already resolved (display name "Bob Fixture"),
 *  matching the live KRPR state when the chip failed. */
function resolvedProfileHost(): NostrProfileHost {
  const bob: ProfileWire = { pubkey: BOB_PK, displayName: "Bob Fixture" };
  return {
    profile: (pubkey) => (pubkey === BOB_PK ? bob : undefined),
    claimProfile: () => {},
    releaseProfile: () => {},
  };
}

/** An event host whose quoted event is resolved (the failing run's state — the
 *  fixture relay served Carol's note before the assertion window). */
function resolvedEventHost(): NostrEventHost {
  const carol: EmbeddedEventModel = {
    kind: 1,
    content: "the genuinely quoted note body",
    createdAt: 0,
    tags: [],
    authorName: "Carol Quoted",
  };
  return {
    claimedEvent: () => carol,
    claimEvent: () => {},
    releaseEvent: () => {},
  };
}

describe("NostrContentView co-located mention + event-ref (#1411)", () => {
  it("renders the resolved mention chip even when a sibling EventRef is present", () => {
    const { container } = render(() => (
      <NostrProfileHostProvider host={resolvedProfileHost()}>
        <NostrEventHostProvider host={resolvedEventHost()}>
          <NostrContentView tree={decodeTree()} />
        </NostrEventHostProvider>
      </NostrProfileHostProvider>
    ));
    const chip = container.querySelector(".nostr-mention-chip");
    expect(chip, "mention chip element should exist").not.toBeNull();
    expect(chip?.textContent).toContain("Bob Fixture");
  });

  it("updates the mention chip when the profile resolves AFTER mount (live sequence)", () => {
    // Live ordering: the chip mounts (profile not yet resolved → anchor), the
    // EventRef sibling claims its quoted event, THEN Bob's kind:0 arrives and
    // the profile host flips to resolved. The chip must react to that.
    const [resolved, setResolved] = createSignal(false);
    const bob: ProfileWire = { pubkey: BOB_PK, displayName: "Bob Fixture" };
    const profileHost: NostrProfileHost = {
      profile: (pubkey) => (resolved() && pubkey === BOB_PK ? bob : undefined),
      claimProfile: () => {},
      releaseProfile: () => {},
    };
    const { container } = render(() => (
      <NostrProfileHostProvider host={profileHost}>
        <NostrEventHostProvider host={resolvedEventHost()}>
          <NostrContentView tree={decodeTree()} />
        </NostrEventHostProvider>
      </NostrProfileHostProvider>
    ));
    // Pre-resolution: honest anchor fallback, no chip.
    expect(container.querySelector(".nostr-mention-chip")).toBeNull();
    // kind:0 arrives.
    setResolved(true);
    const chip = container.querySelector(".nostr-mention-chip");
    expect(chip, "mention chip should appear after profile resolves").not.toBeNull();
    expect(chip?.textContent).toContain("Bob Fixture");
  });

  it("keeps the mention chip when the tree prop is REPLACED each frame (live re-decode)", () => {
    // The live feed re-decodes a NEW ContentTreeWire from content_tree_bytes on
    // every snapshot frame, so `props.item.contentTree` is a fresh object each
    // render — unlike the stable object the other tests pass. Profile resolves
    // on a LATER frame (the realistic order). The chip must survive the tree
    // identity churn and still render once the profile is in.
    const [tree, setTree] = createSignal<ContentTreeWire>(decodeTree());
    const [resolved, setResolved] = createSignal(false);
    const bob: ProfileWire = { pubkey: BOB_PK, displayName: "Bob Fixture" };
    const profileHost: NostrProfileHost = {
      profile: (pubkey) => (resolved() && pubkey === BOB_PK ? bob : undefined),
      claimProfile: () => {},
      releaseProfile: () => {},
    };
    const { container } = render(() => (
      <NostrProfileHostProvider host={profileHost}>
        <NostrEventHostProvider host={resolvedEventHost()}>
          <NostrContentView tree={tree()} fallback="raw" />
        </NostrEventHostProvider>
      </NostrProfileHostProvider>
    ));
    // Several re-decode frames arrive before the profile resolves…
    setTree(decodeTree());
    setTree(decodeTree());
    setResolved(true);
    setTree(decodeTree());
    const chip = container.querySelector(".nostr-mention-chip");
    expect(chip, "mention chip should survive tree re-decode churn").not.toBeNull();
    expect(chip?.textContent).toContain("Bob Fixture");
  });

  it("emits BOTH a claim_profile (Bob) and a claim_event (Carol) from the real host wiring", () => {
    // Reproduce App.tsx's exact host wiring (thin one-liners over a shared
    // dispatcher) with a RECORDING dispatcher instead of the worker, then mount
    // the co-located tree. Confirms the component onMount layer fires both
    // claims — exonerating (or implicating) the chirp dispatch-emission layer
    // independently of the worker/kernel.
    const recorded: RuntimeCommand[] = [];
    const dispatchQuiet = (command: RuntimeCommand): void => {
      recorded.push(command);
    };
    const profileHost: NostrProfileHost = {
      profile: () => undefined,
      claimProfile: (pubkey, consumerId) =>
        dispatchQuiet(claimProfileCommand(pubkey, consumerId)),
      releaseProfile: (pubkey, consumerId) =>
        dispatchQuiet(releaseProfileCommand(pubkey, consumerId)),
    };
    const eventHost: NostrEventHost = {
      claimedEvent: () => undefined,
      claimEvent: (uri, consumerId) => dispatchQuiet(claimEventCommand(uri, consumerId)),
      releaseEvent: (uri, consumerId) => dispatchQuiet(releaseEventCommand(uri, consumerId)),
    };
    render(() => (
      <NostrProfileHostProvider host={profileHost}>
        <NostrEventHostProvider host={eventHost}>
          <NostrContentView tree={decodeTree()} />
        </NostrEventHostProvider>
      </NostrProfileHostProvider>
    ));
    const profileClaim = recorded.find(
      (c) => c.actionType === "nmp.kernel.claim_profile",
    );
    const eventClaim = recorded.find((c) => c.actionType === "nmp.kernel.claim_event");
    expect(profileClaim, "mention should emit a claim_profile").toBeDefined();
    expect((profileClaim?.payload as { pubkey?: string })?.pubkey).toBe(BOB_PK);
    expect(eventClaim, "event-ref should emit a claim_event").toBeDefined();
    expect((eventClaim?.payload as { uri?: string })?.uri).toContain("nostr:nevent1");
  });
});
