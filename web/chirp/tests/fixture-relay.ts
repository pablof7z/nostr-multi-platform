/**
 * NIP-01 in-memory fixture relay for the Chirp Web acceptance suite (#2038 item E).
 *
 * Starts a WebSocket server on a random loopback port. The relay speaks the
 * minimal Nostr relay protocol the acceptance specs need:
 *   - Accepts any WebSocket connection.
 *   - REQ   → emits retained events matching the filter, then EOSE.
 *   - EVENT → records the inbound event, retains it for later REQs, then ACKs.
 *   - CLOSE → no response (per NIP-01).
 *
 * The relay runs in the Node.js Playwright process and never touches the
 * public network, so the suite is hermetic and safe to run in CI.
 *
 * Factories:
 *   `startFixtureRelay()`     — boot smoke relay (EOSE only, no seeded events).
 *   `startFeedFixtureRelay()` — feed relay pre-loaded with genuinely signed
 *                               events (viewer kind:3 + two follows' kind:0/1).
 *   `startGroupFixtureRelay()` — group relay pre-loaded with signed NIP-29
 *                                metadata/member/admin events.
 *
 * All seeded events are signed with real secp256k1 keys via nostr-tools. The
 * nmp-core ingest path verifies signatures and rejects forged ones, so these
 * are honest fixtures — never fake-signed payloads.
 *
 * Ported from the pre-rebuild suite (`git show 6da4b6f6f^:web/chirp/tests/
 * fixture-relay.ts`) and re-pointed at the Item B shell's `?relay_bootstrap=`
 * contract; the relay protocol itself is unchanged.
 */

import type { WebSocket } from "ws";
import type { AddressInfo } from "node:net";
import { createServer } from "node:http";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFixtureWebSocketServer } from "./fixture-relay-server.js";

const FIXTURE_IMAGE = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 400" role="img" aria-label="Chirp fixture media">
  <rect width="640" height="400" fill="#dff4ec"/>
  <circle cx="118" cy="112" r="54" fill="#217a63"/>
  <path d="M210 122h288M210 186h220M210 250h286" stroke="#17202a" stroke-width="24" stroke-linecap="round"/>
  <rect x="64" y="296" width="512" height="36" rx="18" fill="#ffffff" opacity=".82"/>
  <path d="M104 314h188" stroke="#217a63" stroke-width="12" stroke-linecap="round"/>
</svg>`;

/** Start a throwaway HTTP server that serves a real network image at any path. */
function startImageServer(): Promise<{ url: string; close: () => Promise<void> }> {
  return new Promise((resolve, reject) => {
    const server = createServer((_req, res) => {
      res.writeHead(200, {
        "content-type": "image/svg+xml",
        "access-control-allow-origin": "*",
      });
      res.end(FIXTURE_IMAGE);
    });
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as AddressInfo;
      resolve({
        url: `http://127.0.0.1:${port}/avatar.png`,
        close: () => new Promise<void>((res) => server.close(() => res())),
      });
    });
  });
}

export type NostrEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

export type FixtureRelay = {
  /** WebSocket URL the browser connects to, e.g. `ws://127.0.0.1:52341`. */
  url: string;
  /**
   * Number of inbound WebSocket connections accepted so far. The boot smoke
   * asserts this is ≥ 1 to prove the real wasm relay pool dialled the relay —
   * the DegradedRuntime never opens any relay connection.
   */
  connectionCount(): number;
  /** Number of EVENT frames received from browser clients. */
  eventCount(): number;
  /** Snapshot of EVENT payloads received from browser clients. */
  receivedEvents(): NostrEvent[];
  /** Snapshot of retained EVENT payloads sent from this fixture relay to clients. */
  deliveredEvents(): NostrEvent[];
  /** Snapshot of REQ filters received from browser clients. */
  subscriptions(): NostrFilter[];
  /** Gracefully close the server and resolve once all connections are gone. */
  close(): Promise<void>;
};

