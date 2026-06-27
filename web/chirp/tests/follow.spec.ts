/**
 * Follow acceptance (@wasm) — proves the profile follow toggle is backed by the
 * Rust NIP-02 projection and publishes real signed kind:3 edits.
 */

import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm follow: profile toggle publishes signed NIP-02 follow and unfollow edits", async ({
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

    const feedSlot = page.locator('[data-slot="feed"]');
    const targetCard = feedSlot
      .getByTestId("post-card")
      .filter({ hasText: relay.noteContent })
      .first();
    await expect(targetCard).toBeVisible({ timeout: 60_000 });
    await targetCard.getByRole("button", { name: /open profile/i }).click();

    const detail = feedSlot.getByTestId("feed-detail-panel");
    await expect(detail).toHaveAttribute("data-kind", "profile");
    const toggle = detail.getByTestId("profile-follow-toggle");
    await expect(toggle).toHaveText("Following", { timeout: 30_000 });
    await expect(toggle).toHaveAttribute("aria-pressed", "true");

    await toggle.click();
    await expect
      .poll(() => relay.receivedEvents().length, { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1);
    await expect(toggle).toHaveText("Follow", { timeout: 30_000 });
    await expect(toggle).toHaveAttribute("aria-pressed", "false");
    await expect
      .poll(() => hasContactEdit(relay, false), { timeout: 30_000 })
      .toBe(true);

    await toggle.click();
    await expect(toggle).toHaveText("Following", { timeout: 30_000 });
    await expect(toggle).toHaveAttribute("aria-pressed", "true");
    await expect
      .poll(() => hasContactEdit(relay, true), { timeout: 30_000 })
      .toBe(true);

    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });
    await expect(
      page.getByTestId("relay-verdicts").filter({ hasText: "accepted" }).first(),
    ).toBeVisible({ timeout: 30_000 });
  } finally {
    await relay.close();
  }
});

function hasContactEdit(
  relay: Awaited<ReturnType<typeof startFeedFixtureRelay>>,
  includesPrimaryFollow: boolean,
): boolean {
  return relay.receivedEvents().some((event) => {
    if (event.kind !== 3 || event.pubkey !== relay.viewerPubkey) return false;
    const follows = event.tags
      .filter((tag) => tag[0] === "p")
      .map((tag) => tag[1]);
    return (
      follows.includes(relay.secondFollowPubkey) &&
      follows.includes(relay.followPubkey) === includesPrimaryFollow
    );
  });
}
