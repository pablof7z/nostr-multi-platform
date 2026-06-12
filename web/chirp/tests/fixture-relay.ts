/**
 * NIP-01 fixture relay for the Playwright boot smoke (PR-W3).
 *
 * Starts a WebSocket server on a random loopback port.  The relay:
 *   - Accepts any WebSocket connection.
 *   - Handles REQ  → returns EOSE immediately (no seeded events).
 *   - Handles EVENT → acknowledges with OK.
 *   - Handles CLOSE → no response (NIP-01 spec).
 *
 * No external network access.  No state across connections.
 * The boot smoke only needs the relay to *accept* a connection so the
 * wasm runtime's `on_connected` callback fires and the kernel marks the
 * relay as "connected" in the next snapshot.
 */

import { WebSocketServer } from "ws";
import type { WebSocket } from "ws";
import type { AddressInfo } from "net";

export type FixtureRelay = {
  /** WebSocket URL the browser can connect to, e.g. `ws://127.0.0.1:52341`. */
  url: string;
  /**
   * Number of inbound WebSocket connections the relay has accepted so far.
   * The boot smoke asserts this is >= 1 to prove the real wasm opened a
   * relay connection (DegradedRuntime never dials any relays).
   */
  connectionCount(): number;
  /** Gracefully close the server and resolve when all connections are gone. */
  close(): Promise<void>;
};

/**
 * Start the fixture relay and resolve with its URL and a close handle.
 * The server binds on `127.0.0.1` with an OS-assigned port so multiple
 * concurrent test runs (if any) never collide.
 */
export async function startFixtureRelay(): Promise<FixtureRelay> {
  return new Promise((resolve, reject) => {
    const wss = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    let connections = 0;

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
          if (!Array.isArray(msg) || msg.length === 0) {
            return;
          }
          const [verb, ...rest] = msg as [string, ...unknown[]];

          if (verb === "REQ" && typeof rest[0] === "string") {
            // Return EOSE immediately — no stored events in this fixture.
            ws.send(JSON.stringify(["EOSE", rest[0]]));
          } else if (verb === "EVENT") {
            // Acknowledge receipt.  NIP-01 OK message: [OK, id, true, ""].
            const event = rest[0] as Record<string, unknown> | undefined;
            const eventId = typeof event?.id === "string" ? event.id : "";
            ws.send(JSON.stringify(["OK", eventId, true, ""]));
          }
          // CLOSE: no response required per NIP-01.
        });

        ws.on("error", () => {
          // Swallow per-connection errors so one bad client doesn't crash
          // the fixture server.
        });
      });

      resolve({
        url: `ws://127.0.0.1:${port}`,
        connectionCount: () => connections,
        close: () =>
          new Promise<void>((res, rej) => {
            // Terminate all open connections first so wss.close() doesn't
            // block waiting for the wasm relay client to disconnect on its own.
            for (const client of wss.clients) {
              client.terminate();
            }
            wss.close((err) => (err ? rej(err) : res()));
          }),
      });
    });
  });
}
