import { expect, test } from "@playwright/test";
import { nip17, nip19 } from "nostr-tools";
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
    await expect(page.getByTestId("messages-conversation")).toContainText(
      relay.url.replace(/^wss?:\/\//, ""),
    );
    await expect
      .poll(
        () =>
          relay.subscriptions().some((filter) => {
            return (
              Array.isArray(filter.authors) &&
              filter.authors.includes(relay.senderPubkey) &&
              Array.isArray(filter.kinds) &&
              filter.kinds.includes(10050)
            );
          }),
        { timeout: 30_000 },
      )
      .toBe(true);
    await expect
      .poll(
        () =>
          relay
            .deliveredEvents()
            .some((event) => event.kind === 10050 && event.pubkey === relay.senderPubkey),
        { timeout: 30_000 },
      )
      .toBe(true);
    await expect
      .poll(
        () =>
          relay
            .deliveredEvents()
            .some((event) => event.kind === 10050 && event.pubkey === relay.viewerPubkey),
        { timeout: 30_000 },
      )
      .toBe(true);
    const outboundContent = "private reply from chirp web";
    await page.getByTestId("messages-content-input").fill(outboundContent);
    await page.getByTestId("messages-send-button").click();
    await expect
      .poll(
        () => relay.receivedEvents().filter((event) => event.kind === 1059).length,
        { timeout: 60_000 },
      )
      .toBeGreaterThanOrEqual(2);
    const outboundGiftWraps = relay.receivedEvents().filter((event) => event.kind === 1059);
    const recipientWrap = outboundGiftWraps.find((event) =>
      event.tags.some((tag) => tag[0] === "p" && tag[1] === relay.senderPubkey),
    );
    const selfCopyWrap = outboundGiftWraps.find((event) =>
      event.tags.some((tag) => tag[0] === "p" && tag[1] === relay.viewerPubkey),
    );
    expect(recipientWrap, "recipient gift-wrap was published to the relay").toBeTruthy();
    expect(selfCopyWrap, "self-copy gift-wrap was published to the relay").toBeTruthy();
    expect(nip17.unwrapEvent(recipientWrap!, relay.senderSecretKey).content).toBe(outboundContent);
    expect(nip17.unwrapEvent(selfCopyWrap!, relay.viewerSecretKey).content).toBe(outboundContent);
    await expect(page.getByTestId("messages-send-status")).toContainText(
      "Published through Rust-owned NIP-17",
      { timeout: 60_000 },
    );
  } finally {
    await relay.close();
  }
});
