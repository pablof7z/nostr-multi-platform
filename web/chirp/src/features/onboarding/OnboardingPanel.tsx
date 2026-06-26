import { For, Show } from "solid-js";
import type { RuntimeProjection } from "../../nmp/runtimeProjection";
import "./onboarding.css";

export type OnboardingState = {
  runtimeConnected: boolean;
  signerConnected: boolean;
  feedReady: boolean;
  feedCount: number;
  runtimeMode: "worker" | "in_process_fallback";
  diagnostics?: RuntimeProjection;
};

type Step = {
  label: string;
  status: "done" | "active" | "blocked";
  detail: string;
};

function buildSteps(state: OnboardingState): Step[] {
  const relayCount = state.diagnostics?.relays.length ?? 0;
  const connectedRelays =
    state.diagnostics?.relays.filter((relay) => relay.connection === "connected").length ?? 0;

  return [
    {
      label: "Runtime",
      status: state.runtimeConnected ? "done" : "active",
      detail:
        state.runtimeMode === "worker"
          ? "WASM worker is running."
          : "Browser runtime is degraded; publishing is unavailable.",
    },
    {
      label: "Relays",
      status: connectedRelays > 0 ? "done" : relayCount > 0 ? "active" : "blocked",
      detail:
        relayCount > 0
          ? `${connectedRelays}/${relayCount} configured relays connected.`
          : "No configured relay inventory has reached the runtime.",
    },
    {
      label: "Identity",
      status: state.signerConnected ? "done" : "active",
      detail: state.signerConnected
        ? "Signer is connected; write actions can request signatures."
        : "Connect a NIP-07 signer to publish from this browser.",
    },
    {
      label: "Feed",
      status: state.feedReady && state.feedCount > 0 ? "done" : state.feedReady ? "active" : "blocked",
      detail: state.feedReady
        ? `${state.feedCount} notes decoded from the runtime feed projection.`
        : "Waiting for the first feed projection snapshot.",
    },
  ];
}

function nextAction(steps: Step[]): string {
  const blocked = steps.find((step) => step.status !== "done");
  if (!blocked) return "Live: read, compose, react, inspect relay state.";
  if (blocked.label === "Identity") return "Connect a signer to unlock posting and reactions.";
  if (blocked.label === "Relays") return "Waiting for relay sockets to connect.";
  if (blocked.label === "Feed") return "Waiting for followed notes to hydrate.";
  return "Starting the WASM runtime.";
}

export function OnboardingPanel(props: { state: OnboardingState }) {
  const steps = () => buildSteps(props.state);
  const complete = () => steps().every((step) => step.status === "done");
  const completeCount = () => steps().filter((step) => step.status === "done").length;

  return (
    <section class="onboarding-panel" aria-label="Onboarding">
      <div class="onboarding-header">
        <div>
          <p class="panel-kicker">First run</p>
          <h2>{complete() ? "Ready for signed Chirps" : "Set up Chirp"}</h2>
        </div>
        <span class="onboarding-progress">{completeCount()}/4</span>
      </div>
      <div class="onboarding-next" data-complete={complete() ? "true" : "false"}>
        <strong>{complete() ? "Session ready" : "Next"}</strong>
        <span>{nextAction(steps())}</span>
      </div>
      <ol class="onboarding-steps">
        <For each={steps()}>
          {(step) => (
            <li class="onboarding-step" data-status={step.status}>
              <span class="step-index" aria-hidden="true" />
              <div>
                <strong>{step.label}</strong>
                <span>{step.detail}</span>
              </div>
            </li>
          )}
        </For>
      </ol>
      <Show when={props.state.diagnostics?.lastErrorToast}>
        {(error) => (
          <p class="onboarding-error" role="alert">
            {error()}
          </p>
        )}
      </Show>
    </section>
  );
}
