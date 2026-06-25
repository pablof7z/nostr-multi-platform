/**
 * NIP-01 fixture relay for the Playwright boot smoke (PR-W3) and feed e2e (PR-F3).
 *
 * Starts a WebSocket server on a random loopback port.  The relay:
 *   - Accepts any WebSocket connection.
 *   - Handles REQ  → filters seeded events by kind+authors, then EOSE.
 *   - Handles EVENT → acknowledges with OK.
 *   - Handles CLOSE → no response (NIP-01 spec).
 *
 * `startFixtureRelay()` — boot smoke relay (EOSE only, no seeded events).
 * `startFeedFixtureRelay()` — feed relay pre-loaded with genuinely signed
 *   events from a test keypair (viewer + follows).  Returns the relay plus
 *   the viewer keypair so the test can mock window.nostr.
 *
 * No external network access.  No state across connections.
 */

import { WebSocketServer } from "ws";
import type { WebSocket } from "ws";
import type { AddressInfo } from "net";
import { createServer } from "node:http";

// A real 1×1 PNG served over HTTP so the avatar component genuinely fetches +
// decodes a network image (naturalWidth > 0), with no external dependency.
const ONE_BY_ONE_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  "base64",
);

/** Start a throwaway HTTP server that serves the 1×1 PNG at any path. */
function startPngServer(): Promise<{ url: string; close: () => Promise<void> }> {
  return new Promise((resolve, reject) => {
    const server = createServer((_req, res) => {
      res.writeHead(200, { "content-type": "image/png", "access-control-allow-origin": "*" });
      res.end(ONE_BY_ONE_PNG);
    });
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as AddressInfo;
      resolve({
        url: `http://127.0.0.1:${port}/avatar.png`,
        close: () =>
          new Promise<void>((res) => server.close(() => res())),
      });
    });
  });
}
import { generateSecretKey, getPublicKey, finalizeEvent } from "nostr-tools/pure";
import { npubEncode } from "nostr-tools/nip19";

export type FixtureRelay = {
  /** WebSocket URL the browser can connect to, e.g. `ws://127.0.0.1:52341`. */
  url: string;
  /**
   * Number of inbound WebSocket connections the relay has accepted so far.
   * The boot smoke asserts this is >= 1 to prove the real wasm opened a
   * relay connection (DegradedRuntime never dials any relays).
   */
  connectionCount(): number;
  /** Number of EVENT frames received from browser clients. */
  eventCount(): number;
  /** Snapshot of EVENT payloads received from browser clients. */
  receivedEvents(): NostrEvent[];
  /** Gracefully close the server and resolve when all connections are gone. */
  close(): Promise<void>;
};

export type FeedFixtureRelay = FixtureRelay & {
  /** Hex pubkey of the test viewer (use this for the window.nostr mock). */
  viewerPubkey: string;
  /** Hex pubkey of the follow whose kind:1 notes appear in the feed. */
  followPubkey: string;
  /** Content of the first follow's note (assert this in the e2e). */
  noteContent: string;
  /** Display name resolved from the follow's kind:0 (assert in e2e). */
  followDisplayName: string;
  /** Picture URL (data: URI) resolved from the follow's kind:0 (assert in e2e). */
  followPictureUrl: string;
  /** Display name of the second follow who replies (attribution badge). */
  replierDisplayName: string;
};

// ── Internal types ────────────────────────────────────────────────────────────

type NostrEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

type NostrFilter = {
  kinds?: number[];
  authors?: string[];
  ids?: string[];
  since?: number;
  until?: number;
  limit?: number;
  [key: string]: unknown;
};

// ── Filter matching ───────────────────────────────────────────────────────────

function matchesFilter(event: NostrEvent, filter: NostrFilter): boolean {
  if (filter.kinds !== undefined && !filter.kinds.includes(event.kind)) return false;
  if (filter.authors !== undefined && !filter.authors.includes(event.pubkey)) return false;
  if (filter.ids !== undefined && !filter.ids.includes(event.id)) return false;
  if (filter.since !== undefined && event.created_at < filter.since) return false;
  if (filter.until !== undefined && event.created_at > filter.until) return false;
  return true;
}

// ── Core server factory ───────────────────────────────────────────────────────

function startServer(seededEvents: NostrEvent[]): Promise<FixtureRelay> {
  return new Promise((resolve, reject) => {
    const wss = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    let connections = 0;
    const receivedEvents: NostrEvent[] = [];

    wss.once("error", reject);

    wss.once("listening", () => {
      const { port } = wss.address() as AddressInfo;

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
            // Collect filters (rest[1..n]).
            const filters = (rest.slice(1) as NostrFilter[]).filter(
              (f) => typeof f === "object" && f !== null,
            );
            console.log(`[fixture-relay] REQ ${subId} filters:`, JSON.stringify(filters));
            // Send matching seeded events, then EOSE.
            for (const event of seededEvents) {
              const matched = filters.length === 0
                || filters.some((f) => matchesFilter(event, f));
              if (matched) {
                console.log(`[fixture-relay] → EVENT ${subId} kind:${event.kind} from ${event.pubkey.slice(0, 8)}`);
                ws.send(JSON.stringify(["EVENT", subId, event]));
              }
            }
            ws.send(JSON.stringify(["EOSE", subId]));
          } else if (verb === "EVENT") {
            const event = rest[0] as Record<string, unknown> | undefined;
            const eventId = typeof event?.id === "string" ? event.id : "";
            if (event !== undefined) {
              receivedEvents.push(event as NostrEvent);
            }
            ws.send(JSON.stringify(["OK", eventId, true, ""]));
          }
          // CLOSE: no response required per NIP-01.
        });

        ws.on("error", () => {});
      });

      const close = (): Promise<void> =>
        new Promise<void>((res, rej) => {
          for (const client of wss.clients) client.terminate();
          wss.close((err) => (err ? rej(err) : res()));
        });

      resolve({
        url: `ws://127.0.0.1:${port}`,
        connectionCount: () => connections,
        eventCount: () => receivedEvents.length,
        receivedEvents: () => [...receivedEvents],
        close,
      });
    });
  });
}

