import { createSignal } from "solid-js";

type Props = {
  command: string;
};

export default function InstallCommand(props: Props) {
  const [copied, setCopied] = createSignal(false);

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(props.command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard API can fail in insecure contexts; silently no-op.
      // The text remains selectable for manual copy.
    }
  };

  return (
    <div class="install" role="group" aria-label="Install command">
      <code class="install__code">{props.command}</code>
      <button
        type="button"
        class="install__copy"
        onClick={onCopy}
        aria-label={copied() ? "Copied" : "Copy install command"}
      >
        {copied() ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
