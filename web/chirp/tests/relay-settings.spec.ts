import { test, expect } from "@playwright/test";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

function compactUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "");
}

test("@wasm relay settings: add dials a relay and remove updates runtime inventory", async ({
  page,
}) => {
  const bootstrapRelay = await startFixtureRelay();
  const addedRelay = await startFixtureRelay();
  const relayBootstrap = JSON.stringify([[bootstrapRelay.url, "both,indexer"]]);
  const addedLabel = compactUrl(addedRelay.url);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);
    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });
    await expect
      .poll(() => bootstrapRelay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await page.getByTestId("nav-relays").click();
    await expect(shell).toHaveAttribute("data-main-view", "relays");

    await page.getByTestId("relay-url-input").fill(addedRelay.url);
    await page.getByTestId("relay-role-select").selectOption("both");
    await page.getByTestId("relay-add-button").click();

    await expect
      .poll(() => addedRelay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    const addedRow = page.locator(".configured-relay-row").filter({ hasText: addedLabel });
    await expect(addedRow).toBeVisible({ timeout: 15_000 });
    await addedRow.getByTestId("relay-remove-button").click();
    await expect(addedRow).toHaveCount(0, { timeout: 15_000 });
  } finally {
    await addedRelay.close();
    await bootstrapRelay.close();
  }
});

test("@wasm relay settings: publishes signed NIP-65 preferences to a relay", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const advertisedRelay = "wss://relay.example";
  const viewerSecretKey = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSecretKey);
  const signedEvents: Record<string, unknown>[] = [];

  try {
    await page.exposeFunction("signNostrEvent", async (event: Record<string, unknown>) => {
      const signed = finalizeEvent(
        event as Parameters<typeof finalizeEvent>[0],
        viewerSecretKey,
      ) as unknown as Record<string, unknown>;
      signedEvents.push(signed);
      return signed;
    });
    await page.addInitScript((viewerPubkeyHex: string) => {
      (window as unknown as { nostr: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(viewerPubkeyHex),
        signEvent: (event: Record<string, unknown>) =>
          (
            window as unknown as {
              signNostrEvent(e: Record<string, unknown>): Promise<Record<string, unknown>>;
            }
          ).signNostrEvent(event),
      };
    }, viewerPubkey);

    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);
    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    const connect = page.locator('[data-slot="signing"] [data-action="connect-nip07"]');
    await expect(connect).toBeVisible({ timeout: 10_000 });
    await connect.click();

    await page.getByTestId("nav-relays").click();
    await expect(shell).toHaveAttribute("data-main-view", "relays");

    await page.getByTestId("relay-url-input").fill(advertisedRelay);
    await page.getByTestId("relay-role-select").selectOption("both");
    await page.getByTestId("relay-add-button").click();
    await expect(page.locator(".relay-status")).toContainText("Relay settings updated", {
      timeout: 10_000,
    });

    await page.getByTestId("relay-publish-preferences").click();
    await expect(page.locator(".relay-status")).toContainText("Signer approval requested", {
      timeout: 10_000,
    });

    await expect
      .poll(() => relay.receivedEvents().some((event) => event.kind === 10002), {
        timeout: 30_000,
      })
      .toBe(true);

    expect(signedEvents.length).toBeGreaterThanOrEqual(1);
    const relayList = relay
      .receivedEvents()
      .find((event) => event.kind === 10002 && event.pubkey === viewerPubkey);
    expect(relayList).toBeTruthy();
    expect(relayList?.tags).toEqual(expect.arrayContaining([["r", advertisedRelay]]));
  } finally {
    await relay.close();
  }
});
