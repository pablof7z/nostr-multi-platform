/**
 * PR-W3 Boot Smoke — proves the real NMP wasm runtime boots in a real
 * browser against a real (fixture) relay, and that decoded snapshot data
 * reaches the UI (post-#1209 TS bindings regen).
 *
 * Five honest assertions — NONE of these can pass on a DegradedRuntime:
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
 *   4. `[data-testid="nmp-runtime-status"]` contains "running".
 *      Proves the TS bindings regen (#1209) fixed the decode path: the
 *      Tier-3 `running` field is now read from each SnapshotFrame and
 *      surfaced as RuntimeStatus "running".  DegradedRuntime never sets
 *      this element to "running"; a broken decode would leave it "degraded".
 *
 *   5. At least one `[data-testid="relay-row"]` is present (>= 1 row).
 *      Proves the Tier-3 `relay_statuses` vector is decoded from the real
 *      FlatBuffers snapshot and surfaced through RuntimePanel's relay list.
 *      If the TS bindings decode is broken the relay rows stay empty.
 *      (The kernel may report multiple rows per relay URL as connection
 *      state advances; the assertion is >= 1, not a fixed count.)
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

    // ── Assertion 4: runtime status reads "running" (#1209 bindings regen) ──
    //
    // RuntimePanel renders <strong data-testid="nmp-runtime-status"> with the
    // labelled status.  Before #1209, decodeUpdateFrameBytes threw on every
    // real frame (zeroed deprecated payload) and client.ts set status to
    // "browser actor driver missing".  After regen, the Tier-3 `running`
    // field is decoded and status advances to "running".  Failing here means
    // the TS bindings decode is still broken.
    const runtimeStatus = page.locator('[data-testid="nmp-runtime-status"]');
    await expect(runtimeStatus).toHaveText("running", { timeout: 30_000 });

    // ── Assertion 5: relay row decoded from Tier-3 relay_statuses ──────────
    //
    // RuntimePanel renders <div data-testid="relay-row"> for each relay in
    // snapshot.latestRelayStatuses, which is populated from the FlatBuffers
    // Tier-3 `relay_statuses` vector.  The fixture relay URL was passed as
    // relay_bootstrap so the kernel should report at least one relay row
    // once it has processed the first tick.  If the TS bindings decode is
    // broken, relayStatuses decodes to [] and no relay rows appear.
    // (The kernel may report more than one entry per URL across connection
    // state transitions; assert >= 1, not exactly 1.)
    await expect.poll(
      () => page.locator('[data-testid="relay-row"]').count(),
      { timeout: 30_000 },
    ).toBeGreaterThanOrEqual(1);
  } finally {
    await relay.close();
  }
});
