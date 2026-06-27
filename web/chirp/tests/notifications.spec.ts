import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startNotificationsFixtureRelay } from "./notifications-fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm notifications workspace renders Rust-owned p-tag inbox", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startNotificationsFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localNsec = nip19.nsecEncode(relay.viewerSecretKey);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}#notifications`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();

    const panel = page.getByTestId("notifications-panel");
    await expect(panel).toBeVisible({ timeout: 30_000 });
    await expect(panel).toContainText("Notifications");
    await expect(panel).toContainText(relay.replyContent, { timeout: 60_000 });
    await expect(panel).toContainText(relay.mentionContent);
    await expect(panel).toContainText("Reaction");
    await expect(panel).toContainText("Repost");
    await expect(panel).toContainText("127.0.0.1");
    await expect(page.getByTestId("notifications-source")).toHaveText(/[1-9]\d* unread/);
    await expect(page.getByTestId("notification-card").first()).toHaveAttribute(
      "data-read",
      "false",
    );

    await page.getByTestId("notifications-mark-read").click();
    await expect(page.getByTestId("notifications-source")).toContainText("0 unread");
    await expect(page.getByTestId("notification-card").first()).toHaveAttribute(
      "data-read",
      "true",
    );

    await expect
      .poll(
        () =>
          relay.subscriptions().some((filter) => {
            const pTags = filter["#p"];
            return (
              Array.isArray(pTags) &&
              pTags.includes(relay.viewerPubkey) &&
              Array.isArray(filter.kinds) &&
              [1, 6, 7].every((kind) => filter.kinds!.includes(kind))
            );
          }),
        { timeout: 30_000 },
      )
      .toBe(true);
  } finally {
    await relay.close();
  }
});
