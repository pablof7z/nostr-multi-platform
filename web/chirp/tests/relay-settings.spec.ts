import { test, expect } from "@playwright/test";
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
