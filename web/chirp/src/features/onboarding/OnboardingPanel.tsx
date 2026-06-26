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

type NextAction = {
  label: string;
  detail: string;
  href: string;
};

function buildSteps(state: OnboardingState): Step[] {
  const relayCount = state.diagnostics?.relays.length ?? 0;
  const connectedRelays =
    state.diagnostics?.relays.filter((relay) => relay.connection === "connected").length ?? 0;

  return [
    {
      label: "Runtime",
      status: state.runtimeConnected ? "done" : "active",
      detail: state.runtimeConnected
        ? "WASM worker is running."
        : state.runtimeMode === "worker"
          ? "Waiting for the first WASM worker snapshot."
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
        : "Connect NIP-07 or use a memory-only local key to publish.",
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

function nextAction(steps: Step[]): NextAction {
  const blocked = steps.find((step) => step.status !== "done");
  if (!blocked) {
    return {
      label: "Start chirping",
      detail: "Read the feed, publish, react, and inspect relay acceptance.",
      href: "#feed",
    };
  }
  if (blocked.label === "Identity") {
    return {
      label: "Connect identity",
      detail: "Choose NIP-07 or paste a session-only nsec to unlock write actions.",
      href: "#signing",
    };
  }
  if (blocked.label === "Relays") {
    return {
      label: "Check relays",
      detail: "Wait for a relay socket or adjust the configured relay set.",
      href: "#relays",
    };
  }
  if (blocked.label === "Feed") {
    return {
      label: "Open feed",
      detail: "The runtime is connected; waiting for followed notes to hydrate.",
      href: "#feed",
    };
  }
  return {
    label: "Inspect diagnostics",
    detail: "Waiting for the WASM worker to emit its first snapshot.",
    href: "#diagnostics",
  };
}

export function OnboardingPanel(props: { state: OnboardingState }) {
  const steps = () => buildSteps(props.state);
  const complete = () => steps().every((step) => step.status === "done");
  const completeCount = () => steps().filter((step) => step.status === "done").length;
  const action = () => nextAction(steps());

  return (
    <section class="onboarding-panel" aria-label="First-run onboarding">
      <div class="onboarding-header">
        <div>
          <p class="panel-kicker">First run</p>
          <h2>{complete() ? "Ready for signed Chirps" : "Set up Chirp"}</h2>
          <p>
            Bring a relay-backed timeline online, connect an identity, then send
            a signed action with visible relay proof.
          </p>
        </div>
        <span class="onboarding-progress">{completeCount()}/4</span>
      </div>
      <div class="onboarding-next" data-complete={complete() ? "true" : "false"}>
        <div>
          <strong>{complete() ? "Session ready" : "Next step"}</strong>
          <span>{action().detail}</span>
        </div>
        <a class="onboarding-action" href={action().href}>
          {action().label}
        </a>
      </div>
      <ol class="onboarding-steps">
        <For each={steps()}>
          {(step, index) => (
            <li class="onboarding-step" data-status={step.status}>
              <span class="step-index" aria-hidden="true">
                {index() + 1}
              </span>
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
