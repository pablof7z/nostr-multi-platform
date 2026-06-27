/**
 * Feed acceptance (@wasm) — proves the real wasm runtime connects to a fixture
 * relay, ingests genuinely signed events, and pushes the resulting projection
 * to the shell.
 *
 * The shell exposes runtime state through the <main> data-* hooks and the feed
 * feature renders the runtime's home-feed projection. This spec asserts both:
 * real signed events flow end to end into a snapshot frame, and the feed UI
 * exposes the resulting post, profile, thread, and provenance surfaces.
 *
 * Scenario: connect to the fixture relay via ?relay_bootstrap=, install the
 * viewer identity via a stubbed NIP-07 window.nostr, and confirm the kernel
 * ingests the seeded contact-list + follow notes (data-has-snapshot="true",
 * runtime "running", relay connection observed).
 */

import { test, expect } from "@playwright/test";
import { startFeedFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm feed: real signed events from the fixture relay reach a snapshot after connect", async ({
  page,
}) => {
  // Boot + connect + relay round-trips for the contact list, two profiles and
  // two notes. Give the whole flow headroom beyond the default timeout.
  test.setTimeout(240_000);

  const relay = await startFeedFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

  try {
    // Inject the stubbed NIP-07 extension BEFORE the page loads so the connect
    // flow can call getPublicKey() with a real viewer pubkey. signEvent is
    // stubbed defensively in case any write path fires during the test.
    await page.addInitScript((viewerPubkeyHex: string) => {
      (window as unknown as { nostr: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(viewerPubkeyHex),
        signEvent: (event: Record<string, unknown>) =>
          Promise.resolve({
            ...event,
            pubkey: viewerPubkeyHex,
            id: Array.from(crypto.getRandomValues(new Uint8Array(32)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
            sig: Array.from(crypto.getRandomValues(new Uint8Array(64)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
          }),
      };
    }, relay.viewerPubkey);

    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const shell = page.locator(SHELL);

    // Runtime boots and reaches running (first UpdateFrame arrived).
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });

    // The wasm relay pool dialled the fixture relay.
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    // Install the viewer identity via the shell's connect affordance. This sends
    // set_identity(nip07) with the viewer pubkey; the kernel then grows the
    // active-follows feed and ingests the seeded contact list + notes.
    // SigningPanel uses data-action="connect-nip07" on the NIP-07 button;
    // there is no .connect-btn class in the real DOM.
    const connect = page.locator('[data-slot="signing"] [data-action="connect-nip07"]');
    await expect(connect).toBeVisible({ timeout: 10_000 });
    await connect.click();

    // The status indicator flips to connected once the first frame is present.
    await expect(page.locator(".status-indicator")).toHaveAttribute(
      "data-connected",
      "true",
      { timeout: 30_000 },
    );

    // The runtime keeps emitting frames after identity install (ingest of the
    // seeded events produces fresh snapshots) — the snapshot hook stays "true".
    await expect(shell).toHaveAttribute("data-has-snapshot", "true");
    await expect(shell).toHaveAttribute("data-runtime-status", "running");

    const feedSlot = page.locator('[data-slot="feed"]');
    await expect(feedSlot.getByTestId("post-card").first()).toBeVisible({ timeout: 60_000 });
    const firstCard = feedSlot.getByTestId("post-card").first();
    await expect(firstCard).toHaveAttribute("data-event-id", /^[a-f0-9]{64}$/);
    await expect(firstCard.getByRole("button", { name: /like/i })).toBeEnabled();

    const richCard = feedSlot.getByTestId("post-card").filter({ hasText: relay.noteContent }).first();
    await expect(richCard.getByTestId("post-content-hashtag")).toHaveText("#nostr");
    await expect(richCard.getByTestId("post-content-link")).toHaveAttribute(
      "href",
      relay.noteLinkUrl,
    );
    const media = richCard.getByTestId("post-media-grid");
    await expect(media).toBeVisible();
    await expect
      .poll(() =>
        media.locator("img").first().evaluate((img) => (img as HTMLImageElement).naturalWidth),
      )
      .toBeGreaterThan(0);

    await firstCard.getByRole("button", { name: /open profile/i }).click();
    const detail = feedSlot.getByTestId("feed-detail-panel");
    await expect(detail).toHaveAttribute("data-kind", "profile");
    await expect(detail.getByTestId("profile-detail")).toContainText(relay.followDisplayName);
    await expect(detail.getByTestId("profile-follow-toggle")).toBeEnabled();
    await expect(detail.getByTestId("profile-follow-toggle")).toHaveAttribute("aria-pressed", "true");
    await expect(detail.getByTestId("profile-relay-provenance")).toContainText(
      relay.url.replace(/^wss?:\/\//, ""),
    );
    const authoredPost = detail.getByTestId("profile-authored-post").first();
    await expect(authoredPost).toContainText(relay.noteContent);
    await expect(authoredPost.getByTestId("post-content-link")).toHaveAttribute(
      "href",
      relay.noteLinkUrl,
    );

    await richCard.getByRole("button", { name: /open thread/i }).click();
    await expect(detail).toHaveAttribute("data-kind", "thread");
    await expect(detail.getByTestId("thread-reply-attribution")).toContainText(
      relay.replierDisplayName,
    );
  } finally {
    await relay.close();
  }
});
