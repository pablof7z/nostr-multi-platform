/**
 * PR-W3 Boot Smoke — proves the real NMP wasm runtime boots in a real
 * browser against a real (fixture) relay.
 *
 * Three honest assertions — NONE of these can pass on a DegradedRuntime:
 *
 *   1. `[data-testid="nmp-has-snapshot"]` appears in the DOM within 30 s.
 *      This element is rendered by RuntimePanel only when
 *      `snapshot.latestUpdateBytes !== undefined`.  `latestUpdateBytes` is
 *      set the first time an `update_bytes` WorkerEvent arrives.
 *      DegradedRuntime.handle() NEVER emits `update_bytes` — it can only
 *      emit runtime_status / hello_accepted / error / action_accepted /
 *      capability_failure.  Binary snapshot frames are exclusively emitted
 *      by the real NmpWasmRuntime (via WasmBridge).  Seeing this element
 *      is direct proof the real wasm was loaded AND pushed at least one
 *      snapshot frame to the bridge.
 *
 *   2. `[data-testid="nmp-bridge-kind"]` does NOT contain "in-process
 *      fallback".  This confirms the Web Worker was constructed (the
 *      InProcessNmpClient fallback never fires).  Belt-and-suspenders
 *      alongside (1).
 *
 *   3. The fixture relay received at least one inbound WebSocket
 *      connection from the browser.  The relay URL was injected as
 *      `?relay=<url>` and forwarded to `client.start()` as
 *      `relay_bootstrap`, overriding the hardcoded chirp defaults.
 *      DegradedRuntime never dials any relays — relay connections are
 *      opened exclusively by the real wasm runtime via nmp-core's relay
 *      pool.  Observed from the Node.js relay side (not the DOM) so no
 *      dependency on FlatBuffers snapshot decoding.
 *
 * ── KNOWN LIMITATION ──────────────────────────────────────────────────
 * The TypeScript FlatBuffers bindings for SnapshotFrame were generated
 * from an older schema and only expose `payload:Value` (field 1) and
 * `schemaVersion()` (field 0).  PR-B (#991/#979) deprecated and zeroed
 * `payload`; all projection data is now in `typed_projections` (field 2)
 * and Tier-3 fields such as `relay_statuses:[RelayStatus]` (field 10).
 * Decoding the relay-connected state from a live snapshot therefore
 * requires regenerating the TypeScript bindings — tracked as issue #1007
 * (post-v1).  The relay-row DOM assertion (relay URL + "Connected" from
 * a decoded snapshot) is intentionally deferred to that issue rather than
 * faked here.  Assertion 3 above proves the same relay I/O fact from the
 * relay side without needing snapshot decoding.
 * ──────────────────────────────────────────────────────────────────────
 *
 * The fixture relay (fixture-relay.ts) runs in the Node test process and
 * listens on a random loopback port.  The app receives the relay URL via
 * the `?relay=` query parameter, which App.tsx reads and passes to
 * client.start() as relay_bootstrap, overriding the hardcoded chirp URLs.
 */

import { test, expect } from "@playwright/test";
import { startFixtureRelay } from "./fixture-relay.js";

test("NMP wasm runtime boots real wasm in browser against fixture relay", async ({ page }) => {
  const relay = await startFixtureRelay();

  try {
    // Navigate to the app with the fixture relay URL injected.
    await page.goto(`/?relay=${encodeURIComponent(relay.url)}`);

    // ── Assertion 1: real wasm emitted binary snapshot frames ──────────
    //
    // RuntimePanel renders a hidden <span data-testid="nmp-has-snapshot">
    // when snapshot.latestUpdateBytes !== undefined.  latestUpdateBytes is
    // set on every update_bytes WorkerEvent — an event type that only the
    // real NmpWasmRuntime (via WasmBridge) ever emits.
    // DegradedRuntime.handle() cannot reach the update_bytes path.
    await expect(page.locator('[data-testid="nmp-has-snapshot"]')).toHaveCount(1, {
      timeout: 30_000,
    });

    // ── Assertion 2: Worker path active (belt-and-suspenders) ──────────
    //
    // client.ts uses "in_process_fallback" when the Worker API is
    // unavailable or construction fails.  Chromium supports Workers so
    // this should always say "worker v1".
    const bridgeKind = page.locator('[data-testid="nmp-bridge-kind"]');
    await expect(bridgeKind).not.toContainText("in-process fallback");

    // ── Assertion 3: fixture relay received a real WS connection ───────
    //
    // The wasm relay pool opens WebSocket connections to bootstrap relays
    // during kernel startup.  The fixture relay URL was injected via
    // ?relay= and passed as relay_bootstrap.  We poll connectionCount()
    // (incremented on the Node side for each WS upgrade) until it reaches
    // 1.  DegradedRuntime never opens any relay connections, so a count
    // ≥ 1 proves the real runtime's relay pool fired at least once.
    await expect.poll(() => relay.connectionCount(), { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
  } finally {
    await relay.close();
  }
});
