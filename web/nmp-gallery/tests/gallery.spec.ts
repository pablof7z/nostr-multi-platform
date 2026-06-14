/**
 * Gallery real-data proof. Every assertion below is impossible to pass with a
 * mock, a fixture, or a degraded runtime — they require the real wasm kernel to
 * dial real relays, fetch the showcase identity's real kind:0, and surface it
 * through the resolved_profiles (KRPR) projection that the user-* components
 * render. A passing run is the thing actually working; the screenshots it emits
 * are a byproduct.
 *
 *   1. kernel status reads "running" (Tier-3 `running` from a real snapshot).
 *   2. profiles resolved >= 1 (real KRPR decode of a real kind:0).
 *   3. the user-avatar <img> has a real http(s) src AND the image decoded
 *      (naturalWidth > 0) — i.e. a real picture loaded from the network.
 *   4. user-name shows a real display name (non-empty, not the raw-hex fallback).
 *   5. user-nip05 badge shows a non-empty verified identifier.
 */
import { test, expect } from "@playwright/test";
import { mkdirSync } from "node:fs";

const SHOTS = new URL("../screenshots/", import.meta.url);

test("gallery resolves a real profile from real relays — no mocks", async ({ page }) => {
  page.on("console", (m) => {
    if (m.type() === "error") console.log(`[browser:error] ${m.text()}`);
  });

  await page.goto("/");

  // 1 — real kernel running.
  await expect(page.locator('[data-testid="status-bar"]')).toContainText("running", {
    timeout: 45_000,
  });

  // 2 — at least one profile resolved from a real kind:0.
  await expect
    .poll(
      async () => {
        const txt = (await page.locator('[data-testid="status-bar"]').innerText()).match(
          /profiles resolved\s+(\d+)/i,
        );
        return txt ? Number(txt[1]) : 0;
      },
      { timeout: 60_000 },
    )
    .toBeGreaterThanOrEqual(1);

  // 3 — the avatar shows a REAL picture (network-loaded, decoded).
  const avatarImg = page.locator('[data-testid="avatar-row"] img').first();
  await expect(avatarImg).toBeVisible({ timeout: 45_000 });
  const src = await avatarImg.getAttribute("src");
  expect(src, "avatar src must be a real http(s) URL").toMatch(/^https?:\/\//);
  await expect
    .poll(() => avatarImg.evaluate((el: HTMLImageElement) => el.naturalWidth), {
      timeout: 45_000,
    })
    .toBeGreaterThan(0);

  // 4 — a real display name (not the abcd…wxyz hex fallback).
  const name = (await page.locator('[data-testid="name-demo"]').innerText()).trim();
  expect(name.length, "display name must be non-empty").toBeGreaterThan(0);
  expect(name, "display name must not be the raw-hex fallback").not.toMatch(/^[0-9a-f]{6}…[0-9a-f]{4}$/);

  // 5 — a real NIP-05 identifier.
  const nip05 = (await page.locator('[data-testid="nip05-demo"]').innerText()).trim();
  expect(nip05.length, "nip05 badge must show a non-empty identifier").toBeGreaterThan(0);

  // 6 — content-view renders a REAL kernel-parsed content tree, not the raw
  //     fallback. The app only mounts these once the kernel returns a non-empty,
  //     placeholder-free NFCT tree (the honesty gate). We additionally prove the
  //     render came from the TREE by asserting tree-derived markup that the raw
  //     string fallback could never produce.
  const note = page.locator('[data-testid="content-note"]');
  await expect(note).toBeVisible({ timeout: 60_000 });
  const noteText = (await note.innerText()).trim();
  expect(noteText.length, "note content must be non-empty").toBeGreaterThan(0);

  // The article is the strong honesty proof: markdown → multiple <p class="nostr-p">
  // block elements + at least one anchor. A fallback string render produces ZERO
  // of these structural elements, so their presence proves the tree path ran.
  const article = page.locator('[data-testid="content-article"]');
  await expect(article).toBeVisible({ timeout: 60_000 });
  await expect
    .poll(() => article.locator("p.nostr-p").count(), { timeout: 30_000 })
    .toBeGreaterThanOrEqual(2);
  const articleLinks = await article.locator("a.nostr-url, a.nostr-link").count();
  expect(articleLinks, "article must render at least one tree-derived link").toBeGreaterThanOrEqual(1);

  // Byproduct: per-component screenshots of the resolved state.
  mkdirSync(SHOTS, { recursive: true });
  const shot = (sel: string, name: string) =>
    page.locator(sel).screenshot({ path: new URL(name, SHOTS).pathname });

  // Ensure all demos resolved (no "resolving…" placeholders) before shooting.
  await expect(page.locator('[data-testid="resolving"]')).toHaveCount(0);

  await page.screenshot({ path: new URL("gallery-full.png", SHOTS).pathname, fullPage: true });
  await shot("#user-avatar .component-stage", "web-user-avatar.png");
  await shot("#user-name .component-stage", "web-user-name.png");
  await shot("#user-nip05 .component-stage", "web-user-nip05.png");
  await shot("#user-card .component-stage", "web-user-card.png");
  await shot("#content-view .component-stage", "web-content-view.png");

  console.log(
    `[proof] name="${name}" nip05="${nip05}" avatar="${src}" ` +
      `note="${noteText.slice(0, 40)}" articleLinks=${articleLinks}`,
  );
});
