// Web Worker entry point for the chirp app.
// Delegates all worker logic to @nmp/runtime-web — no duplication.
import { startNmpWorker } from "@nmp/runtime-web/worker-init";

startNmpWorker();
