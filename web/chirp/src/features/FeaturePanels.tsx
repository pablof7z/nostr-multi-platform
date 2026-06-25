import { For, Show, createSignal, type JSX } from "solid-js";
import { RefreshCw, Settings } from "lucide-solid";
import { publishProfileCommand, type RuntimeCommand } from "../nmp/actions";
import { shortKey, type FeatureSnapshot } from "../nmp/snapshot";

type PanelProps = { feature: FeatureSnapshot; onCommand: (command: RuntimeCommand) => Promise<void> };

export function SettingsPanel(props: PanelProps & { onStart: () => void }) {
  const [profileName, setProfileName] = createSignal("");
  const [profileAbout, setProfileAbout] = createSignal("");
  return (
    <section class="feature-panel">
      <PanelHeader icon={<Settings size={22} />} title="Settings" subtitle={props.feature.settingsHub.subtitle || "Profile publish and read-only runtime state"} />
      <div class="action-strip">
        <button type="button" onClick={props.onStart}><RefreshCw size={16} /> Start</button>
        <input value={profileName()} placeholder="profile name" onInput={(event) => setProfileName(event.currentTarget.value)} />
        <input value={profileAbout()} placeholder="about" onInput={(event) => setProfileAbout(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(publishProfileCommand({ name: profileName(), about: profileAbout() }))}>Publish profile</button>
      </div>
      <h2>Accounts</h2>
      <Show when={props.feature.accounts.length > 0} fallback={<Empty label="No accounts in snapshot." />}>
        <For each={props.feature.accounts}>{(account) => <article class="list-row"><strong>{account.display || shortKey(account.id)}</strong><span>{account.active ? "active" : account.signer}</span></article>}</For>
      </Show>
      <h2>Outbox</h2>
      <Show when={props.feature.outbox.length > 0} fallback={<Empty label="Publish outbox is empty." />}>
        <For each={props.feature.outbox}>
          {(item) => <article class="list-row"><strong>{item.title || item.handle}</strong><span>{item.statusLabel}</span><p>{item.preview}</p></article>}
        </For>
      </Show>
    </section>
  );
}

function PanelHeader(props: { icon: JSX.Element; title: string; subtitle: string }) {
  return <header class="topbar"><div class="title-row">{props.icon}<div><p class="eyebrow">{props.subtitle}</p><h1>{props.title}</h1></div></div></header>;
}

function Empty(props: { label: string }) {
  return <p class="empty-copy">{props.label}</p>;
}
