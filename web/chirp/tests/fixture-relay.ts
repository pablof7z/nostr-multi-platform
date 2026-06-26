/**
 * NIP-01 in-memory fixture relay for the Chirp Web acceptance suite (#2038 item E).
 *
 * Starts a WebSocket server on a random loopback port. The relay speaks the
 * minimal Nostr relay protocol the acceptance specs need:
 *   - Accepts any WebSocket connection.
 *   - REQ   → emits seeded events matching the filter, then EOSE.
 *   - EVENT → records the inbound event, acknowledges with OK.
 *   - CLOSE → no response (per NIP-01).
 *
 * The relay runs in the Node.js Playwright process and never touches the
 * public network, so the suite is hermetic and safe to run in CI.
 *
 * Factories:
 *   `startFixtureRelay()`     — boot smoke relay (EOSE only, no seeded events).
 *   `startFeedFixtureRelay()` — feed relay pre-loaded with genuinely signed
 *                               events (viewer kind:3 + two follows' kind:0/1).
 *
 * All seeded events are signed with real secp256k1 keys via nostr-tools. The
 * nmp-core ingest path verifies signatures and rejects forged ones, so these
 * are honest fixtures — never fake-signed payloads.
 *
 * Ported from the pre-rebuild suite (`git show 6da4b6f6f^:web/chirp/tests/
 * fixture-relay.ts`) and re-pointed at the Item B shell's `?relay_bootstrap=`
 * contract; the relay protocol itself is unchanged.
 */

import { WebSocketServer } from "ws";
import type { WebSocket } from "ws";
import type { AddressInfo } from "net";
import { createServer } from "node:http";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

// A real 1×1 PNG served over HTTP so any avatar component genuinely fetches +
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
  /** Gracefully close the server and resolve once all connections are gone. */
  close(): Promise<void>;
};

export type FeedFixtureRelay = FixtureRelay & {
  /** Hex pubkey of the test viewer (use this for the window.nostr mock). */
  viewerPubkey: string;
  /** Hex pubkey of the follow whose kind:1 note appears in the feed. */
  followPubkey: string;
  /** Content of the follow's note (assert against the rendered feed). */
  noteContent: string;
  /** Display name resolved from the follow's kind:0. */
  followDisplayName: string;
  /** Picture URL (http) resolved from the follow's kind:0. */
  followPictureUrl: string;
  /** Display name of the second follow who replies (attribution badge). */
  replierDisplayName: string;
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

function matchesFilter(event: NostrEvent, filter: NostrFilter): boolean {
  if (filter.kinds !== undefined && !filter.kinds.includes(event.kind)) return false;
  if (filter.authors !== undefined && !filter.authors.includes(event.pubkey)) return false;
  if (filter.ids !== undefined && !filter.ids.includes(event.id)) return false;
  if (filter.since !== undefined && event.created_at < filter.since) return false;
  if (filter.until !== undefined && event.created_at > filter.until) return false;
  return true;
}

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
            const filters = (rest.slice(1) as NostrFilter[]).filter(
              (f) => typeof f === "object" && f !== null,
            );
            const sendSoon = (frame: string) => setTimeout(() => ws.send(frame), 0);
            for (const event of seededEvents) {
              const matched =
                filters.length === 0 || filters.some((f) => matchesFilter(event, f));
              if (matched) {
                sendSoon(JSON.stringify(["EVENT", subId, event]));
              }
            }
            sendSoon(JSON.stringify(["EOSE", subId]));
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

/**
 * Boot smoke relay — no seeded events. Accepts connections and EOSEs every
 * subscription, proving the real wasm relay pool dialled out. Also used by the
 * publish spec, where the only events the relay sees are the browser's own
 * outbound EVENT frames.
 */
export async function startFixtureRelay(): Promise<FixtureRelay> {
  return startServer([]);
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
  const noteContent = "hello from fixture relay";
  const followADisplayName = "Alice Fixture";
  const followBDisplayName = "Bob Fixture";

  // Serve the picture over real HTTP: nmp-core keeps only http(s) picture URLs
  // (a data: URI would be filtered), so this is a genuine network image the
  // avatar can fetch + decode with no external dependency.
  const imageServer = await startPngServer();
  const followAPictureUrl = imageServer.url;

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
      content: noteContent,
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
