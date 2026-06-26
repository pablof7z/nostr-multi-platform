// worker.js — the dedicated Web Worker that hosts the OPFS-SQLite conformance run.
//
// This is the load-bearing half of the vehicle: the OPFS SyncAccessHandle pool
// VFS only works inside a dedicated Worker (createSyncAccessHandle does not
// exist on the page main thread), so the wasm engine MUST be instantiated and
// driven from here, not from index.html.
//
// type:"module" so it can `import` the wasm-bindgen `--target web` glue, which
// in turn imports the vendored sqlite3 snippet.
import init, { run_conformance } from "./nmp_sqlite_wasm_conformance.js";

(async () => {
  try {
    await init(); // instantiate the wasm module inside this Worker
    // Resolves with the JSON report on full pass.
    const report = await run_conformance();
    postMessage({ ok: true, report });
  } catch (err) {
    // run_conformance REJECTS with the JSON report when any assertion fails;
    // a panic rejects with an Error. Normalise both to a string the host shows.
    const report = typeof err === "string" ? err : err && err.message ? err.message : String(err);
    postMessage({ ok: false, report });
  }
})();