// ── Public API: boot smoke relay (no seeded events) ──────────────────────────

/**
 * Start the fixture relay and resolve with its URL and a close handle.
 * Used by the boot smoke (PR-W3): no seeded events — the relay just accepts
 * connections and EOSEs every subscription, proving the wasm relay pool fires.
 */
export async function startFixtureRelay(): Promise<FixtureRelay> {
  return startServer([]);
}

// ── Public API: feed fixture relay (signed events) ───────────────────────────

/**
 * Start a fixture relay pre-loaded with genuinely signed Nostr events:
 *
 *   viewer  → kind:3 contact list (follows = [followA, followB])
 *   followA → kind:1 note ("hello from fixture relay")
 *   followB → kind:1 reply to followA's note (NIP-10 e-tag)
 *   followA → kind:0 profile (name: "Alice Fixture")
 *   followB → kind:0 profile (name: "Bob Fixture")
 *
 * All events are signed with real secp256k1 keys via nostr-tools.  The
 * ingest path in nmp-core verifies signatures and will reject unsigned or
 * fake-signature events.
 *
 * Returns the relay plus the viewer keypair so the Playwright test can mock
 * window.nostr without any fake pubkeys.
 */
export async function startFeedFixtureRelay(): Promise<FeedFixtureRelay> {
  const viewerSk = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSk);

  const followASk = generateSecretKey();
  const followAPubkey = getPublicKey(followASk);

  const followBSk = generateSecretKey();
  const followBPubkey = getPublicKey(followBSk);

  const now = Math.floor(Date.now() / 1000);
  const noteContent = "hello from fixture relay";
  const followADisplayName = "Alice Fixture";
  const followBDisplayName = "Bob Fixture";
  const followAFallbackName = "Alice Fallback";
  const followBFallbackName = "Bob Fallback";
  // A real, self-contained 1×1 PNG so the avatar component genuinely loads an
  // image (naturalWidth > 0) without a network dependency — proving NostrAvatar
  // renders the resolved kind:0 `picture` in Chirp's feed card. (The deployed
  // gallery additionally proves the real-network picture path.)
  // Serve the picture over real HTTP from a local server: the kernel keeps
  // only `http(s)` picture URLs (nmp-core/kernel/nostr.rs filters non-http
  // schemes), so a data: URI would be dropped. This is a genuine network image
  // the avatar fetches + decodes (naturalWidth > 0) with no external dependency.
  const imageServer = await startPngServer();
  const followAPictureUrl = imageServer.url;

  // kind:0 — Alice's profile (must arrive before or with kind:1 so display resolves)
  const profileA = finalizeEvent(
    {
      kind: 0,
      created_at: now - 100,
      tags: [],
      content: JSON.stringify({
        display_name: followADisplayName,
        name: followAFallbackName,
        picture: followAPictureUrl,
      }),
    },
    followASk,
  ) as NostrEvent;

  // kind:0 — Bob's profile
  const profileB = finalizeEvent(
    {
      kind: 0,
      created_at: now - 100,
      tags: [],
      content: JSON.stringify({
        display_name: followBDisplayName,
        name: followBFallbackName,
      }),
    },
    followBSk,
  ) as NostrEvent;

  // kind:1 — Alice's root note (the one that will appear in the feed). It
  // mentions Bob via a `nostr:npub1…` URI + p-tag, so the kernel tokenizes a
  // Mention node into the content tree and Chirp renders it as a resolved
  // NostrMentionChip (avatar + "Bob Fixture") rather than a raw npub anchor.
  const npubBob = npubEncode(followBPubkey);
  const noteAMention = ` cc nostr:${npubBob}`;
  const noteA = finalizeEvent(
    {
      kind: 1,
      created_at: now - 50,
      tags: [["p", followBPubkey]],
      content: noteContent + noteAMention,
    },
    followASk,
  ) as NostrEvent;

  // kind:1 — Bob's reply to Alice's note (produces the attribution badge).
  // NIP-10 convention: p-tag both the root author (Alice) AND the replier
  // (Bob himself) so that `collect_unknown_refs` adds Bob's pubkey to
  // `unknown_ids` when the kernel ingests this event.  Without the self-tag
  // Bob's pubkey never enters `unknown_ids` (kind:3 p-tags are not processed
  // by `collect_unknown_refs`, which only runs on kind:1/6 timeline events),
  // so the discovery oneshot that fetches kind:0 profiles would never include
  // Bob — leaving his display name in the attribution badge unresolved.
  // Tracked as a potential runtime kernel gap in
  // https://github.com/pablof7z/nostr-multi-platform/issues/1257.
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

  // kind:3 — viewer's contact list (follows = Alice + Bob)
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

  const seeded: NostrEvent[] = [contactList, profileA, profileB, noteA, noteB];

  const base = await startServer(seeded);
  return {
    ...base,
    close: async () => {
      await base.close();
      await imageServer.close();
    },
    viewerPubkey,
    followPubkey: followAPubkey,
    noteContent,
    followDisplayName: followADisplayName,
    followPictureUrl: followAPictureUrl,
    replierDisplayName: followBDisplayName,
  };
}
