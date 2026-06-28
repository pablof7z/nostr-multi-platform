import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import { hydrateDmPeerRelayListCommand, sendDmCommand } from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";
import {
  decodeDmInboxFrame,
  type DmConversationItem,
  type DmMessageItem,
} from "../../nmp/dmInboxDecoder";
import {
  decodeRuntimeProjection,
  type ActionResultRuntimeRow,
} from "../../nmp/runtimeProjection";
import "./messages.css";

export function MessagesPanel() {
  const { client, snapshot } = useNmpClient();
  const [selectedPeer, setSelectedPeer] = createSignal<string | null>(null);
  const hydratedPeers = new Set<string>();
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

  createEffect(() => {
    const peer = selected()?.peerPubkey;
    if (!peer || hydratedPeers.has(peer)) return;
    hydratedPeers.add(peer);
    void client.dispatchCommand(hydrateDmPeerRelayListCommand(peer));
  });

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
        <span data-state="live">Rust-owned send</span>
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
              <DmSendForm showRecipient />
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
      <DmSendForm recipientPubkey={props.conversation.peerPubkey} />
    </section>
  );
}

function DmSendForm(props: { recipientPubkey?: string; showRecipient?: boolean }) {
  const { client, snapshot } = useNmpClient();
  const [recipient, setRecipient] = createSignal(props.recipientPubkey ?? "");
  const [content, setContent] = createSignal("");
  const [status, setStatus] = createSignal<"idle" | "sending" | "accepted" | "failed">("idle");
  const [message, setMessage] = createSignal("Rust builds, encrypts, wraps, and routes the DM.");
  const [pendingCorrelation, setPendingCorrelation] = createSignal<string | null>(null);
  const [recentResults, setRecentResults] = createSignal<ActionResultRuntimeRow[]>([]);
  const runtime = createMemo(() => decodeRuntimeProjection(snapshot().latestUpdateBytes));

  const target = () => (props.recipientPubkey ?? recipient()).trim();
  const trimmedContent = () => content().trim();
  const canSend = () => status() !== "sending" && target().length > 0 && trimmedContent().length > 0;

  createEffect(() => {
    const rows = runtime()?.actionResults ?? [];
    if (rows.length === 0) return;
    setRecentResults((previous) => mergeActionResults(previous, rows));
  });

  createEffect(() => {
    const correlation = pendingCorrelation();
    if (!correlation) return;
    const result = recentResults().find((row) => row.correlationId === correlation);
    if (!result) return;
    if (result.status === "published") {
      setContent("");
      setStatus("accepted");
      setMessage("Published through Rust-owned NIP-17 gift-wrap routing.");
      setPendingCorrelation(null);
      return;
    }
    if (result.status === "failed") {
      setStatus("failed");
      setMessage(result.error ?? "Rust rejected the DM before publish.");
      setPendingCorrelation(null);
    }
  });

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!canSend()) return;
    setStatus("sending");
    setMessage("Sending through nmp.nip17.send; waiting for relay verdict.");
    try {
      const afterDispatch = await client.dispatchCommand(sendDmCommand(target(), trimmedContent()));
      const accepted = afterDispatch.events.find(
        (item) => item.type === "action_accepted" && item.action_type === "nmp.nip17.send",
      );
      const failure = afterDispatch.events.find(
        (item) => item.type === "capability_failure" && item.capability === "nmp.nip17.send",
      );
      if (accepted?.type === "action_accepted") {
        setPendingCorrelation(accepted.correlation_id);
        setMessage("Rust accepted the DM command; waiting for relay verdict.");
      } else if (failure?.type === "capability_failure") {
        setStatus("failed");
        setMessage(failure.reason);
      } else {
        setStatus("failed");
        setMessage("Runtime did not acknowledge the DM command.");
      }
    } catch (error) {
      setStatus("failed");
      setMessage(error instanceof Error ? error.message : "Send failed.");
    }
  };

  return (
    <form class="messages-compose" data-testid="messages-compose" onSubmit={submit}>
      <Show when={props.showRecipient}>
        <label>
          <span>Recipient pubkey</span>
          <input
            data-testid="messages-recipient-input"
            autocomplete="off"
            spellcheck={false}
            value={recipient()}
            onInput={(event) => setRecipient(event.currentTarget.value)}
          />
        </label>
      </Show>
      <label>
        <span>Message</span>
        <textarea
          data-testid="messages-content-input"
          rows={3}
          value={content()}
          onInput={(event) => setContent(event.currentTarget.value)}
        />
      </label>
      <div class="messages-compose-actions">
        <p data-state={status()} data-testid="messages-send-status">
          {message()}
        </p>
        <button type="submit" data-testid="messages-send-button" disabled={!canSend()}>
          Send
        </button>
      </div>
    </form>
  );
}

function mergeActionResults(
  previous: ActionResultRuntimeRow[],
  incoming: readonly ActionResultRuntimeRow[],
): ActionResultRuntimeRow[] {
  const seen = new Set<string>();
  return [...incoming, ...previous]
    .filter((row) => {
      const key = [
        row.correlationId,
        row.status,
        row.eventId ?? "",
        row.error ?? "",
        row.result ?? "",
      ].join(":");
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, 16);
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
