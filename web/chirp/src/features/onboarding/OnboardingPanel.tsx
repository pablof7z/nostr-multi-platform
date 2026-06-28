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

type Proof = {
  label: string;
  value: string;
  tone: "good" | "warn" | "muted";
};

type JourneyAction = {
  label: string;
  href: string;
  state: "ready" | "locked";
};

function hasAcceptedRelayReceipt(result: string | undefined): boolean {
  if (!result) return false;
  try {
    const parsed: unknown = JSON.parse(result);
    if (typeof parsed !== "object" || parsed === null) return false;
    const record = parsed as Record<string, unknown>;
    if (record.kind !== "publish_relay_receipt" || !Array.isArray(record.relays)) return false;
    return record.relays.some((relay) => {
      if (typeof relay !== "object" || relay === null) return false;
      const status = (relay as Record<string, unknown>).status;
      return status === "ok" || status === "accepted";
    });
  } catch {
    return false;
  }
}

function buildSteps(state: OnboardingState): Step[] {
  const relayCount = state.diagnostics?.relays.length ?? 0;
  const connectedRelays =
    state.diagnostics?.relays.filter((relay) => relay.connection === "connected").length ?? 0;
  const acceptedPublishes =
    state.diagnostics?.actionResults.filter((result) => hasAcceptedRelayReceipt(result.result))
      .length ?? 0;

  return [
    {
      label: "Start runtime",
      status: state.runtimeConnected ? "done" : "active",
      detail: state.runtimeConnected
        ? "WASM worker is running and producing snapshots."
        : state.runtimeMode === "worker"
          ? "Waiting for the first WASM worker snapshot."
          : "Browser runtime is degraded; publishing stays unavailable.",
    },
    {
      label: "Read signed out",
      status:
        connectedRelays > 0 && state.feedReady
          ? "done"
          : relayCount > 0 || state.feedReady
            ? "active"
            : "blocked",
      detail:
        connectedRelays > 0
          ? `${connectedRelays}/${relayCount} relays connected; ${state.feedCount} feed notes decoded.`
          : "Waiting for relay inventory and the first feed projection.",
    },
    {
      label: "Connect identity",
      status: state.signerConnected ? "done" : "active",
      detail: state.signerConnected
        ? "Write actions can now request signatures."
        : "Use NIP-07 for a normal account or a session nsec for local testing.",
    },
    {
      label: "Prove publish path",
      status: acceptedPublishes > 0 ? "done" : state.signerConnected && state.feedReady ? "active" : "blocked",
      detail: acceptedPublishes > 0
        ? `${acceptedPublishes} signed action accepted by a relay.`
        : state.signerConnected
        ? "Publish, react, follow, or save and inspect per-relay verdicts."
        : "Connect a signer before testing signed social actions.",
    },
  ];
}

function buildProofs(state: OnboardingState): Proof[] {
  const relayCount = state.diagnostics?.relays.length ?? 0;
  const connectedRelays =
    state.diagnostics?.relays.filter((relay) => relay.connection === "connected").length ?? 0;
  const acceptedPublishes =
    state.diagnostics?.actionResults.filter((result) => hasAcceptedRelayReceipt(result.result))
      .length ?? 0;

  return [
    {
      label: "Runtime",
      value: state.runtimeMode === "worker" ? "WASM worker" : "degraded",
      tone: state.runtimeConnected && state.runtimeMode === "worker" ? "good" : "warn",
    },
    {
      label: "Relays",
      value: relayCount > 0 ? `${connectedRelays}/${relayCount}` : "pending",
      tone: connectedRelays > 0 ? "good" : "warn",
    },
    {
      label: "Feed",
      value: state.feedReady ? `${state.feedCount} notes` : "pending",
      tone: state.feedReady && state.feedCount > 0 ? "good" : "warn",
    },
    {
      label: "Proof",
      value: acceptedPublishes > 0 ? `${acceptedPublishes} accepted` : "none yet",
      tone: acceptedPublishes > 0 ? "good" : "muted",
    },
  ];
}

function buildJourneyActions(state: OnboardingState): JourneyAction[] {
  return [
    {
      label: "Read feed",
      href: "#feed",
      state: state.runtimeConnected && state.feedReady ? "ready" : "locked",
    },
    {
      label: "Find people",
      href: "#search",
      state: state.runtimeConnected ? "ready" : "locked",
    },
    {
      label: "DMs",
      href: "#messages",
      state: state.signerConnected ? "ready" : "locked",
    },
    {
      label: "Inspect proof",
      href: "#diagnostics",
      state: state.runtimeConnected ? "ready" : "locked",
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
  if (blocked.label === "Connect identity") {
    return {
      label: "Connect identity",
      detail: "Choose NIP-07 or paste a session-only nsec to unlock write actions.",
      href: "#signing",
    };
  }
  if (blocked.label === "Read signed out") {
    return {
      label: "Check relays",
      detail: "Wait for a relay socket or adjust the configured relay set.",
      href: "#relays",
    };
  }
  if (blocked.label === "Prove publish path") {
    return {
      label: "Try an action",
      detail: "Post, react, follow, or save a note, then inspect relay acceptance.",
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
  const proofs = () => buildProofs(props.state);
  const journeyActions = () => buildJourneyActions(props.state);
  const complete = () => steps().every((step) => step.status === "done");
  const completeCount = () => steps().filter((step) => step.status === "done").length;
  const action = () => nextAction(steps());

  return (
    <section class="onboarding-panel" aria-label="First-run onboarding" data-testid="onboarding-panel">
      <div class="onboarding-header">
        <div>
          <p class="panel-kicker">First run</p>
          <h2>{complete() ? "Chirp is ready" : "Get to a real session"}</h2>
          <p>
            Read from relays first, connect an identity when you are ready to
            write, then confirm the signed path through relay verdicts.
          </p>
        </div>
        <span class="onboarding-progress">{completeCount()}/4</span>
      </div>
      <div class="onboarding-meter" aria-hidden="true">
        <span style={{ width: `${(completeCount() / 4) * 100}%` }} />
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
      <div class="onboarding-actions" aria-label="First-run workspaces">
        <For each={journeyActions()}>
          {(item) => (
            <a href={item.href} data-state={item.state}>
              <span>{item.label}</span>
              <strong>{item.state === "ready" ? "Ready" : "Locked"}</strong>
            </a>
          )}
        </For>
      </div>
      <div class="onboarding-proof-grid" aria-label="Session proof">
        <For each={proofs()}>
          {(proof) => (
            <div class="onboarding-proof" data-tone={proof.tone}>
              <span>{proof.label}</span>
              <strong>{proof.value}</strong>
            </div>
          )}
        </For>
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
