import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm reposts: local-key repost publishes a signed NIP-18 kind:6 event", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localNsec = nip19.nsecEncode(relay.viewerSecretKey);

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    await expect(page.locator(SHELL)).toHaveAttribute("data-runtime-status", "running", {
      timeout: 30_000,
    });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();
    await expect(page.locator(".status-indicator")).toHaveAttribute("data-connected", "true", {
      timeout: 30_000,
    });

    const targetCard = page
      .getByTestId("post-card")
      .filter({ hasText: relay.noteContent })
      .first();
    await expect(targetCard).toBeVisible({ timeout: 60_000 });
    const targetEventId = await targetCard.getAttribute("data-event-id");
    expect(targetEventId).toMatch(/^[a-f0-9]{64}$/);

    const repost = targetCard.getByRole("button", { name: /^repost$/i });
    await expect(repost).toBeEnabled();
    await repost.click();

    await expect
      .poll(
        () =>
          relay.receivedEvents().some(
            (event) =>
              event.kind === 6 &&
              event.pubkey === relay.viewerPubkey &&
              event.content === "" &&
              event.tags.some((tag) => tag[0] === "e" && tag[1] === targetEventId) &&
              event.tags.some((tag) => tag[0] === "p" && tag[1] === relay.followPubkey) &&
              event.tags.some((tag) => tag[0] === "k" && tag[1] === "1"),
          ),
        { timeout: 30_000 },
      )
      .toBe(true);

    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });
    await expect(page.getByTestId("relay-verdicts")).toContainText("accepted", {
      timeout: 30_000,
    });
  } finally {
    await relay.close();
  }
});
