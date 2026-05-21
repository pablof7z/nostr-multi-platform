import { DegradedRuntime } from "./degradedRuntime";
import type { WorkerEvent, WorkerRequest } from "./protocol";

const runtime = new DegradedRuntime();
const scope = self as unknown as {
  onmessage: ((message: MessageEvent<WorkerRequest>) => void) | null;
  postMessage: (message: WorkerEvent) => void;
};

scope.onmessage = (message: MessageEvent<WorkerRequest>) => {
  try {
    for (const event of runtime.handle(message.data)) {
      scope.postMessage(event);
    }
  } catch (error) {
    const event: WorkerEvent = {
      type: "error",
      code: "worker_exception",
      message: error instanceof Error ? error.message : "worker failed",
    };
    scope.postMessage(event);
  }
};