export type FeedFixtureRelay = FixtureRelay & {
  /** Secret key for the test viewer; specs may encode it as nsec for local-key onboarding. */
  viewerSecretKey: Uint8Array;
  /** Hex pubkey of the test viewer (use this for the window.nostr mock). */
  viewerPubkey: string;
  /** Hex pubkey of the follow whose kind:1 note appears in the feed. */
  followPubkey: string;
  /** Hex pubkey of the second follow, used to prove kind:3 edits preserve siblings. */
  secondFollowPubkey: string;
  /** Stable phrase inside the follow's note (assert against the rendered feed). */
  noteContent: string;
  /** Normal URL embedded in the follow's note. */
  noteLinkUrl: string;
  /** Image URL embedded in the follow's note. */
  noteImageUrl: string;
  /** Display name resolved from the follow's kind:0. */
  followDisplayName: string;
  /** Picture URL (http) resolved from the follow's kind:0. */
  followPictureUrl: string;
  /** Display name of the second follow who replies (attribution badge). */
  replierDisplayName: string;
  /** Content of a long-form event used by search specs outside the home feed. */
  longformContent: string;
};

export type GroupFixtureRelay = FixtureRelay & {
  groupId: string;
  groupName: string;
  groupAbout: string;
  groupPictureUrl: string;
  groupMessageContent: string;
  memberCount: number;
  adminCount: number;
};

export type FixtureRelayOptions = {
  eventAck?: {
    ok?: boolean;
    message?: string;
    delayMs?: number;
  };
  secure?: boolean;
};

export type NostrFilter = {
  kinds?: number[];
  authors?: string[];
  ids?: string[];
  since?: number;
  until?: number;
  limit?: number;
  [key: string]: unknown;
};

function matchesFilter(event: NostrEvent, filter: NostrFilter): boolean {
  if (filter.kinds !== undefined && !filter.kinds.includes(event.kind)) return false;
  if (filter.authors !== undefined && !filter.authors.includes(event.pubkey)) return false;
  if (filter.ids !== undefined && !filter.ids.includes(event.id)) return false;
  if (filter.since !== undefined && event.created_at < filter.since) return false;
  if (filter.until !== undefined && event.created_at > filter.until) return false;
  for (const [key, value] of Object.entries(filter)) {
    if (!key.startsWith("#")) continue;
    if (!Array.isArray(value)) continue;
    const tagName = key.slice(1);
    const accepted = value.filter((item): item is string => typeof item === "string");
    if (accepted.length === 0) continue;
    const matched = event.tags.some(
      (tag) => tag[0] === tagName && typeof tag[1] === "string" && accepted.includes(tag[1]),
    );
    if (!matched) return false;
  }
  if (typeof filter.search === "string" && !matchesSearch(event, filter.search)) return false;
  return true;
}

