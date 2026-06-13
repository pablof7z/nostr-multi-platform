/**
 * Inspector smoke test — proves the NMP Inspector dock renders real decoded
 * Tier-3 data from the wasm kernel.
 *
 * Two assertions beyond the boot.spec.ts baseline:
 *
 *   1. Opening the Inspector (clicking the pulse strip) reveals at least one
 *      `[data-testid="relay-row"]` in the expanded panel.  This confirms the
 *      Relays panel decodes and renders the Tier-3 `relay_statuses` vector.
 *
 *   2. At least one relay row carries a non-zero `bytesRx` metric (bytes the
 *      kernel received from that relay).  After the wasm relay pool connects to
 *      the fixture relay, the kernel emits bootstrap REQs (triggered by the
 *      Connect step that sets the viewer pubkey), and the fixture relay responds
 *      with EOSE, incrementing the per-URL `bytesRx` counter tracked by the
 *      kernel's ingest path.  This proves the expanded `DecodedRelayStatus`
 *      fields are decoded and surfaced in the DOM — not fabricated zeros.
 *
 *   Note on bytesTx: the per-URL `bytes_tx` counter in the kernel is updated
 *   only by the native relay-management actor (`relay_mgmt.rs::record_tx_to`).
 *   The browser wasm path fans outbound messages via `fan_out_outbound` which
 *   calls `driver.send_text` directly without updating that counter, so
 *   `bytesTx` is structurally 0 for browser relay connections.  We test
 *   `bytesRx` instead, which IS incremented by the kernel's ingest path
 *   (`ingest/mod.rs::record_transport_rx`) on every inbound text/binary frame.
 *
 * Preconditions:
 *   - Real wasm is loaded (fixture relay URL injected via ?relay=).
 *   - window.nostr is mocked (injected before page load) so Connect works
 *     headless and the kernel gets an active account, enabling bootstrap REQs.
 *   - The inspector dock is in its default COLLAPSED state on page load.
 *   - Clicking `[data-testid="inspector-toggle"]` expands the dock.
 *   - The "Relays" tab is not the default tab, so we click it after opening.
 */

import { test, expect } from "@playwright/test";
import { startFixtureRelay } from "./fixture-relay.js";

// Fixed test pubkey — valid 32-byte hex, no real key material needed since the
// fixture relay ignores authors and just EOSEs everything.
const TEST_PUBKEY = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

test("Inspector dock renders real relay rows and non-zero bytesRx after kernel connects", async ({
  page,
}) => {
  const relay = await startFixtureRelay();

  try {
    // Inject mock window.nostr BEFORE the page loads so the Connect flow can
    // call getPublicKey() synchronously.  The fixture relay ignores authors,
    // so any valid-looking pubkey hex works here.
    await page.addInitScript((pubkey: string) => {
      (window as Window & { nostr?: unknown }).nostr = {
        getPublicKey: () => Promise.resolve(pubkey),
        signEvent: (event: Record<string, unknown>) =>
          Promise.resolve({
            ...event,
            pubkey,
            id: Array.from(crypto.getRandomValues(new Uint8Array(32)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
            sig: Array.from(crypto.getRandomValues(new Uint8Array(64)))
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(""),
          }),
      };
    }, TEST_PUBKEY);

    await page.goto(`/?relay=${encodeURIComponent(relay.url)}`);

    // Wait for the wasm runtime to boot and push at least one snapshot frame
    // (same guard as boot.spec.ts assertion 1 — ensures the real wasm is up).
    await expect(page.locator('[data-testid="nmp-has-snapshot"]')).toHaveCount(1, {
      timeout: 30_000,
    });

    // Wait for the fixture relay to have received a connection from the wasm
    // relay pool.  This proves the browser relay driver dialled the relay URL
    // before we check any per-URL metrics.
    await expect.poll(() => relay.connectionCount(), { timeout: 20_000 }).toBeGreaterThanOrEqual(1);

    // Click Connect so the kernel gets an active account and emits bootstrap
    // REQs toward the fixture relay.  The fixture relay will EOSE each one,
    // incrementing the per-URL bytesRx counter in the kernel's ingest path.
    await expect(page.locator('[data-testid="connect-btn"]')).toBeVisible({ timeout: 10_000 });
    await page.locator('[data-testid="connect-btn"]').click();

    // Expand the Inspector dock by clicking the pulse strip.
    await page.locator('[data-testid="inspector-toggle"]').click();

    // Navigate to the Relays tab.
    await page.locator('button[role="tab"]', { hasText: "Relays" }).click();

    // ── Assertion 1: relay rows decoded from Tier-3 relay_statuses ──────────
    //
    // PanelRelays renders <div data-testid="relay-row"> for each relay in
    // latestRelayStatuses, decoded from the FlatBuffers Tier-3 vector.
    // The fixture relay URL was passed as relay_bootstrap so the kernel reports
    // at least one relay row.
    await expect
      .poll(
        () => page.locator('[data-testid="relay-row"]').count(),
        { timeout: 20_000 },
      )
      .toBeGreaterThanOrEqual(1);

    // ── Assertion 2: non-zero bytesRx in at least one relay row ─────────────
    //
    // PanelRelays renders `data-testid="relay-bytes-rx"` for each relay row
    // showing the bytesRx counter from `DecodedRelayStatus`.  After Connect
    // the kernel emits bootstrap REQs; the fixture relay responds with EOSE
    // for each, which the kernel's ingest path records as received bytes.
    // A zero value here would mean either the wasm never sent any REQ
    // (impossible given the Connect step and connectionCount guard above) or
    // the DecodedRelayStatus bytesRx field is not being decoded from
    // FlatBuffers.
    await expect
      .poll(
        async () => {
          const elements = page.locator('[data-testid="relay-bytes-rx"]');
          const count = await elements.count();
          for (let i = 0; i < count; i++) {
            const text = (await elements.nth(i).textContent()) ?? "";
            // Text format: "rx 1234 B" — extract the numeric part.
            const match = text.match(/rx\s+([\d]+)/);
            if (match && parseInt(match[1]!, 10) > 0) return true;
          }
          return false;
        },
        { timeout: 20_000 },
      )
      .toBe(true);
  } finally {
    await relay.close();
  }
});
