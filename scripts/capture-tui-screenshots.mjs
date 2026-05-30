#!/usr/bin/env node
// Capture nmp-gallery-tui component screenshots.
//
// Pipeline (per component):
//   1. Launch the release TUI inside a detached tmux session at a fixed
//      terminal geometry (COLS x ROWS).
//   2. Send key presses to navigate the left component list to the target
//      component index (Down arrow N times from the top).
//   3. Wait WAIT_MS (single wall-clock wait) for the kernel to connect to
//      relays and resolve embeds/profiles — no poll loop.
//   4. `tmux capture-pane -e -p` to grab the ANSI-colored pane.
//   5. Pipe through `ansi2html -i -s` to produce inline-styled HTML on a
//      dark background.
//   6. Playwright (headless Chromium) renders the HTML and screenshots a
//      clipped viewport (CLIP_W x CLIP_H) to PNG.
//
// Usage:
//   node scripts/capture-tui-screenshots.mjs <out_dir> [comp1 comp2 ...]
// With no component args, captures the full registry list.

import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execSync } from 'node:child_process';

// Resolve playwright from the global npm prefix (the package is installed
// globally in this environment, not in a local node_modules).
const GLOBAL_ROOT = execSync('npm root -g', { encoding: 'utf8' }).trim();
const { chromium } = await import('file://' + join(GLOBAL_ROOT, 'playwright', 'index.mjs'));

const BIN = new URL('../target/release/nmp-gallery-tui', import.meta.url).pathname;

// Terminal geometry. The committed previews are 1000x408 — a clipped top
// slice of a taller pane. We render the full pane then clip the viewport.
const COLS = Number(process.env.TUI_COLS || 120);
const ROWS = Number(process.env.TUI_ROWS || 30);
const FONT_PX = Number(process.env.TUI_FONT || 15);   // ansi2html global font size
const LINE_H = Number(process.env.TUI_LH || 1.3);     // pre line-height
const PAD_X = Number(process.env.TUI_PADX || 8);
const PAD_Y = Number(process.env.TUI_PADY || 6);
const WAIT_MS = Number(process.env.TUI_WAIT || 28000); // kernel connect + resolve budget (single wait)
const CLIP_W = 1000;
const CLIP_H = 408;
const SETTLE_MS = 1500;      // redraw settle after navigation key

// Component order in the left list (gallery::COMPONENTS).
const COMPONENTS = [
  'relay-list',
  'user-avatar', 'user-name', 'user-nip05', 'user-npub', 'user-card',
  'content-core', 'content-view', 'content-mention-chip', 'content-minimal',
  'content-media-grid', 'content-quote-card',
  'embed-article', 'embed-profile', 'embed-note', 'embed-highlight',
];

// id -> output filename stem (tui-<stem>-preview.png).
function fileStem(id) { return `tui-${id}`; }

function sh(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { encoding: 'utf8', ...opts });
}

function tmux(args, opts = {}) {
  return sh('tmux', args, opts);
}

async function main() {
  const outDir = process.argv[2];
  if (!outDir) {
    console.error('usage: capture-tui-screenshots.mjs <out_dir> [components...]');
    process.exit(2);
  }
  const targets = process.argv.slice(3);
  const wanted = targets.length ? targets : COMPONENTS;

  const work = mkdtempSync(join(tmpdir(), 'tui-cap-'));
  const session = 'tui_cap_' + process.pid;

  // Kill any stale session.
  spawnSync('tmux', ['kill-session', '-t', session], { stdio: 'ignore' });

  // Start one persistent TUI session; navigate within it so the kernel only
  // cold-starts once and stays warm across components.
  tmux(['new-session', '-d', '-s', session, '-x', String(COLS), '-y', String(ROWS), BIN]);

  // One generous wall-clock wait for first relay connect + resolve.
  console.error(`[capture] booting kernel, waiting ${WAIT_MS}ms for relay resolve...`);
  await sleep(WAIT_MS);

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: CLIP_W, height: CLIP_H } });

  let currentIndex = 0; // TUI starts at content-view? No: starts at index for default component.
  // The binary's default component is "content-view" (index 7) but the
  // selection cursor in interactive mode starts at component_index(default).
  // To be deterministic we Home to the top first.
  tmux(['send-keys', '-t', session, 'Home']);
  await sleep(SETTLE_MS);
  currentIndex = 0;

  for (const id of wanted) {
    const target = COMPONENTS.indexOf(id);
    if (target < 0) { console.error(`[skip] unknown component ${id}`); continue; }

    const delta = target - currentIndex;
    const key = delta >= 0 ? 'Down' : 'Up';
    for (let i = 0; i < Math.abs(delta); i++) {
      tmux(['send-keys', '-t', session, key]);
      await sleep(120);
    }
    currentIndex = target;
    // Embeds (article/note/highlight/profile) carry a referenced event whose
    // byline/projection resolves a beat after the component first renders;
    // give them extra settle so the resolved content is on screen.
    const settle = id.startsWith('embed-') ? SETTLE_MS + 9000 : SETTLE_MS;
    await sleep(settle);

    const ansi = tmux(['capture-pane', '-e', '-p', '-t', session]);
    const ansiPath = join(work, `${id}.ansi`);
    writeFileSync(ansiPath, ansi);

    // ansi2html: inline styles, dark scheme, fixed font.
    const html = sh('ansi2html', ['-i', '-f', `${FONT_PX}px`], { input: ansi });
    const htmlDoc = wrapHtml(html);
    const htmlPath = join(work, `${id}.html`);
    writeFileSync(htmlPath, htmlDoc);

    await page.goto('file://' + htmlPath);
    await page.waitForTimeout(120);
    const outPath = join(outDir, `${fileStem(id)}-preview.png`);
    await page.screenshot({ path: outPath, clip: { x: 0, y: 0, width: CLIP_W, height: CLIP_H } });
    console.error(`[ok] ${id} -> ${outPath}`);
  }

  await browser.close();
  spawnSync('tmux', ['kill-session', '-t', session], { stdio: 'ignore' });
  rmSync(work, { recursive: true, force: true });
}

function wrapHtml(inner) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;background:#000;}
  pre{margin:0;padding:${PAD_Y}px ${PAD_X}px;background:#000;color:#cfcfcf;
      font-family:"Menlo","DejaVu Sans Mono",monospace;line-height:${LINE_H};
      white-space:pre;}
  </style></head><body><pre>${inner}</pre></body></html>`;
}

function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

main().catch((e) => { console.error(e); process.exit(1); });