function matchesSearch(event: NostrEvent, search: string): boolean {
  const terms = search
    .toLowerCase()
    .split(/\s+/)
    .map((term) => term.trim())
    .filter(Boolean);
  if (terms.length === 0) return true;
  const haystack = event.content.toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function startFixtureRelayServer(
  seededEvents: NostrEvent[],
  options: FixtureRelayOptions = {},
): Promise<FixtureRelay> {
  return startFixtureWebSocketServer(options.secure ?? false).then(({ wss, url, close }) => {
    let connections = 0;
    const receivedEvents: NostrEvent[] = [];
    const deliveredEvents: NostrEvent[] = [];
    const subscriptions: NostrFilter[] = [];

    wss.on("connection", (ws: WebSocket) => {
      connections += 1;

      ws.on("message", (raw: Buffer | string) => {
        let msg: unknown;
        try {
          msg = JSON.parse(typeof raw === "string" ? raw : raw.toString());
        } catch {
          return;
        }
        if (!Array.isArray(msg) || msg.length === 0) return;
        const [verb, ...rest] = msg as [string, ...unknown[]];

        if (verb === "REQ" && typeof rest[0] === "string") {
          const subId = rest[0];
          const filters = (rest.slice(1) as NostrFilter[]).filter(
            (f) => typeof f === "object" && f !== null,
          );
          subscriptions.push(...filters);
          const sendSoon = (frame: string) => setTimeout(() => ws.send(frame), 0);
          const sendEventSoon = (event: NostrEvent) =>
            setTimeout(() => {
              deliveredEvents.push(event);
              ws.send(JSON.stringify(["EVENT", subId, event]));
            }, 0);
          const retainedEvents = [...seededEvents, ...receivedEvents];
          for (const event of retainedEvents) {
            const matched = filters.length === 0 || filters.some((f) => matchesFilter(event, f));
            if (matched) sendEventSoon(event);
          }
          sendSoon(JSON.stringify(["EOSE", subId]));
        } else if (verb === "EVENT") {
          const event = rest[0] as Record<string, unknown> | undefined;
          const eventId = typeof event?.id === "string" ? event.id : "";
          if (event !== undefined) {
            receivedEvents.push(event as NostrEvent);
          }
          const ack = options.eventAck ?? {};
          const ok = ack.ok ?? true;
          const message = ack.message ?? "";
          setTimeout(() => {
            if (ws.readyState === 1) ws.send(JSON.stringify(["OK", eventId, ok, message]));
          }, ack.delayMs ?? 0);
        }
        // CLOSE: no response required per NIP-01.
      });

      ws.on("error", () => {});
    });

    return {
      url,
      connectionCount: () => connections,
      eventCount: () => receivedEvents.length,
      receivedEvents: () => [...receivedEvents],
      deliveredEvents: () => [...deliveredEvents],
      subscriptions: () => [...subscriptions],
      close,
    };
  });
}

/**
 * Boot smoke relay — no seeded events. Accepts connections and EOSEs every
 * subscription, proving the real wasm relay pool dialled out. Also used by the
 * publish spec, where the only events the relay sees are the browser's own
 * outbound EVENT frames.
 */
export async function startFixtureRelay(options: FixtureRelayOptions = {}): Promise<FixtureRelay> {
  return startFixtureRelayServer([], options);
}

/**
 * Feed fixture relay — pre-loaded with genuinely signed Nostr events:
 *
 *   viewer  → kind:3 contact list (follows = [followA, followB])
 *   followA → kind:0 profile (display name "Alice Fixture", http picture)
 *   followB → kind:0 profile (display name "Bob Fixture")
 *   followA → kind:1 root note ("hello from fixture relay")
 *   followB → kind:1 reply to followA's note (NIP-10 e/p tags)
 *
 * Returns the relay plus the viewer keypair so the spec can mock window.nostr
 * with a real pubkey.
 */
export async function startFeedFixtureRelay(): Promise<FeedFixtureRelay> {
  const viewerSk = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSk);

  const followASk = generateSecretKey();
  const followAPubkey = getPublicKey(followASk);

  const followBSk = generateSecretKey();
  const followBPubkey = getPublicKey(followBSk);

  const now = Math.floor(Date.now() / 1000);
  const noteText = "hello from fixture relay";
  const longformContent = "longform fixture article about chirp search";
  const followADisplayName = "Alice Fixture";
  const followBDisplayName = "Bob Fixture";

  // Serve the picture over real HTTP: nmp-core keeps only http(s) picture URLs
  // (a data: URI would be filtered), so this is a genuine network image the
  // avatar can fetch + decode with no external dependency.
  const imageServer = await startImageServer();
  const followAPictureUrl = imageServer.url;
  const noteContent = noteText;
  const noteLinkUrl = "https://example.com/chirp";
  const noteImageUrl = imageServer.url;
  const richNoteContent = `${noteText} #nostr ${noteLinkUrl} ${noteImageUrl}`;

  const profileA = finalizeEvent(
    {
      kind: 0,
      created_at: now - 100,
      tags: [],
      content: JSON.stringify({
        display_name: followADisplayName,
        name: "Alice Fallback",
        picture: followAPictureUrl,
      }),
    },
    followASk,
  ) as NostrEvent;

  const profileB = finalizeEvent(
    {
      kind: 0,
      created_at: now - 100,
      tags: [],
      content: JSON.stringify({
        display_name: followBDisplayName,
        name: "Bob Fallback",
      }),
    },
    followBSk,
  ) as NostrEvent;

  const noteA = finalizeEvent(
    {
      kind: 1,
      created_at: now - 50,
      tags: [["p", followBPubkey]],
      content: richNoteContent,
    },
    followASk,
  ) as NostrEvent;

  // NIP-10 reply: p-tag both the root author (Alice) AND the replier (Bob) so
  // the kernel's discovery path resolves Bob's kind:0 for the attribution badge.
  const noteB = finalizeEvent(
    {
      kind: 1,
      created_at: now - 10,
      tags: [
        ["e", noteA.id, "", "reply"],
        ["p", followAPubkey],
        ["p", followBPubkey],
      ],
      content: "great note!",
    },
    followBSk,
  ) as NostrEvent;

  const contactList = finalizeEvent(
    {
      kind: 3,
      created_at: now,
      tags: [
        ["p", followAPubkey],
        ["p", followBPubkey],
      ],
      content: "",
    },
    viewerSk,
  ) as NostrEvent;

  const longform = finalizeEvent(
    {
      kind: 30023,
      created_at: now - 5,
      tags: [["title", "Chirp search fixture"]],
      content: longformContent,
    },
    followASk,
  ) as NostrEvent;

  const seeded: NostrEvent[] = [contactList, profileA, profileB, noteA, noteB, longform];

  const base = await startFixtureRelayServer(seeded);
  return {
    ...base,
    close: async () => {
      await base.close();
      await imageServer.close();
    },
    viewerSecretKey: viewerSk,
    viewerPubkey,
    followPubkey: followAPubkey,
    secondFollowPubkey: followBPubkey,
    noteContent,
    noteLinkUrl,
    noteImageUrl,
    longformContent,
    followDisplayName: followADisplayName,
    followPictureUrl: followAPictureUrl,
    replierDisplayName: followBDisplayName,
  };
}

