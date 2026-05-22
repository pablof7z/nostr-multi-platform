import { RefreshCw } from "lucide-solid";

export function Topbar(props: {
  eyebrow: string;
  title: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <header class="topbar">
      <div>
        <p class="eyebrow">{props.eyebrow}</p>
        <h1>{props.title}</h1>
      </div>
      {props.onAction && (
        <button class="icon-button" type="button" aria-label={props.actionLabel} onClick={props.onAction}>
          <RefreshCw size={18} />
        </button>
      )}
    </header>
  );
}
