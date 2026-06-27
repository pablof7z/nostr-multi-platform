/**
 * Publish acceptance (@wasm) — proves a composed note is signed via a stubbed
 * NIP-07 extension and published as a valid signed EVENT the fixture relay
 * receives. Covers acceptance scenarios 3 (publish → relay receives signed
 * EVENT) and 4 (stubbed window.nostr → publish round-trips).
 *
 * Signing path under test (no NIP-46): the wasm worker emits a `sign_request`,
 * the Item B shell's main-thread broker (signBroker.ts) calls
 * window.nostr.signEvent, and the worker publishes the pre-signed event through
 * the kernel's outbox to the bootstrap relay. We stub window.nostr with a REAL
 * secp256k1 signer (nostr-tools finalizeEvent over an exposed Node function), so
 * the EVENT the relay receives carries a genuine signature — the kernel's
 * publish path rejects malformed events, so a forged stub would not round-trip.
 *
 * Compose-UI dependency (Item C/D, building in parallel): the Item B shell has
 * no compose box yet — the [data-slot="feed"]/[data-slot="signing"] mount points
 * are populated by Items C/D. This spec discovers the compose affordance at
 * runtime and, if it is not yet present, SKIPS with an explicit reason rather
 * than silently passing. The moment Items C/D land the compose UI on master,
 * this spec executes the full round-trip with no further edits here. Selectors
 * cover the conventions used by the pre-rebuild suite plus data-slot fallbacks.
 */

import { expect, test, type BrowserContext } from "@playwright/test";
import { nip19 } from "nostr-tools";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { startFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

// Candidate selectors for the compose box + publish button. Item C/D may use any
// of these; the spec uses the first that resolves. Kept broad on purpose so the
// spec activates regardless of which convention the compose UI ships with.
const COMPOSE_SELECTORS = [
  'textarea[aria-label="Compose chirp"]',
  '[data-testid="compose-input"]',
  '[data-slot="feed"] textarea',
  '[data-slot="signing"] textarea',
  "main textarea",
];

async function findCompose(page: import("@playwright/test").Page) {
  for (const selector of COMPOSE_SELECTORS) {
    const locator = page.locator(selector).first();
    const visible = await locator.isVisible().catch(() => false);
    if (visible) return locator;
  }
  return undefined;
}

test("@wasm publish: composed note is NIP-07 signed and the relay receives a valid EVENT", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFixtureRelay({ eventAck: { delayMs: 2_500 } });
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const viewerSecretKey = generateSecretKey();
  const viewerPubkey = getPublicKey(viewerSecretKey);
  const content = `chirp acceptance publish ${Date.now()}`;
  const signedEvents: Record<string, unknown>[] = [];

  try {
    // Real secp256k1 signing in Node, exposed to the page so the NIP-07 stub
    // produces genuinely signed events the kernel will accept and publish.
    await page.exposeFunction("signNostrEvent", async (event: Record<string, unknown>) => {
      const signed = finalizeEvent(
        event as Parameters<typeof finalizeEvent>[0],
        viewerSecretKey,
      ) as unknown as Record<string, unknown>;
      signedEvents.push(signed);
      return signed;
    });
    await page.addInitScript((viewerPubkeyHex: string) => {
      (window as unknown as { nostr: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(viewerPubkeyHex),
        signEvent: (event: Record<string, unknown>) =>
          (
            window as unknown as {
              signNostrEvent(e: Record<string, unknown>): Promise<Record<string, unknown>>;
            }
          ).signNostrEvent(event),
      };
    }, viewerPubkey);

    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    // Install the viewer identity via the shell's connect affordance.
    // SigningPanel uses data-action="connect-nip07" on the NIP-07 button;
    // there is no .connect-btn class in the real DOM.
    const connect = page.locator('[data-slot="signing"] [data-action="connect-nip07"]');
    await expect(connect).toBeVisible({ timeout: 10_000 });
    await connect.click();
    await expect(page.locator(".status-indicator")).toHaveAttribute("data-connected", "true", {
      timeout: 30_000,
    });

    // Discover the compose UI (Item C/D). Skip with an explicit reason if absent.
    const compose = await findCompose(page);
    test.skip(
      compose === undefined,
      "Compose UI not present in the Item B shell yet — Items C/D own [data-slot=\"feed\"]/" +
        "[data-slot=\"signing\"]. This @wasm publish round-trip activates automatically once " +
        "the compose box + publish button land on master (no edits needed here).",
    );
    if (compose === undefined) return; // satisfies the type checker after skip

    // Compose + publish.
    await compose.fill(content);
    await page.getByRole("button", { name: /publish|chirp|post|send/i }).first().click();

    await expect(page.getByTestId("publish-outbox")).toContainText("in flight", {
      timeout: 15_000,
    });
    await expect(page.getByTestId("publish-outbox")).toContainText(content, {
      timeout: 15_000,
    });
    await expect(page.getByTestId("publish-outbox")).toContainText(/sending|pending|retrying/i, {
      timeout: 15_000,
    });

    // The fixture relay received the published EVENT.
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

    // The NIP-07 stub signed exactly the note (scenario 4: round-trip).
    expect(signedEvents.length).toBeGreaterThanOrEqual(1);

    // The relay received a kind:1 EVENT from the viewer with the exact content
    // (scenario 3: valid signed EVENT — the kernel would reject a bad signature
    // before publishing, so receipt proves the signature is genuine).
    expect(relay.receivedEvents()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ kind: 1, pubkey: viewerPubkey, content }),
      ]),
    );
  } finally {
    await relay.close();
  }
});

