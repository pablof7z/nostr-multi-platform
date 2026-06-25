import { expect, test } from "@playwright/test";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFixtureRelay } from "./fixture-relay.js";

test("publish uses shared outbox routing and skips pure indexer relays", async ({ page }) => {
  test.setTimeout(90_000);
  const indexerRelay = await startFixtureRelay();
  const writeRelay = await startFixtureRelay();
  const viewerSecretKey = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSecretKey);
  const content = `playwright publish ${Date.now()}`;
  const signedEvents: unknown[] = [];

  try {
    await page.exposeFunction("signNostrEvent", async (event: Record<string, unknown>) => {
      const signed = finalizeEvent(event, viewerSecretKey);
      signedEvents.push(signed);
      return signed;
    });
    await page.addInitScript((viewerPubkeyHex: string) => {
      (window as Window & { nostr?: unknown; signNostrEvent?: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(viewerPubkeyHex),
        signEvent: (event: Record<string, unknown>) =>
          (window as Window & {
            signNostrEvent(event: Record<string, unknown>): Promise<Record<string, unknown>>;
          }).signNostrEvent(event),
      };
    }, viewerPubkey);

    const relayBootstrap = JSON.stringify([
      [indexerRelay.url, "indexer"],
      [writeRelay.url, "both,indexer"],
    ]);
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);
    await expect(page.locator('[data-testid="nmp-runtime-status"]')).toHaveText("running", {
      timeout: 30_000,
    });
    await expect
      .poll(() => indexerRelay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);
    await expect
      .poll(() => writeRelay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await page.locator('[data-testid="connect-btn"]').click();
    await page.locator('textarea[aria-label="Compose chirp"]').fill(content);
    await page.getByRole("button", { name: /publish/i }).click();

    await expect
      .poll(() => writeRelay.eventCount(), { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1);
    await expect.poll(() => indexerRelay.eventCount(), { timeout: 5_000 }).toBe(0);
    expect(signedEvents).toHaveLength(1);

    expect(writeRelay.receivedEvents()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          kind: 1,
          pubkey: viewerPubkey,
          content,
        }),
      ]),
    );
  } finally {
    await Promise.all([indexerRelay.close(), writeRelay.close()]);
  }
});