export async function startGroupFixtureRelay(): Promise<GroupFixtureRelay> {
  const groupSk = generateSecretKey();
  const memberASk = generateSecretKey();
  const memberBSk = generateSecretKey();
  const adminSk = generateSecretKey();
  const memberAPubkey = getPublicKey(memberASk);
  const memberBPubkey = getPublicKey(memberBSk);
  const adminPubkey = getPublicKey(adminSk);
  const now = Math.floor(Date.now() / 1000);
  const imageServer = await startImageServer();
  const groupId = "nmp-builders";
  const groupName = "NMP Builders";
  const groupAbout = "A public room for Rust-owned Nostr clients on every platform.";
  const groupMessageContent = "Rust-owned group timeline from the fixture relay";

  const metadata = finalizeEvent(
    {
      kind: 39000,
      created_at: now - 20,
      tags: [
        ["d", groupId],
        ["name", groupName],
        ["about", groupAbout],
        ["picture", imageServer.url],
        ["public"],
        ["open"],
      ],
      content: "",
    },
    groupSk,
  ) as NostrEvent;

  const admins = finalizeEvent(
    {
      kind: 39001,
      created_at: now - 10,
      tags: [
        ["d", groupId],
        ["p", adminPubkey],
      ],
      content: "",
    },
    groupSk,
  ) as NostrEvent;

  const members = finalizeEvent(
    {
      kind: 39002,
      created_at: now - 5,
      tags: [
        ["d", groupId],
        ["p", memberAPubkey],
        ["p", memberBPubkey],
      ],
      content: "",
    },
    groupSk,
  ) as NostrEvent;

  const message = finalizeEvent(
    {
      kind: 9,
      created_at: now,
      tags: [["h", groupId]],
      content: groupMessageContent,
    },
    memberASk,
  ) as NostrEvent;

  const base = await startFixtureRelayServer([metadata, admins, members, message]);
  return {
    ...base,
    close: async () => {
      await base.close();
      await imageServer.close();
    },
    groupId,
    groupName,
    groupAbout,
    groupPictureUrl: imageServer.url,
    groupMessageContent,
    memberCount: 2,
    adminCount: 1,
  };
}
