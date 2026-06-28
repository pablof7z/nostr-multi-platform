import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm thread: local-key reply publishes marked NIP-10 tags", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localSecretKey = generateSecretKey();
  const localNsec = nip19.nsecEncode(localSecretKey);
  const localPubkey = getPublicKey(localSecretKey);
  const content = `chirp thread reply ${Date.now()}`;

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();
    await expect(page.locator(".status-indicator")).toHaveAttribute("data-connected", "true", {
      timeout: 30_000,
    });

    const rootCard = page
      .getByTestId("post-card")
      .filter({ hasText: relay.noteContent })
      .first();
    await expect(rootCard).toBeVisible({ timeout: 60_000 });
    const parentId = await rootCard.getAttribute("data-event-id");
    expect(parentId).toMatch(/^[a-f0-9]{64}$/);

    await rootCard.getByRole("button", { name: /open thread/i }).click();
    const detail = page.getByTestId("feed-detail-panel");
    await expect(detail).toHaveAttribute("data-kind", "thread");
    await detail.getByTestId("thread-reply-input").fill(content);
    await detail.getByTestId("thread-reply-submit").click();

    await expect
      .poll(
        () =>
          relay
            .receivedEvents()
            .some((event) => event.kind === 1 && event.pubkey === localPubkey && event.content === content),
        { timeout: 30_000 },
      )
      .toBe(true);

    const replyEvent = relay
      .receivedEvents()
      .find((event) => event.kind === 1 && event.pubkey === localPubkey && event.content === content);
    expect(replyEvent).toBeDefined();
    const tags = replyEvent!.tags;
    expect(tags).toEqual(
      expect.arrayContaining([
        expect.arrayContaining(["e", parentId, "", "root"]),
        expect.arrayContaining(["e", parentId, "", "reply"]),
        expect.arrayContaining(["p", relay.followPubkey]),
      ]),
    );
    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });
  } finally {
    await relay.close();
  }
});
