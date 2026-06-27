import { For, Show, createMemo, createSignal } from "solid-js";
import { blockedWorkspaceCommand } from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";
import {
  decodeDmInboxFrame,
  type DmConversationItem,
  type DmMessageItem,
} from "../../nmp/dmInboxDecoder";
import { decodeRuntimeProjection } from "../../nmp/runtimeProjection";
import "./messages.css";

const SEND_CAPABILITY = "nmp.nip17.send";

export function MessagesPanel() {
  const { client, snapshot } = useNmpClient();
  const [selectedPeer, setSelectedPeer] = createSignal<string | null>(null);
  const [lastCapability, setLastCapability] = createSignal<string | null>(null);
  const [busyCapability, setBusyCapability] = createSignal<string | null>(null);
  const runtime = createMemo(() => decodeRuntimeProjection(snapshot().latestUpdateBytes));
  const activeAccount = createMemo(() => runtime()?.activeAccountPubkey);
  const inbox = createMemo(() => decodeDmInboxFrame(snapshot().latestUpdateBytes));
  const conversations = createMemo(() => inbox()?.conversations ?? []);
  const selected = createMemo(() => {
    const rows = conversations();
    if (rows.length === 0) return undefined;
    const peer = selectedPeer();
    return rows.find((row) => row.peerPubkey === peer) ?? rows[0];
  });
  const messageCount = createMemo(() =>
    conversations().reduce((total, row) => total + row.messages.length, 0),
  );
  const decryptState = createMemo(() => inbox()?.decryptState ?? "unavailable");
  const inspectSend = async () => {
    if (busyCapability()) return;
    setBusyCapability(SEND_CAPABILITY);
    try {
      await client.dispatchCommand(blockedWorkspaceCommand(SEND_CAPABILITY));
      setLastCapability(SEND_CAPABILITY);
    } finally {
      setBusyCapability(null);
    }
  };

  return (
    <section class="messages-panel" aria-label="Private messages" data-testid="messages-panel">
      <div class="messages-header">
        <div>
          <p class="panel-kicker">NIP-17</p>
          <h2>Private messages</h2>
        </div>
        <span class="messages-source" data-testid="messages-source">
          {activeAccount()
            ? `${conversations().length} threads / ${messageCount()} messages`
            : "sign in required"}
        </span>
      </div>

      <div class="messages-actions" aria-label="Messages workspace status">
        <span data-state={activeAccount() ? "live" : "pending"}>
          {activeAccount() ? "live gift-wrap inbox" : "waiting for identity"}
        </span>
        <span data-state={decryptState() === "ok" ? "live" : decryptState()}>
          decrypt {decryptState()}
        </span>
        <Show when={(inbox()?.undecryptedCount ?? 0) > 0}>
          <span data-state="limited">{inbox()?.undecryptedCount} pending decrypt</span>
        </Show>
        <button
          type="button"
          class="messages-send-diagnostic"
          data-testid="messages-send-diagnostic"
          disabled={busyCapability() !== null}
          onClick={() => void inspectSend()}
        >
          Inspect send
        </button>
      </div>

      <Show
        when={activeAccount()}
        fallback={
          <div class="messages-empty" data-testid="messages-signed-out">
            <strong>Connect a signer</strong>
            <span>NIP-17 gift-wrap decrypt opens only for the active Rust-owned account.</span>
          </div>
        }
      >
        <Show
          when={conversations().length > 0}
          fallback={
            <div class="messages-empty" data-testid="messages-empty">
              <strong>No private messages yet</strong>
              <span>
                Chirp is listening for kind:1059 gift-wraps tagged to the active account.
              </span>
            </div>
          }
        >
          <div class="messages-layout">
            <ol class="messages-thread-list" data-testid="messages-thread-list">
              <For each={conversations()}>
                {(conversation) => (
                  <ConversationButton
                    conversation={conversation}
                    selected={selected()?.peerPubkey === conversation.peerPubkey}
                    onSelect={setSelectedPeer}
                  />
                )}
              </For>
            </ol>
            <Show when={selected()}>
              {(conversation) => <ConversationView conversation={conversation()} />}
            </Show>
          </div>
        </Show>
      </Show>

      <Show when={lastCapability()}>
        {(capability) => (
          <p class="messages-diagnostic" role="status" data-testid="messages-diagnostic">
            Recorded diagnostic for <code>{capability()}</code>.
          </p>
        )}
      </Show>
    </section>
  );
}

function ConversationButton(props: {
  conversation: DmConversationItem;
  selected: boolean;
  onSelect: (peer: string) => void;
}) {
  const last = () => props.conversation.messages[props.conversation.messages.length - 1];
  return (
    <li>
      <button
        type="button"
        class="messages-thread-button"
        data-testid="messages-thread"
        data-selected={props.selected ? "true" : "false"}
        onClick={() => props.onSelect(props.conversation.peerPubkey)}
      >
        <strong title={props.conversation.peerPubkey}>{shortHex(props.conversation.peerPubkey)}</strong>
        <span>{last()?.content ?? "No decrypted messages"}</span>
      </button>
    </li>
  );
}

function ConversationView(props: { conversation: DmConversationItem }) {
  return (
    <section class="messages-conversation" data-testid="messages-conversation">
      <div class="messages-conversation-head">
        <div>
          <p class="panel-kicker">Conversation</p>
          <h3 title={props.conversation.peerPubkey}>{shortHex(props.conversation.peerPubkey)}</h3>
        </div>
        <span>{props.conversation.messages.length} messages</span>
      </div>
      <ol class="messages-log">
        <For each={props.conversation.messages}>
          {(message) => <MessageBubble message={message} />}
        </For>
      </ol>
      <div class="messages-compose-blocked" data-testid="messages-compose-blocked">
        <strong>Sending is blocked on web</strong>
        <span>
          The inbox is live; outbound NIP-17 send waits for the browser runtime to wire
          Rust signer and recipient relay capabilities into protocol expansion.
        </span>
      </div>
    </section>
  );
}

function MessageBubble(props: { message: DmMessageItem }) {
  const relays = () =>
    props.message.sourceRelays.map((relay) => relay.replace(/^wss?:\/\//, "")).join(", ") ||
    "source pending";
  return (
    <li
      class="messages-bubble"
      data-testid="messages-message"
      data-outgoing={props.message.isOutgoing ? "true" : "false"}
    >
      <p>{props.message.content}</p>
      <div class="messages-meta">
        <span title={props.message.senderPubkey}>{shortHex(props.message.senderPubkey)}</span>
        <span>{formatTime(props.message.createdAt)}</span>
        <span title={props.message.sourceRelays.join(", ")}>{relays()}</span>
        <Show when={props.message.replyTo}>
          {(replyTo) => <span title={replyTo()}>reply {shortHex(replyTo())}</span>}
        </Show>
      </div>
    </li>
  );
}

function shortHex(value: string): string {
  return value.length > 14 ? `${value.slice(0, 8)}...${value.slice(-4)}` : value;
}

function formatTime(createdAt: number): string {
  if (createdAt <= 0) return "unknown time";
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(createdAt * 1000));
}
