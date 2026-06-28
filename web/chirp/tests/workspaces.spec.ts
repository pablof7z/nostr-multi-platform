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
    await expect(page.getByTestId("nav-notifications")).toBeVisible();
    await expect(page.getByTestId("nav-messages")).toBeVisible();
    await expect(page.getByTestId("workspace-notifications")).toHaveCount(0);
    await expect(page.getByTestId("workspace-messages")).toHaveCount(0);
    await expect(page.getByTestId("nav-groups")).toBeVisible();
    await expect(page.getByTestId("workspace-wallet")).toContainText("Wallet connection");
    await expect(page.getByTestId("workspace-moderation")).toContainText("WoT");
    await expect(page.getByTestId("workspace-offline")).toHaveCount(0);
    await expect(page.getByTestId("nav-offline")).toBeVisible();

    await page.getByTestId("inspect-wallet").click();
    await expect(page.getByTestId("workspace-diagnostic")).toContainText("nmp.nip57.wallet");

    await page.locator("#diagnostics").scrollIntoViewIfNeeded();
    await expect(page.locator(".outbox-state")).toContainText("Runtime rejected action");
    await expect(page.locator(".outbox-state")).toContainText("unsupported_in_chirp_web");
  } finally {
    await relay.close();
  }
});

test("@wasm blocked workspace deep routes focus the requested destination", async ({ page }) => {
  test.setTimeout(90_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const routes = [
    ["wallet", "Wallet and zaps", "nmp.nip57.wallet"],
    ["moderation", "Trust and moderation", "nmp.trust_controls"],
  ] as const;

  try {
    for (const [route, title, capability] of routes) {
      await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}#${route}`);
      const shell = page.locator(SHELL);
      await expect(shell).toHaveAttribute("data-main-view", route, { timeout: 30_000 });
      await expect(page.getByTestId("nav-workspaces")).toHaveAttribute("aria-current", "page");
      await expect(page.getByRole("heading", { name: title })).toBeVisible();

      const focused = page.getByTestId(`workspace-${route}`);
      await expect(focused).toHaveAttribute("data-focused", "true");
      await expect(focused).toContainText("blocked");
      await page.getByTestId(`inspect-${route}`).click();
      await expect(page.getByTestId("workspace-diagnostic")).toContainText(capability);
    }
  } finally {
    await relay.close();
  }
});

test("@wasm storage workspace exposes runtime replay diagnostics", async ({ page }) => {
  test.setTimeout(90_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}#offline`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });

    const panel = page.getByTestId("offline-panel");
    await expect(panel).toBeVisible();
    await expect(page.getByRole("heading", { name: "Inspect storage health" })).toBeVisible();
    await expect(page.getByTestId("offline-storage-state")).toContainText(/Store|Storage/);
    await expect(panel).toContainText("Durable offline publish replay remains blocked");
    await expect(page.getByTestId("offline-relays")).toContainText("127.0.0.1", {
      timeout: 30_000,
    });
    await expect(page.getByTestId("offline-interests")).toContainText(/nmp\.|feed|profile/i, {
      timeout: 30_000,
    });
    await expect(page.getByTestId("offline-wires")).toContainText(/EOSE|waiting for EOSE/i, {
      timeout: 30_000,
    });
  } finally {
    await relay.close();
  }
});
