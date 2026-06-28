import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm onboarding: no-extension browser reaches a complete local-key product session", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localNsec = nip19.nsecEncode(relay.viewerSecretKey);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-main-view", "setup");
    await expect(page.getByTestId("nav-setup")).toHaveAttribute("aria-current", "page");
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    const onboarding = page.locator(".onboarding-panel");
    await expect(onboarding).toBeVisible({ timeout: 10_000 });
    await expect(onboarding).toContainText("Get to a real session");
    await expect(onboarding).toContainText("Connect identity");
    await expect(onboarding).toContainText("Use NIP-07 for a normal account");

    await expect(page.locator(".signing-method", { hasText: "NIP-07 browser signer" })).toBeVisible();
    await expect(page.locator(".signing-method", { hasText: "Session nsec" })).toBeVisible();
    await expect(page.getByText("No extension detected in this browser")).toBeVisible();
    await expect(page.getByTestId("local-nsec-input")).toBeVisible();
    await expect(page.locator('[data-slot="feed"]')).toHaveCount(0);

    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();

    await expect(page.locator('[data-slot="active-signer"]')).toContainText("Local key", {
      timeout: 30_000,
    });
    await expect(shell).toHaveAttribute("data-main-view", "home");
    await expect(page.getByTestId("feed-timeline")).toContainText(relay.noteContent, {
      timeout: 60_000,
    });
    await expect(onboarding.locator(".onboarding-progress")).toHaveText("3/4");
    await expect(onboarding).toContainText("Try an action");

    const content = `onboarding acceptance ${Date.now()}`;
    await page.getByTestId("compose-input").fill(content);
    await page.getByRole("button", { name: "Post", exact: true }).click();
    await expect
      .poll(
        () => relay.receivedEvents().some((event) => event.kind === 1 && event.content === content),
        { timeout: 30_000 },
      )
      .toBe(true);

    await expect(onboarding).toContainText("Chirp is ready", { timeout: 30_000 });
    await expect(onboarding.locator(".onboarding-progress")).toHaveText("4/4");
    await expect(onboarding).toContainText("accepted");

    const storageText = await page.evaluate(() =>
      [
        ...Object.entries(localStorage),
        ...Object.entries(sessionStorage),
      ]
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    expect(storageText).not.toContain(localNsec);
  } finally {
    await relay.close();
  }
});
