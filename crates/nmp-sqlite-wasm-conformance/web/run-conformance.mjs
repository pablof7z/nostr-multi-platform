// run-conformance.mjs — headless-browser driver for the OPFS conformance vehicle.
//
// Serves web/pkg/ over http://127.0.0.1 (a secure context for OPFS — localhost
// counts as secure, no TLS needed) and loads index.html in headless Chromium.
// The page spawns the dedicated Worker that runs the engine; this script waits
// for window.__conformance_result, prints each assertion, and exits non-zero on
// any failure. It NEVER fakes a pass: a missing result, a worker error, or a
// failed assertion all exit non-zero.
//
// Usage: node run-conformance.mjs [pkgDir]
//   env PLAYWRIGHT_BROWSER_CHANNEL=chrome  → use system Chrome instead of the
//   Playwright-bundled Chromium (handy locally when only Chrome is installed).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { join, normalize, extname } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const PKG_DIR = process.argv[2] ? normalize(process.argv[2]) : join(HERE, "pkg");
const TIMEOUT_MS = 60_000;

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json; charset=utf-8",
};

const server = createServer(async (req, res) => {
  try {
    const urlPath = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (urlPath === "/favicon.ico") {
      res.writeHead(204).end(); // browsers auto-probe this; don't log a 404
      return;
    }
    const rel = normalize(urlPath).replace(/^(\.\.[/\\])+/, "");
    const file = join(PKG_DIR, rel === "/" || rel === "" ? "index.html" : rel);
    if (!file.startsWith(PKG_DIR)) {
      res.writeHead(403).end("forbidden");
      return;
    }
    const body = await readFile(file);
    res.writeHead(200, { "content-type": MIME[extname(file)] || "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
});

function listen() {
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve(server.address().port));
  });
}

const port = await listen();
const base = `http://127.0.0.1:${port}/`;
console.log(`[harness] serving ${PKG_DIR} at ${base}`);

const channel = process.env.PLAYWRIGHT_BROWSER_CHANNEL || undefined;
const browser = await chromium.launch({ headless: true, channel });
const page = await browser.newPage();
page.on("console", (m) => console.log(`[browser:${m.type()}] ${m.text()}`));
page.on("pageerror", (e) => console.log(`[browser:pageerror] ${e.message}`));

let exitCode = 1;
try {
  await page.goto(base, { waitUntil: "load" });
  await page.waitForFunction(() => window.__conformance_result !== undefined, {
    timeout: TIMEOUT_MS,
  });
  const result = await page.evaluate(() => window.__conformance_result);

  let parsed = null;
  try {
    parsed = JSON.parse(result.report);
  } catch {
    /* report was a plain error string, not JSON */
  }

  if (parsed && Array.isArray(parsed.steps)) {
    for (const s of parsed.steps) {
      console.log(`  ${s.ok ? "PASS" : "FAIL"}  ${s.name} — ${s.detail}`);
    }
  } else {
    console.log(`  report: ${result.report}`);
  }

  if (result.ok && parsed && parsed.passed) {
    console.log("[harness] RESULT: PASS — OPFS-SQLite dedicated-Worker conformance OK");
    exitCode = 0;
  } else {
    console.error("[harness] RESULT: FAIL");
    exitCode = 1;
  }
} catch (err) {
  console.error(`[harness] RESULT: FAIL — ${err && err.message ? err.message : err}`);
  exitCode = 1;
} finally {
  await browser.close();
  await new Promise((r) => server.close(r));
}

process.exit(exitCode);
