// worker.js — the dedicated Web Worker that hosts the OPFS durability run.
//
// The OPFS SyncAccessHandle pool VFS (which the durable store is built on) only
// works inside a dedicated Worker — `createSyncAccessHandle()` does not exist on
// the page main thread. So the wasm runtime MUST be instantiated and driven from
// here, not from index.html.
//
// type:"module" so it can `import` the wasm-bindgen `--target web` glue, which in
// turn imports the vendored sqlite3 snippet.
import init, { run_conformance } from "./nmp_browser_runtime_conformance.js";

(async () => {
  try {
    await init(); // instantiate the wasm module inside this Worker
    // Resolves with the JSON report on full pass.
    const report = await run_conformance();
    postMessage({ ok: true, report });
  } catch (err) {
    // run_conformance REJECTS with the JSON report when any assertion fails; a
    // panic rejects with an Error. Normalise both to a string the host shows.
    const report = typeof err === "string" ? err : err && err.message ? err.message : String(err);
    postMessage({ ok: false, report });
  }
})();
