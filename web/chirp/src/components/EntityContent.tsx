import { For } from "solid-js";
import { AtSign, FileText, KeyRound, Link, Radio, ShieldAlert } from "lucide-solid";
import { shortEntity, tokenizeContent, type Bech32EntityType } from "../chirp/content";

export function EntityContent(props: { content: string; onOpen: (entity: string) => void }) {
  return (
    <p class="entity-content">
      <For each={tokenizeContent(props.content)}>
        {(token) =>
          token.type === "text" ? (
            token.value
          ) : (
            <button
              class={`entity-token entity-${token.entityType}`}
              type="button"
              onClick={() => props.onOpen(token.value)}
              title={token.value}
            >
              <EntityIcon type={token.entityType} />
              {shortEntity(token.value)}
            </button>
          )
        }
      </For>
    </p>
  );
}

function EntityIcon(props: { type: Bech32EntityType }) {
  switch (props.type) {
    case "profile":
      return <AtSign size={14} />;
    case "note":
    case "event":
      return <FileText size={14} />;
    case "address":
      return <Link size={14} />;
    case "relay":
      return <Radio size={14} />;
    case "secret":
      return <ShieldAlert size={14} />;
    case "unknown":
      return <KeyRound size={14} />;
  }
}
