import { expect, test } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test("@wasm profile: local-key onboarding signs and publishes metadata", async ({ page }) => {
  test.setTimeout(120_000);

  const relay = await startFixtureRelay({ eventAck: { delayMs: 1_000 } });
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localSecretKey = generateSecretKey();
  const localNsec = nip19.nsecEncode(localSecretKey);
  const localPubkey = getPublicKey(localSecretKey);
  const name = `Chirp Local ${Date.now()}`;
  const about = "Profile publish acceptance via NMP browser runtime";
  const picture = "https://chirp.f7z.io/icon.svg";

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

    await page.getByTestId("nav-profile").click();
    await expect(shell).toHaveAttribute("data-main-view", "profile");

    await page.getByTestId("profile-name-input").fill(name);
    await page.getByTestId("profile-about-input").fill(about);
    await page.getByTestId("profile-picture-input").fill(picture);
    await page.getByTestId("profile-publish-submit").click();

    await expect(page.getByTestId("publish-outbox")).toContainText("in flight", {
      timeout: 15_000,
    });
    await expect
      .poll(() => relay.eventCount(), { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1);
    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });
    await expect(page.getByTestId("relay-verdicts")).toContainText("127.0.0.1", {
      timeout: 30_000,
    });
    await expect(page.getByTestId("relay-verdicts")).toContainText("accepted", {
      timeout: 30_000,
    });

    const profileEvent = relay.receivedEvents().find((event) =>
      event.kind === 0 && event.pubkey === localPubkey
    );
    expect(profileEvent).toBeDefined();
    const metadata = JSON.parse(profileEvent?.content ?? "{}") as Record<string, string>;
    expect(metadata).toMatchObject({ name, about, picture });
  } finally {
    await relay.close();
  }
});