test("@wasm publish: local-key onboarding signs and publishes without a browser extension", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFixtureRelay({ eventAck: { delayMs: 1_000 } });
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localSecretKey = generateSecretKey();
  const localNsec = nip19.nsecEncode(localSecretKey);
  const localPubkey = getPublicKey(localSecretKey);
  const content = `chirp local-key publish ${Date.now()}`;

  try {
    await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const shell = page.locator(SHELL);
    await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });
    await expect
      .poll(() => relay.connectionCount(), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(1);

    await expect(page.getByText("Connect signer")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("local-nsec-input")).toBeVisible({ timeout: 10_000 });
    await page.getByTestId("local-nsec-input").fill(localNsec);
    await page.getByTestId("local-nsec-submit").click();

    await expect(page.locator(".status-indicator")).toHaveAttribute("data-connected", "true", {
      timeout: 30_000,
    });
    await expect(page.locator('[data-slot="active-signer"]')).toContainText("Local key");
    await expect(page.getByTestId("local-nsec-input")).toHaveCount(0);

    const compose = await findCompose(page);
    test.skip(
      compose === undefined,
      "Compose UI not present in the Item B shell yet — local-key publish round-trip activates " +
        "automatically once the compose box + publish button land on master.",
    );
    if (compose === undefined) return;

    await compose.fill(content);
    await page.getByRole("button", { name: /publish|chirp|post|send/i }).first().click();

    await expect(page.getByTestId("publish-outbox")).toContainText("in flight", {
      timeout: 15_000,
    });
    await expect(page.getByTestId("publish-outbox")).toContainText(content, {
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
    expect(relay.receivedEvents()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ kind: 1, pubkey: localPubkey, content }),
      ]),
    );
  } finally {
    await relay.close();
  }
});

test("@wasm publish: accepted local-key note refetches in a second browser session", async ({
  page,
}) => {
  test.setTimeout(120_000);

  const relay = await startFixtureRelay();
  const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);
  const localSecretKey = generateSecretKey();
  const localNsec = nip19.nsecEncode(localSecretKey);
  const localPubkey = getPublicKey(localSecretKey);
  const content = `chirp refetch proof ${Date.now()}`;
  let secondContext: BrowserContext | undefined;

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

    const compose = await findCompose(page);
    test.skip(
      compose === undefined,
      "Compose UI not present — second-session refetch proof activates once compose lands.",
    );
    if (compose === undefined) return;

    await compose.fill(content);
    await page.getByRole("button", { name: /publish|chirp|post|send/i }).first().click();

    await expect
      .poll(() => relay.eventCount(), { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1);
    expect(relay.receivedEvents()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ kind: 1, pubkey: localPubkey, content }),
      ]),
    );
    await expect(page.getByTestId("action-results")).toContainText(/published|accepted/i, {
      timeout: 30_000,
    });

    const browser = page.context().browser();
    if (browser === null) throw new Error("Playwright browser handle unavailable");
    secondContext = await browser.newContext();
    const secondPage = await secondContext.newPage();
    await secondPage.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

    const secondShell = secondPage.locator(SHELL);
    await expect(secondShell).toHaveAttribute("data-runtime-status", "running", {
      timeout: 30_000,
    });
    await expect(secondShell).toHaveAttribute("data-has-snapshot", "true", {
      timeout: 30_000,
    });
    await expect(secondPage.getByTestId("feed-timeline")).toContainText(content, {
      timeout: 60_000,
    });
  } finally {
    await secondContext?.close();
    await relay.close();
  }
});
