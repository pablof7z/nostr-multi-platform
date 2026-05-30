#!/usr/bin/env node
// Build the nmp-gallery verification PDF: matrix criteria + every resolved
// screenshot, grouped by section then platform, each beside its pass/fail
// status. Lets the reviewer judge "it works" against the actual pixels.
//
// Usage: node scripts/build-verification-pdf.mjs
// Output: docs/testing/nmp-gallery-verification-report.pdf
//
// Resolves Playwright from the global npm prefix (no local node_modules).

import { execSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";

const ROOT = execSync("git rev-parse --show-toplevel").toString().trim();
const SHOTS = path.join(ROOT, "web/registry/public/screenshots");
const OUT = path.join(ROOT, "docs/testing/nmp-gallery-verification-report.pdf");

const gitRev = execSync("git rev-parse --short HEAD").toString().trim();

// Platforms shown on the website (Desktop is a diagnostic target, not in the
// registry). file(component) → the screenshot filename for that platform.
const PLATFORMS = [
  { key: "ios", label: "iOS (SwiftUI)", file: (c) => `${c}-ios-gallery-preview.png` },
  { key: "android", label: "Android (Compose)", file: (c) => `${c}-kotlin-preview.png` },
  { key: "tui", label: "TUI (ratatui)", file: (c) => `tui-${c}-preview.png` },
];

// Sections + components + the per-cell pass criterion (what the reviewer must
// see). Mirrors docs/testing/nmp-gallery-verification-matrix.md.
const SECTIONS = [
  {
    label: "User",
    components: [
      ["user-avatar", "Real profile photo (not blank/identicon)"],
      ["user-name", "Display name PABLOF7z (not hex/npub)"],
      ["user-nip05", "Verified NIP-05 badge (e.g. _@f7z.io / ✓f7z.io)"],
      ["user-npub", "npub1l2vyh…utajft chip (Rust-truncated)"],
      ["user-card", "avatar photo + PABLOF7z + nip05"],
    ],
  },
  {
    label: "Relay",
    components: [
      ["relay-list", "purplepag.es + relay.primal.net, role badges + status dots"],
    ],
  },
  {
    label: "Content",
    components: [
      ["content-core", "Wire tree / identicon render"],
      ["content-view", "@PABLOF7z mention (not hex) + note body"],
      ["content-mention-chip", "@PABLOF7z chip resolved (hex reference-fallback chip is intentional)"],
      ["content-minimal", "@PABLOF7z inline flow mention"],
      ["content-media-grid", "real loaded images (not placeholders)"],
      ["content-quote-card", "@PABLOF7z author + note body, variants"],
    ],
  },
  {
    label: "Embeds & Kinds (inline within surrounding note text)",
    components: [
      ["embed-article", "INLINE 'hey, check out my article [card] I hope you enjoy it!'; title 'What's left of the internet?'; author Gigi; hero image"],
      ["embed-profile", "INLINE 'met @PABLOF7z at a nostr conference …'; resolved mention"],
      ["embed-note", "INLINE 'this is a great point [card] what do you think?'; 'grok cli is INSANELY bad, jesus'; author PABLOF7z"],
      ["embed-highlight", "INLINE 'found this interesting [pull-quote]'; 'Vibe-coding is what brought me back to programming'; author PABLOF7z"],
    ],
  },
];

// Known Android gaps (documented, not shipped resolved). component:platform.
const KNOWN_GAPS = {
  "android:content-view": "Android kind:1 note fetch gap",
  "android:content-quote-card": "Android kind:1 note fetch gap",
  "android:embed-note": "Android kind:1 note fetch gap",
  "android:embed-article": "Android kind:30023 naddr fetch gap",
};

function imgDataUri(file) {
  const p = path.join(SHOTS, file);
  if (!existsSync(p)) return null;
  const b64 = readFileSync(p).toString("base64");
  return `data:image/png;base64,${b64}`;
}

let cells = "";
for (const section of SECTIONS) {
  cells += `<h2>${section.label}</h2>`;
  for (const [comp, criterion] of section.components) {
    cells += `<div class="comp"><div class="crit"><b>${comp}</b><br><span>${criterion}</span></div><div class="shots">`;
    for (const pf of PLATFORMS) {
      const gap = KNOWN_GAPS[`${pf.key}:${comp}`];
      const uri = gap ? null : imgDataUri(pf.file(comp));
      cells += `<figure class="${gap ? "gap" : uri ? "ok" : "missing"}">`;
      if (uri) {
        cells += `<img src="${uri}"/>`;
      } else if (gap) {
        cells += `<div class="ph gap">GAP<br><small>${gap}</small></div>`;
      } else {
        cells += `<div class="ph">no screenshot</div>`;
      }
      cells += `<figcaption>${pf.label}${gap ? " ⚠️" : uri ? " ✓" : ""}</figcaption></figure>`;
    }
    cells += `</div></div>`;
  }
}

const html = `<!doctype html><html><head><meta charset="utf-8"><style>
  * { box-sizing: border-box; }
  body { font: 13px -apple-system, system-ui, sans-serif; color:#111; margin:0; padding:24px; }
  h1 { font-size: 22px; margin:0 0 4px; }
  .meta { color:#666; margin-bottom:16px; font-size:12px; }
  .legend { background:#f5f5f7; border-radius:8px; padding:10px 14px; margin-bottom:20px; font-size:12px; line-height:1.5; }
  h2 { font-size:16px; margin:24px 0 10px; padding-bottom:4px; border-bottom:2px solid #ddd; page-break-after:avoid; }
  .comp { display:flex; gap:16px; margin-bottom:18px; page-break-inside:avoid; align-items:flex-start; }
  .crit { width:200px; flex:none; font-size:12px; }
  .crit span { color:#555; }
  .shots { display:flex; gap:12px; flex:1; }
  figure { margin:0; flex:1; max-width:240px; text-align:center; }
  figure img { width:100%; border:1px solid #ccc; border-radius:6px; }
  figcaption { font-size:11px; color:#444; margin-top:4px; }
  .ph { border:1px dashed #bbb; border-radius:6px; height:160px; display:flex; flex-direction:column; align-items:center; justify-content:center; color:#999; font-size:11px; }
  .ph.gap { border-color:#e0a800; background:#fff8e1; color:#8a6d00; }
  figure.gap figcaption { color:#8a6d00; }
</style></head><body>
  <h1>nmp-gallery — cross-platform verification report</h1>
  <div class="meta">master @ ${gitRev} · platforms: iOS (SwiftUI), Android (Compose), TUI · captured on running apps, verified via accessibility tree + pixels</div>
  <div class="legend">
    <b>How to read this:</b> each row is one component; the criterion (left) is what must be visible; the three images are the live render on each platform.
    <b>✓</b> = resolved screenshot captured from the running app. <b>⚠️ GAP</b> = a known, documented bug where that platform fails to resolve — NOT shipped as a fake pass.<br>
    <b>No-hacks rules applied:</b> names show display names (PABLOF7z/Gigi) never raw hex; images actually render (no blank-placeholder hand-waving); embeds render inline within their surrounding note text; loading states are not accepted as final.<br>
    <b>Known gaps:</b> Android content-view / content-quote-card / embed-note / embed-article depend on the kind:1 note (276d69d6…) or kind:30023 article naddr, which fail to fetch on the Android emulator while the same refs resolve on iOS + TUI. Isolated Android fetch gap, under investigation — see docs/testing/nmp-gallery-verification-matrix.md.
  </div>
  ${cells}
</body></html>`;

import { writeFileSync, unlinkSync } from "node:fs";
import { pathToFileURL } from "node:url";

const tmpHtml = path.join(ROOT, "docs/testing/.verification-report.html");
writeFileSync(tmpHtml, html);
const pwPath = execSync("npm root -g").toString().trim();
const pw = await import(pathToFileURL(path.join(pwPath, "playwright", "index.js")).href);
const chromium = pw.chromium ?? pw.default?.chromium;
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(pathToFileURL(tmpHtml).href, { waitUntil: "networkidle" });
await page.pdf({ path: OUT, format: "A4", printBackground: true, margin: { top: "16mm", bottom: "16mm", left: "10mm", right: "10mm" } });
await browser.close();
unlinkSync(tmpHtml);
console.log("wrote " + OUT);
