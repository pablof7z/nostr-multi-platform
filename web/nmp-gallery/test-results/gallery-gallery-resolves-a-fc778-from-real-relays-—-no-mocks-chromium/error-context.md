# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: gallery.spec.ts >> gallery resolves a real profile from real relays — no mocks
- Location: tests/gallery.spec.ts:21:1

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: locator('[data-testid="avatar-row"] img').first()
Expected: visible
Timeout: 45000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" with timeout 45000ms
  - waiting for locator('[data-testid="avatar-row"] img').first()

```

```yaml
- text: kernel running relays 2/2 connected profiles resolved 1
- banner:
  - heading "NMP Component Gallery — Web" [level=1]
  - paragraph:
    - text: Every component below renders the real
    - code: fa984bd7…
    - text: profile resolved live by the NMP kernel (real WASM, real relays). No mocks, no fixtures.
- heading "user-avatar" [level=2]
- paragraph: Reference-first avatar — claims its profile, shows the real picture, falls back to a deterministic identicon.
- heading "user-name" [level=2]
- paragraph: Display-name text from the resolved kind:0.
- text: fa984b…8f52
- heading "user-nip05" [level=2]
- paragraph: NIP-05 verified-identity badge — renders only when the profile carries a nip05.
- heading "user-card" [level=2]
- paragraph: "Compact author header: avatar + name + NIP-05 badge."
- button "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52, profile": fa984b…8f52
```

# Test source

```ts
  1  | /**
  2  |  * Gallery real-data proof. Every assertion below is impossible to pass with a
  3  |  * mock, a fixture, or a degraded runtime — they require the real wasm kernel to
  4  |  * dial real relays, fetch the showcase identity's real kind:0, and surface it
  5  |  * through the resolved_profiles (KRPR) projection that the user-* components
  6  |  * render. A passing run is the thing actually working; the screenshots it emits
  7  |  * are a byproduct.
  8  |  *
  9  |  *   1. kernel status reads "running" (Tier-3 `running` from a real snapshot).
  10 |  *   2. profiles resolved >= 1 (real KRPR decode of a real kind:0).
  11 |  *   3. the user-avatar <img> has a real http(s) src AND the image decoded
  12 |  *      (naturalWidth > 0) — i.e. a real picture loaded from the network.
  13 |  *   4. user-name shows a real display name (non-empty, not the raw-hex fallback).
  14 |  *   5. user-nip05 badge shows a non-empty verified identifier.
  15 |  */
  16 | import { test, expect } from "@playwright/test";
  17 | import { mkdirSync } from "node:fs";
  18 | 
  19 | const SHOTS = new URL("../screenshots/", import.meta.url);
  20 | 
  21 | test("gallery resolves a real profile from real relays — no mocks", async ({ page }) => {
  22 |   page.on("console", (m) => {
  23 |     if (m.type() === "error") console.log(`[browser:error] ${m.text()}`);
  24 |   });
  25 | 
  26 |   await page.goto("/");
  27 | 
  28 |   // 1 — real kernel running.
  29 |   await expect(page.locator('[data-testid="status-bar"]')).toContainText("running", {
  30 |     timeout: 45_000,
  31 |   });
  32 | 
  33 |   // 2 — at least one profile resolved from a real kind:0.
  34 |   await expect
  35 |     .poll(
  36 |       async () => {
  37 |         const txt = (await page.locator('[data-testid="status-bar"]').innerText()).match(
  38 |           /profiles resolved\s+(\d+)/i,
  39 |         );
  40 |         return txt ? Number(txt[1]) : 0;
  41 |       },
  42 |       { timeout: 60_000 },
  43 |     )
  44 |     .toBeGreaterThanOrEqual(1);
  45 | 
  46 |   // 3 — the avatar shows a REAL picture (network-loaded, decoded).
  47 |   const avatarImg = page.locator('[data-testid="avatar-row"] img').first();
> 48 |   await expect(avatarImg).toBeVisible({ timeout: 45_000 });
     |                           ^ Error: expect(locator).toBeVisible() failed
  49 |   const src = await avatarImg.getAttribute("src");
  50 |   expect(src, "avatar src must be a real http(s) URL").toMatch(/^https?:\/\//);
  51 |   await expect
  52 |     .poll(() => avatarImg.evaluate((el: HTMLImageElement) => el.naturalWidth), {
  53 |       timeout: 45_000,
  54 |     })
  55 |     .toBeGreaterThan(0);
  56 | 
  57 |   // 4 — a real display name (not the abcd…wxyz hex fallback).
  58 |   const name = (await page.locator('[data-testid="name-demo"]').innerText()).trim();
  59 |   expect(name.length, "display name must be non-empty").toBeGreaterThan(0);
  60 |   expect(name, "display name must not be the raw-hex fallback").not.toMatch(/^[0-9a-f]{6}…[0-9a-f]{4}$/);
  61 | 
  62 |   // 5 — a real NIP-05 identifier.
  63 |   const nip05 = (await page.locator('[data-testid="nip05-demo"]').innerText()).trim();
  64 |   expect(nip05.length, "nip05 badge must show a non-empty identifier").toBeGreaterThan(0);
  65 | 
  66 |   // Byproduct: per-component screenshots of the resolved state.
  67 |   mkdirSync(SHOTS, { recursive: true });
  68 |   const shot = (sel: string, name: string) =>
  69 |     page.locator(sel).screenshot({ path: new URL(name, SHOTS).pathname });
  70 | 
  71 |   // Ensure all demos resolved (no "resolving…" placeholders) before shooting.
  72 |   await expect(page.locator('[data-testid="resolving"]')).toHaveCount(0);
  73 | 
  74 |   await page.screenshot({ path: new URL("gallery-full.png", SHOTS).pathname, fullPage: true });
  75 |   await shot("#user-avatar .component-stage", "web-user-avatar.png");
  76 |   await shot("#user-name .component-stage", "web-user-name.png");
  77 |   await shot("#user-nip05 .component-stage", "web-user-nip05.png");
  78 |   await shot("#user-card .component-stage", "web-user-card.png");
  79 | 
  80 |   console.log(`[proof] name="${name}" nip05="${nip05}" avatar="${src}"`);
  81 | });
  82 | 
```