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

test("@wasm search: feed hashtag opens NIP-50 note discovery", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

  try {
    await page.goto(
      `/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}&search_relay=${encodeURIComponent(
        relay.url,
      )}`,
    );

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await page.getByRole("link", { name: "Home" }).click();

    const richCard = page
      .getByTestId("post-card")
      .filter({ hasText: relay.noteContent })
      .first();
    await expect(richCard).toBeVisible({ timeout: 60_000 });
    await richCard.getByTestId("post-content-hashtag").click();

    await expect(shell).toHaveAttribute("data-main-view", "search");
    await expect(page.getByTestId("search-input")).toHaveValue("#nostr");
    await expect(page.getByRole("heading", { name: "Search relays and cache" })).toBeVisible();
    await expect(page.getByTestId("search-results")).toContainText(relay.noteContent, {
      timeout: 60_000,
    });
    await expect(page.getByTestId("search-results")).toContainText(
      relay.url.replace(/^wss?:\/\//, ""),
    );
  } finally {
    await relay.close();
  }
});
