import { expect, test } from "@playwright/test";
import { startFixtureRelay } from "./fixture-relay.js";

const VIEWER_PUBKEY =
  "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

test("publish sends signed EVENT to fixture relay", async ({ page }) => {
  test.setTimeout(90_000);
  const relay = await startFixtureRelay();
  const content = `playwright publish ${Date.now()}`;

  try {
    await page.addInitScript((viewerPubkeyHex: string) => {
      (window as Window & { nostr?: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(viewerPubkeyHex),
        signEvent: (event: Record<string, unknown>) =>
          Promise.resolve({
            ...event,
            pubkey: viewerPubkeyHex,
            id: Array.from(crypto.getRandomValues(new Uint8Array(32)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
            sig: Array.from(crypto.getRandomValues(new Uint8Array(64)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
          }),
      };
    }, VIEWER_PUBKEY);

    await page.goto(`/?relay=${encodeURIComponent(relay.url)}`);
    await expect(page.locator('[data-testid="nmp-runtime-status"]')).toHaveText("running", {
      timeout: 30_000,
    });
    await expect.poll(() => relay.connectionCount(), { timeout: 20_000 }).toBeGreaterThanOrEqual(1);

    await page.locator('[data-testid="connect-btn"]').click();
    await page.locator('textarea[aria-label="Compose chirp"]').fill(content);
    await page.getByRole("button", { name: /publish/i }).click();

    await expect
      .poll(() => relay.eventCount(), { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1);

    expect(relay.receivedEvents()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          kind: 1,
          pubkey: VIEWER_PUBKEY,
          content,
        }),
      ]),
    );
  } finally {
    await relay.close();
  }
});
