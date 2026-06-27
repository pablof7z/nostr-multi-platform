import { expect, test } from "@playwright/test";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm blocked workspaces are explicit and diagnostics-backed", async ({ page }) => {
  test.setTimeout(90_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}#workspaces`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });

    const panel = page.getByTestId("workspaces-panel");
    await expect(panel).toBeVisible();
    await expect(page.getByRole("heading", { name: "More Chirp workspaces" })).toBeVisible();
    await expect(page.getByTestId("workspace-notifications")).toContainText("blocked");
    await expect(page.getByTestId("workspace-messages")).toContainText("NIP-17");
    await expect(page.getByTestId("workspace-groups")).toContainText("Group discovery");
    await expect(page.getByTestId("workspace-wallet")).toContainText("Wallet connection");
    await expect(page.getByTestId("workspace-moderation")).toContainText("WoT");
    await expect(page.getByTestId("workspace-offline")).toContainText("partial");

    await page.getByTestId("inspect-messages").click();
    await expect(page.getByTestId("workspace-diagnostic")).toContainText("nmp.nip17.inbox");

    await page.locator("#diagnostics").scrollIntoViewIfNeeded();
    await expect(page.locator(".outbox-state")).toContainText("Runtime rejected action");
    await expect(page.locator(".outbox-state")).toContainText("unsupported_in_chirp_web");
  } finally {
    await relay.close();
  }
});
