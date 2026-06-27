import { expect, test } from "@playwright/test";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm search: NIP-50 results render from the Rust typed search sidecar", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

  try {
    await page.goto(
      `/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}&search_relay=${encodeURIComponent(
        relay.url,
      )}`,
    );

    await expect(page.locator(SHELL)).toHaveAttribute("data-runtime-status", "running", {
      timeout: 30_000,
    });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await page.getByTestId("nav-search").click();
    await expect(page.getByTestId("search-panel")).toBeVisible();
    await page.getByRole("tab", { name: "Long-form" }).click();

    await page.getByTestId("search-input").fill(relay.longformContent);
    await page.getByTestId("search-submit").click();

    const results = page.getByTestId("search-results");
    await expect(results.getByTestId("search-result-card").first()).toContainText(
      relay.longformContent,
      { timeout: 60_000 },
    );
    await expect(results).toContainText(relay.url.replace(/^wss?:\/\//, ""));
  } finally {
    await relay.close();
  }
});
