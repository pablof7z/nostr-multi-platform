import { For, Show, createSignal, type JSX } from "solid-js";
import { Bolt, MessageSquare, Plus, Radio, RefreshCw, Send, Settings, UserPlus, UsersRound } from "lucide-solid";
import {
  discoverGroupsCommand,
  identityCommand,
  joinGroupCommand,
  openProfileCommand,
  openTagCommand,
  openThreadCommand,
  outboxCommand,
  postGroupMessageCommand,
  publishDmRelayListCommand,
  publishProfileCommand,
  reactGroupMessageCommand,
  relayCommand,
  replyGroupMessageCommand,
  sendDmCommand,
  walletCommand,
  type RuntimeCommand,
} from "../nmp/actions";
import { shortKey, type FeatureSnapshot } from "../nmp/snapshot";

type PanelProps = { feature: FeatureSnapshot; onCommand: (command: RuntimeCommand) => Promise<void> };

export function ChatsPanel(props: PanelProps) {
  const [recipient, setRecipient] = createSignal("");
  const [message, setMessage] = createSignal("");
  const [relays, setRelays] = createSignal("");
  return (
    <section class="feature-panel">
      <PanelHeader icon={<MessageSquare size={22} />} title="Chats" subtitle="NIP-17 inbox and DM relay list" />
      <div class="action-strip">
        <input value={recipient()} placeholder="recipient pubkey" onInput={(event) => setRecipient(event.currentTarget.value)} />
        <input value={message()} placeholder="message" onInput={(event) => setMessage(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(sendDmCommand(recipient(), message()))}><Send size={16} /> Send</button>
      </div>
      <div class="action-strip">
        <input value={relays()} placeholder="wss://relay.one wss://relay.two" onInput={(event) => setRelays(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(publishDmRelayListCommand(words(relays())))}><Radio size={16} /> Publish DM relays</button>
      </div>
      <Show when={props.feature.dmConversations.length > 0} fallback={<Empty label="No DM conversations in the shared snapshot." />}>
        <For each={props.feature.dmConversations}>
          {(conversation) => (
            <article class="list-row">
              <strong>{conversation.peerDisplay || shortKey(conversation.peerPubkey)}</strong>
              <span>{conversation.latest}</span>
              <For each={conversation.messages}>{(message) => <Bubble outgoing={message.outgoing} text={message.content} />}</For>
            </article>
          )}
        </For>
      </Show>
    </section>
  );
}

export function GroupsPanel(props: PanelProps) {
  const [relay, setRelay] = createSignal("");
  const [localId, setLocalId] = createSignal("");
  const [message, setMessage] = createSignal("");
  return (
    <section class="feature-panel">
      <PanelHeader icon={<UsersRound size={22} />} title="Groups" subtitle="NIP-29 discovery, join, and chat projections" />
      <div class="action-strip">
        <input value={relay()} placeholder="host relay" onInput={(event) => setRelay(event.currentTarget.value)} />
        <input value={localId()} placeholder="group id" onInput={(event) => setLocalId(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(discoverGroupsCommand(relay()))}><RefreshCw size={16} /> Discover</button>
        <button type="button" onClick={() => props.onCommand(joinGroupCommand(relay(), localId()))}><UserPlus size={16} /> Join</button>
      </div>
      <div class="action-strip">
        <input value={message()} placeholder="group message" onInput={(event) => setMessage(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(postGroupMessageCommand(relay(), localId(), message()))}><Send size={16} /> Post</button>
      </div>
      <div class="split-grid">
        <section>
          <h2>Discovered</h2>
          <Show when={props.feature.discoveredGroups.length > 0} fallback={<Empty label="No public groups discovered yet." />}>
            <For each={props.feature.discoveredGroups}>
              {(group) => <article class="list-row"><strong>{group.name}</strong><span>{group.memberCount} members · {group.hostRelayUrl}</span><p>{group.about}</p></article>}
            </For>
          </Show>
        </section>
        <section>
          <h2>Chat</h2>
          <Show when={props.feature.groupMessages.length > 0} fallback={<Empty label="No group messages in the snapshot." />}>
            <For each={props.feature.groupMessages}>
              {(item) => (
                <article class="list-row">
                  <Bubble outgoing={item.outgoing} text={item.content} />
                  <button type="button" onClick={() => props.onCommand(reactGroupMessageCommand(relay(), localId(), item.id))}>React</button>
                  <button type="button" onClick={() => props.onCommand(replyGroupMessageCommand(relay(), localId(), item.id, message()))}>Reply</button>
                </article>
              )}
            </For>
          </Show>
        </section>
      </div>
    </section>
  );
}

export function WalletPanel(props: PanelProps) {
  const [uri, setUri] = createSignal("");
  const [invoice, setInvoice] = createSignal("");
  const wallet = () => props.feature.wallet;
  return (
    <section class="feature-panel">
      <PanelHeader icon={<Bolt size={22} />} title="Wallet" subtitle="NWC status, balance, connect, pay, disconnect" />
      <div class="status-banner"><strong>{wallet().status || "disconnected"}</strong><span>{wallet().balanceMsats === undefined ? "balance pending" : `${Math.floor(wallet().balanceMsats! / 1000)} sats`}</span><span>{wallet().walletNpub || wallet().relayUrl}</span></div>
      <div class="action-strip">
        <input value={uri()} placeholder="nostr+walletconnect://..." onInput={(event) => setUri(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(walletCommand("connect", { uri: uri() }))}><Bolt size={16} /> Connect</button>
        <button type="button" onClick={() => props.onCommand(walletCommand("disconnect"))}>Disconnect</button>
      </div>
      <div class="action-strip">
        <input value={invoice()} placeholder="bolt11 invoice" onInput={(event) => setInvoice(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(walletCommand("pay_invoice", { bolt11: invoice() }))}>Pay invoice</button>
      </div>
    </section>
  );
}

export function SettingsPanel(props: PanelProps & { onStart: () => void }) {
  const [accountName, setAccountName] = createSignal("");
  const [secret, setSecret] = createSignal("");
  const [relay, setRelay] = createSignal("");
  const [profileName, setProfileName] = createSignal("");
  const [profileAbout, setProfileAbout] = createSignal("");
  const [lookup, setLookup] = createSignal("");
  return (
    <section class="feature-panel">
      <PanelHeader icon={<Settings size={22} />} title="Settings" subtitle={props.feature.settingsHub.subtitle || "Accounts, relays, outbox, diagnostics"} />
      <div class="action-strip">
        <input value={accountName()} placeholder="account name" onInput={(event) => setAccountName(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(identityCommand("create_account", { name: accountName() }))}><Plus size={16} /> Create</button>
        <button type="button" onClick={() => props.onCommand(identityCommand("nostrconnect", {}))}>Nostr Connect</button>
      </div>
      <div class="action-strip">
        <input value={secret()} placeholder="nsec or bunker URI" onInput={(event) => setSecret(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(identityCommand("import_nsec", { nsec: secret() }))}>Import nsec</button>
        <button type="button" onClick={() => props.onCommand(identityCommand("signin_bunker", { uri: secret() }))}>Bunker</button>
      </div>
      <div class="action-strip">
        <input value={relay()} placeholder="wss://relay.example" onInput={(event) => setRelay(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(relayCommand("add", { url: relay() }))}>Add relay</button>
        <button type="button" onClick={() => props.onCommand(relayCommand("remove", { url: relay() }))}>Remove relay</button>
        <button type="button" onClick={props.onStart}><RefreshCw size={16} /> Start</button>
      </div>
      <div class="action-strip">
        <input value={profileName()} placeholder="profile name" onInput={(event) => setProfileName(event.currentTarget.value)} />
        <input value={profileAbout()} placeholder="about" onInput={(event) => setProfileAbout(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(publishProfileCommand({ name: profileName(), about: profileAbout() }))}>Publish profile</button>
      </div>
      <div class="action-strip">
        <input value={lookup()} placeholder="profile pubkey, event id, or #tag" onInput={(event) => setLookup(event.currentTarget.value)} />
        <button type="button" onClick={() => props.onCommand(openProfileCommand(lookup()))}>Open profile</button>
        <button type="button" onClick={() => props.onCommand(openThreadCommand(lookup()))}>Open thread</button>
        <button type="button" onClick={() => props.onCommand(openTagCommand(lookup().replace(/^#/, "")))}>Open tag</button>
      </div>
      <h2>Accounts</h2>
      <Show when={props.feature.accounts.length > 0} fallback={<Empty label="No accounts in snapshot." />}>
        <For each={props.feature.accounts}>{(account) => <article class="list-row"><strong>{account.display || shortKey(account.id)}</strong><span>{account.active ? "active" : account.signer}</span></article>}</For>
      </Show>
      <h2>Outbox</h2>
      <Show when={props.feature.outbox.length > 0} fallback={<Empty label="Publish outbox is empty." />}>
        <For each={props.feature.outbox}>
          {(item) => <article class="list-row"><strong>{item.title || item.handle}</strong><span>{item.statusLabel}</span><p>{item.preview}</p><button type="button" disabled={!item.canRetry} onClick={() => props.onCommand(outboxCommand("retry", item.handle))}>Retry</button><button type="button" onClick={() => props.onCommand(outboxCommand("cancel", item.handle))}>Cancel</button></article>}
        </For>
      </Show>
    </section>
  );
}

function PanelHeader(props: { icon: JSX.Element; title: string; subtitle: string }) {
  return <header class="topbar"><div class="title-row">{props.icon}<div><p class="eyebrow">{props.subtitle}</p><h1>{props.title}</h1></div></div></header>;
}

function Bubble(props: { outgoing: boolean; text: string }) {
  return <p class={props.outgoing ? "bubble outgoing" : "bubble"}>{props.text}</p>;
}

function Empty(props: { label: string }) {
  return <p class="empty-copy">{props.label}</p>;
}

function words(value: string): string[] {
  return value.split(/\s+/).filter(Boolean);
}
