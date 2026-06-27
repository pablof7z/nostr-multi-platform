import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startMessagesFixtureRelay } from "./messages-fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm messages workspace renders Rust-owned NIP-17 inbox", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startMessagesFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localNsec = nip19.nsecEncode(relay.viewerSecretKey);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}#messages`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-main-view", "messages");
    await expect(page.getByTestId("nav-messages")).toHaveAttribute("aria-current", "page");
    await expect(page.getByTestId("messages-panel")).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId("messages-signed-out")).toContainText("Connect a signer");

    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();

    const panel = page.getByTestId("messages-panel");
    await expect(panel).toContainText("live gift-wrap inbox", { timeout: 60_000 });
    await expect(panel).toContainText("decrypt ok", { timeout: 60_000 });
    await expect
      .poll(
        () =>
          relay.subscriptions().some((filter) => {
            const pTags = filter["#p"];
            return (
              Array.isArray(pTags) &&
              pTags.includes(relay.viewerPubkey) &&
              Array.isArray(filter.kinds) &&
              filter.kinds.includes(1059)
            );
          }),
        { timeout: 30_000 },
      )
      .toBe(true);
    await expect(page.getByTestId("messages-source")).toHaveText("1 threads / 1 messages");
    await expect(page.getByTestId("messages-thread")).toContainText(relay.messageContent);
    await expect(page.getByTestId("messages-conversation")).toContainText(
      relay.messageContent,
      { timeout: 60_000 },
    );
    await expect(page.getByTestId("messages-conversation")).toContainText(
      relay.senderPubkey.slice(0, 8),
    );
    await expect(page.getByTestId("messages-compose-blocked")).toContainText(
      "Sending is blocked on web",
    );

    await page.getByTestId("messages-send-diagnostic").click();
    await expect(page.getByTestId("messages-diagnostic")).toContainText("nmp.nip17.send");
  } finally {
    await relay.close();
  }
});
