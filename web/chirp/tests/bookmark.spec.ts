/**
 * Bookmark acceptance (@wasm) — proves feed save state is backed by Rust's
 * NIP-51 kind:10003 projection and publishes real signed bookmark-list edits.
 */

import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm bookmarks: save toggle publishes signed NIP-51 bookmark add and remove edits", async ({
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

    const save = targetCard.getByRole("button", { name: /^bookmark$/i });
    await expect(save).toBeEnabled();
    await expect(save).toHaveAttribute("aria-pressed", "false");
    await save.click();

    await expect
      .poll(() => hasBookmarkEdit(relay, targetEventId!, true), { timeout: 30_000 })
      .toBe(true);
    await expect(targetCard.getByRole("button", { name: /remove bookmark/i })).toHaveAttribute(
      "aria-pressed",
      "true",
      { timeout: 30_000 },
    );

    await targetCard.getByRole("button", { name: /remove bookmark/i }).click();
    await expect
      .poll(() => hasBookmarkEdit(relay, targetEventId!, false), { timeout: 30_000 })
      .toBe(true);
    await expect(targetCard.getByRole("button", { name: /^bookmark$/i })).toHaveAttribute(
      "aria-pressed",
      "false",
      { timeout: 30_000 },
    );

    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });
    await expect(page.getByTestId("relay-verdicts").last()).toContainText("accepted", {
      timeout: 30_000,
    });
  } finally {
    await relay.close();
  }
});

function hasBookmarkEdit(
  relay: Awaited<ReturnType<typeof startFeedFixtureRelay>>,
  eventId: string,
  includesTarget: boolean,
): boolean {
  return relay.receivedEvents().some((event) => {
    if (event.kind !== 10003 || event.pubkey !== relay.viewerPubkey) return false;
    const bookmarks = event.tags
      .filter((tag) => tag[0] === "e")
      .map((tag) => tag[1]);
    return bookmarks.includes(eventId) === includesTarget;
  });
}
