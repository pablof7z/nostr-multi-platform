// Public NMP client factory for the Chirp web shell.
//
// Domain state and protocol decisions stay in Rust; this file only selects the
// worker bridge or the degraded in-process bridge when Worker construction is
// unavailable.

import { InProcessNmpClient } from "./inProcessClient";
import { WorkerNmpClient } from "./workerClient";
import type { NmpClient } from "./clientTypes";

export { makeCorrelationId } from "./correlationId";
export type { NmpClient, RuntimeConnection, RuntimeSnapshot, SearchOpenRequest } from "./clientTypes";

export function createNmpClient(): NmpClient {
  if (typeof Worker === "undefined") {
    console.warn(
      "[nmp] Web Worker API is unavailable (SSR, CSP, or browser restriction). " +
        "Falling back to in-process degraded runtime — every action will return capability_failure.",
    );
    return new InProcessNmpClient();
  }
  try {
    return new WorkerNmpClient();
  } catch (err) {
    console.warn(
      "[nmp] Worker construction failed — falling back to in-process degraded runtime. " +
        "Every action will return capability_failure. Worker error:",
      err,
    );
    return new InProcessNmpClient();
  }
}
