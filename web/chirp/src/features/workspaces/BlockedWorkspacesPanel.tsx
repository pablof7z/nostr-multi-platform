import { For, Show, createMemo, createSignal } from "solid-js";
import { blockedWorkspaceCommand } from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";
import "./workspaces.css";

type WorkspaceStatus = "blocked" | "partial";

type Workspace = {
  id: string;
  title: string;
  capability: string;
  status: WorkspaceStatus;
  reason: string;
  proof: string;
};

const WORKSPACES: Workspace[] = [
  {
    id: "wallet",
    title: "Wallet and zaps",
    capability: "nmp.nip57.wallet",
    status: "blocked",
    reason: "Wallet connection, zap request, payment, and receipt state are not available on web.",
    proof: "NWC and zap diagnostics must come from shared value-flow crates, not UI state.",
  },
  {
    id: "moderation",
    title: "Trust and moderation",
    capability: "nmp.trust_controls",
    status: "blocked",
    reason: "Mute, block, relay, WoT, and hidden-content policy are not projected for web yet.",
    proof: "Filtering policy must be replayable Rust state with visible provenance.",
  },
];

export function BlockedWorkspacesPanel(props: { signedIn: boolean; focus?: string }) {
  const { client } = useNmpClient();
  const [lastCapability, setLastCapability] = createSignal<string | null>(null);
  const [busyCapability, setBusyCapability] = createSignal<string | null>(null);
  const visibleWorkspaces = createMemo(() => {
    const focus = props.focus;
    if (!focus) return WORKSPACES;
    return [...WORKSPACES].sort((a, b) => (a.id === focus ? -1 : b.id === focus ? 1 : 0));
  });
  const inspect = async (workspace: Workspace) => {
    if (busyCapability()) return;
    setBusyCapability(workspace.capability);
    try {
      await client.dispatchCommand(blockedWorkspaceCommand(workspace.capability));
      setLastCapability(workspace.capability);
    } finally {
      setBusyCapability(null);
    }
  };

  return (
    <section class="workspaces-panel" aria-label="More Chirp workspaces" data-testid="workspaces-panel">
      <div class="workspaces-header">
        <div>
          <p class="panel-kicker">More</p>
          <h2>{props.focus ? "Blocked destination" : "Blocked product areas"}</h2>
        </div>
        <span class="workspace-session" data-signed-in={props.signedIn ? "true" : "false"}>
          {props.signedIn ? "signed session" : "read mode"}
        </span>
      </div>

      <div class="workspace-grid">
        <For each={visibleWorkspaces()}>
          {(workspace) => (
            <article
              class="workspace-row"
              data-status={workspace.status}
              data-focused={workspace.id === props.focus ? "true" : "false"}
              data-testid={`workspace-${workspace.id}`}
            >
              <div class="workspace-badge" aria-hidden="true">
                {workspace.title.slice(0, 1)}
              </div>
              <div class="workspace-copy">
                <div class="workspace-title-row">
                  <strong>{workspace.title}</strong>
                  <span>{workspace.status}</span>
                </div>
                <p>{workspace.reason}</p>
                <small>{workspace.proof}</small>
              </div>
              <button
                type="button"
                class="workspace-inspect"
                data-testid={`inspect-${workspace.id}`}
                disabled={busyCapability() !== null}
                onClick={() => void inspect(workspace)}
              >
                {busyCapability() === workspace.capability ? "Checking" : "Inspect"}
              </button>
            </article>
          )}
        </For>
      </div>

      <Show when={lastCapability()}>
        {(capability) => (
          <p class="workspace-diagnostic" role="status" data-testid="workspace-diagnostic">
            Recorded diagnostic for <code>{capability()}</code>.
          </p>
        )}
      </Show>
    </section>
  );
}
