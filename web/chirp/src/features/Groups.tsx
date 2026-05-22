import { For, createSignal } from "solid-js";
import { Hash, MessageSquare, Plus } from "lucide-solid";
import type { GroupSummary } from "../chirp/model";
import { Topbar } from "./Topbar";

export function GroupsView(props: {
  groups: GroupSummary[];
  onJoin: (id: string) => void;
  onOpen: (id: string) => void;
  onSend: (id: string, text: string) => void;
}) {
  const [message, setMessage] = createSignal("");
  const firstGroup = () => props.groups[0];

  return (
    <>
      <Topbar eyebrow="Groups" title="NIP-29 and Marmot groups" />
      <section class="stack-list">
        <For each={props.groups}>
          {(group) => (
            <article class="group-row">
              <div>
                <h2>
                  <Hash size={17} />
                  {group.name}
                </h2>
                <p>{group.description}</p>
                <small>{group.memberCount} members on {group.host}</small>
              </div>
              <div class="row-actions">
                <button type="button" onClick={() => props.onOpen(group.id)}>
                  <MessageSquare size={16} />
                  Open
                </button>
                <button type="button" onClick={() => props.onJoin(group.id)} disabled={group.joined}>
                  <Plus size={16} />
                  {group.joined ? "Joined" : "Join"}
                </button>
              </div>
            </article>
          )}
        </For>
      </section>
      <section class="message-box">
        <textarea
          value={message()}
          placeholder="Message the selected group"
          onInput={(event) => setMessage(event.currentTarget.value)}
        />
        <button
          type="button"
          disabled={message().trim().length === 0 || !firstGroup()}
          onClick={() => firstGroup() && props.onSend(firstGroup().id, message())}
        >
          <MessageSquare size={17} />
          Send group message
        </button>
      </section>
    </>
  );
}
