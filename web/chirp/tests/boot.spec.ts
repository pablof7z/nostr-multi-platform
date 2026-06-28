/**
 * Boot acceptance — proves the Chirp Web shell (Item B) boots the runtime and
 * surfaces the data-* test hooks the rest of the suite asserts against.
 *
 * Two tiers, separated by the `@wasm` tag so CI can run them independently:
 *
 *   • Fallback smoke (NO @wasm) — runs WITHOUT the wasm artifact, so it is the
 *     PR-blocking browser coverage. By deleting `window.Worker` before the app
 *     module loads we force `createNmpClient()` down the InProcessNmpClient /
 *     DegradedRuntime path. This proves the shell mounts, parses the URL, sends
 *     `start`, and renders the three data-* hooks with honest degraded values.
 *
 *   • Real wasm boot (@wasm) — requires the built wasm artifact under
 *     public/nmp-browser-runtime/, so it runs in the nightly full-e2e job. It proves the
 *     real NmpWasmRuntime boots in a worker, reaches "running", emits at least
 *     one UpdateFrame (data-has-snapshot="true"), and dials the fixture relay.
 *
 * Hooks (rendered on <main> by App.tsx):
 *   data-bridge-kind     = "worker" | "in_process_fallback"
 *   data-runtime-status  = "running" | "ready" | "degraded:<reason>"
 *   data-has-snapshot    = "true" once the first UpdateFrame arrives
 */

import { test, expect } from "@playwright/test";
import { startFixtureRelay } from "./fixture-relay.js";

const SHELL = "main.app-shell";

test.describe("boot", () => {
  test("fallback path: shell boots degraded without a worker (no wasm needed)", async ({
    page,
  }) => {
    // Remove the Worker constructor BEFORE any app code runs. createNmpClient()
    // sees `typeof Worker === "undefined"` and returns InProcessNmpClient, which
    // drives DegradedRuntime("browser_bridge_unavailable").
    await page.addInitScript(() => {
      // @ts-expect-error — intentionally unset the global for the fallback path.
      delete window.Worker;
    });

    await page.goto("/");

    const shell = page.locator(SHELL);

    // The shell mounts and renders the bridge-kind hook for the fallback path.
    await expect(shell).toHaveAttribute("data-bridge-kind", "in_process_fallback", {
      timeout: 15_000,
    });

    // DegradedRuntime reports degraded:browser_bridge_unavailable after `start`.
    await expect(shell).toHaveAttribute(
      "data-runtime-status",
      "degraded:browser_bridge_unavailable",
      { timeout: 15_000 },
    );

    // No UpdateFrame is ever emitted on the degraded path.
    await expect(shell).toHaveAttribute("data-has-snapshot", "false");

    // The signing slot stays honest when the runtime cannot install signers.
    await expect(page.locator('[data-slot="signing"] .signing-degraded')).toBeVisible();
  });

  test("@wasm real wasm runtime boots in a worker and dials the fixture relay", async ({
    page,
  }) => {
    const relay = await startFixtureRelay();
    const relayBootstrap = JSON.stringify([[relay.url, "both,indexer"]]);

    try {
      await page.goto(`/?relay_bootstrap=${encodeURIComponent(relayBootstrap)}`);

      const shell = page.locator(SHELL);

      // The real client constructs a Worker, so the bridge kind is "worker".
      await expect(shell).toHaveAttribute("data-bridge-kind", "worker", { timeout: 30_000 });

      // The first UpdateFrame flips data-has-snapshot to "true" — an event only
      // the real NmpWasmRuntime can emit (DegradedRuntime never produces one).
      await expect(shell).toHaveAttribute("data-has-snapshot", "true", { timeout: 30_000 });

      // On the first frame client.ts mirrors the kernel run-state to "running".
      await expect(shell).toHaveAttribute("data-runtime-status", "running", { timeout: 30_000 });

      // The wasm relay pool dialled the bootstrap relay. Observed Node-side, so
      // it does not depend on any FlatBuffers snapshot decoding in the browser.
      await expect
        .poll(() => relay.connectionCount(), { timeout: 20_000 })
        .toBeGreaterThanOrEqual(1);

      // Product readiness surface: first-run starts in setup, while relay
      // diagnostics remain reachable from the rail without developer tools.
      await expect(shell).toHaveAttribute("data-main-view", "setup");
      await expect(page.locator(".onboarding-panel")).toBeVisible();
      await page.getByRole("link", { name: "Home" }).click();
      await expect(shell).toHaveAttribute("data-main-view", "home");
      const emptyFeed = page.getByTestId("feed-empty");
      await expect(emptyFeed).toContainText("Search");
      await expect(emptyFeed).toContainText("Groups");
      await expect(emptyFeed).toContainText("Relays");
      await expect(emptyFeed).toContainText("Signer");
      await page.getByRole("link", { name: "Saved" }).click();
      await expect(shell).toHaveAttribute("data-main-view", "saved");
      const emptySaved = page.getByTestId("saved-empty");
      await expect(emptySaved).toContainText("Home");
      await expect(emptySaved).toContainText("Search");
      await page.getByRole("link", { name: "Signer" }).click();
      await expect(shell).toHaveAttribute("data-main-view", "signer");
      await expect(page.getByTestId("nav-signer")).toHaveAttribute("aria-current", "page");
      await expect(page.locator('[data-slot="signing"]')).toBeVisible();
      await expect(page.locator('[data-slot="feed"]')).toHaveCount(0);
      await page.getByRole("link", { name: "Relays" }).click();
      await expect(shell).toHaveAttribute("data-main-view", "relays");
      await expect(page.locator(".relay-row").first()).toBeVisible();
      await page.getByRole("link", { name: "Diagnostics" }).click();
      await expect(shell).toHaveAttribute("data-main-view", "diagnostics");
      await expect(page.locator(".diagnostics-panel")).toBeVisible();
      await page.getByRole("button", { name: /routing/i }).click();
      await expect(page.locator('[data-testid="routing-trace"]')).toBeVisible();
    } finally {
      await relay.close();
    }
  });
});
