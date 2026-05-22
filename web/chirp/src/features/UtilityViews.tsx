import { For, createSignal } from "solid-js";
import { Bell, MessageSquare, Search, Send, Settings, Wallet } from "lucide-solid";
import { saveRelays, search as dispatchSearch, sendDirectMessage } from "../chirp/actions";
import { showcaseProfiles, timelineItems } from "../chirp/model";
import type { NmpClient } from "../nmp/client";
import { Avatar } from "../components/Avatar";
import { EntityContent } from "../components/EntityContent";
import { Topbar } from "./Topbar";

export function MentionsView(props: { onEntity: (entity: string) => void }) {
  return (
    <>
      <Topbar eyebrow="Mentions" title="Replies and reactions" />
      <section class="empty-state">
        <Bell size={24} />
        <h2>No mention snapshot yet</h2>
        <p>When the wasm actor publishes notification updates, this view renders them directly.</p>
      </section>
      <section class="stack-list">
        <For each={timelineItems.slice(0, 1)}>
          {(item) => <EntityContent content={`Preview mention: ${item.content}`} onOpen={props.onEntity} />}
        </For>
      </section>
    </>
  );
}

export function SearchView(props: { client: NmpClient; onProfile: (pubkey: string) => void }) {
  const [query, setQuery] = createSignal("");
  return (
    <>
      <Topbar eyebrow="Search" title="People, notes, and groups" />
      <section class="search-box">
        <input
          value={query()}
          placeholder="Search Nostr"
          onInput={(event) => setQuery(event.currentTarget.value)}
        />
        <button type="button" onClick={() => dispatchSearch(props.client, query())}>
          <Search size={17} />
          Search
        </button>
      </section>
      <section class="stack-list">
        <For each={showcaseProfiles}>
          {(profile) => (
            <button class="person-row" type="button" onClick={() => props.onProfile(profile.pubkey)}>
              <Avatar profile={profile} className="mini-avatar" />
              <span>
                <strong>{profile.name}</strong>
                <small>@{profile.handle}</small>
              </span>
            </button>
          )}
        </For>
      </section>
    </>
  );
}

export function MessagesView(props: { client: NmpClient; onProfile: (pubkey: string) => void }) {
  const [recipient, setRecipient] = createSignal(showcaseProfiles[1]);
  const [draft, setDraft] = createSignal("");
  const send = async () => {
    if (!draft().trim()) {
      return;
    }
    await sendDirectMessage(props.client, recipient().pubkey, draft());
    setDraft("");
  };

  return (
    <>
      <Topbar eyebrow="Messages" title="NIP-17 direct messages" />
      <section class="messages-layout">
        <div class="conversation-list">
          <For each={showcaseProfiles.slice(1)}>
            {(profile) => (
              <button
                class={recipient().pubkey === profile.pubkey ? "person-row active" : "person-row"}
                type="button"
                onClick={() => setRecipient(profile)}
              >
                <Avatar profile={profile} className="mini-avatar" />
                <span>
                  <strong>{profile.name}</strong>
                  <small>@{profile.handle}</small>
                </span>
              </button>
            )}
          </For>
        </div>
        <div class="dm-thread">
          <button class="thread-title" type="button" onClick={() => props.onProfile(recipient().pubkey)}>
            <Avatar profile={recipient()} className="mini-avatar" />
            <span>
              <strong>{recipient().name}</strong>
              <small>NIP-17 session</small>
            </span>
          </button>
          <div class="empty-state compact">
            <MessageSquare size={22} />
            <h2>Encrypted DM snapshot pending</h2>
            <p>The browser sends typed capability results; Rust owns decrypt, routing, and retry policy.</p>
          </div>
          <div class="dm-compose">
            <input
              value={draft()}
              placeholder={`Message ${recipient().name}`}
              onInput={(event) => setDraft(event.currentTarget.value)}
            />
            <button type="button" onClick={send} disabled={draft().trim().length === 0}>
              <Send size={17} />
              Send
            </button>
          </div>
        </div>
      </section>
    </>
  );
}

export function WalletView() {
  return (
    <>
      <Topbar eyebrow="Wallet" title="NWC wallet" />
      <section class="empty-state">
        <Wallet size={24} />
        <h2>Wallet capability not connected</h2>
        <p>NWC connect, balance, and zap flows belong behind typed browser capabilities.</p>
      </section>
    </>
  );
}

export function SettingsView(props: { client: NmpClient; relays: string[] }) {
  const [relayText, setRelayText] = createSignal(props.relays.join("\n"));
  return (
    <>
      <Topbar eyebrow="Settings" title="Account and relays" />
      <section class="settings-grid">
        <div class="settings-card">
          <h2>
            <Settings size={17} />
            Relays
          </h2>
          <textarea value={relayText()} onInput={(event) => setRelayText(event.currentTarget.value)} />
          <button
            type="button"
            onClick={() => saveRelays(props.client, relayText().split("\n").map((line) => line.trim()).filter(Boolean))}
          >
            Save relay rows
          </button>
        </div>
        <div class="settings-card">
          <h2>Runtime contract</h2>
          <p>Web renders state, executes browser capabilities, and dispatches typed actions.</p>
          <p>Rust owns identity, routing, follow policy, group membership, and publish decisions.</p>
        </div>
      </section>
    </>
  );
}
