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
 *   2. At least one relay row carries a non-zero `bytesTx` metric (bytes the
 *      kernel sent to that relay).  After the wasm relay pool connects to the
 *      fixture relay and sends a REQ, `bytesTx` will be > 0.  This proves the
 *      expanded `DecodedRelayStatus` fields are decoded and surfaced in the DOM
 *      — not fabricated zeros.
 *
 * Preconditions (inherited from boot.spec.ts pattern):
 *   - Real wasm is loaded (fixture relay URL injected via ?relay=).
 *   - The inspector dock is in its default COLLAPSED state on page load.
 *   - Clicking `[data-testid="inspector-toggle"]` expands the dock.
 *   - The "Relays" tab is not the default tab, so we click it after opening.
 */

import { test, expect } from "@playwright/test";
import { startFixtureRelay } from "./fixture-relay.js";

test("Inspector dock renders real relay rows and non-zero bytesTx after kernel connects", async ({
  page,
}) => {
  const relay = await startFixtureRelay();

  try {
    await page.goto(`/?relay=${encodeURIComponent(relay.url)}`);

    // Wait for the wasm runtime to boot and push at least one snapshot frame
    // (same guard as boot.spec.ts assertion 1 — ensures the real wasm is up).
    await expect(page.locator('[data-testid="nmp-has-snapshot"]')).toHaveCount(1, {
      timeout: 30_000,
    });

    // Also wait for the fixture relay to have received a connection from the
    // wasm relay pool before checking metrics (bytesTx requires at least one
    // REQ to have been sent).
    await expect.poll(() => relay.connectionCount(), { timeout: 20_000 }).toBeGreaterThanOrEqual(1);

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

    // ── Assertion 2: non-zero bytesTx in at least one relay row ─────────────
    //
    // PanelRelays renders `data-testid="relay-bytes-tx"` for each relay row
    // showing the bytesTx counter from `DecodedRelayStatus`.  After the kernel
    // connects to the fixture relay and sends a REQ (which it does for logical
    // interests), bytesTx will be > 0 for that relay.  A zero value here would
    // mean either the wasm never connected (impossible given assertion 1 and
    // the relay.connectionCount() guard above) or the DecodedRelayStatus
    // bytesTx field is not being decoded.
    await expect
      .poll(
        async () => {
          const elements = page.locator('[data-testid="relay-bytes-tx"]');
          const count = await elements.count();
          for (let i = 0; i < count; i++) {
            const text = (await elements.nth(i).textContent()) ?? "";
            // Text format: "tx 1234 B" — extract the numeric part.
            const match = text.match(/tx\s+([\d]+)/);
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
