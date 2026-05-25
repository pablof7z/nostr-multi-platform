import { createMemo } from "solid-js";
import { highlightSwift } from "./highlight";

type Props = {
  /** Source body to render. Set to `null` to render a placeholder. */
  source: string | null;
  /** File path shown above the code block. */
  filePath?: string;
  /** Language hint — currently only `swift` is highlighted. */
  language?: "swift" | "shell" | "text";
};

/**
 * A single tab's content: a file-path header + a highlighted <pre>.
 */
export default function CodeBlock(props: Props) {
  const html = createMemo(() => {
    if (props.source === null) return null;
    if (props.language === "swift" || props.language === undefined) {
      return highlightSwift(props.source);
    }
    return props.source
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  });

  return (
    <>
      {props.filePath ? <div class="tabs__path">{props.filePath}</div> : null}
      {props.source === null ? (
        <pre class="code code--placeholder">
          {"// This component is being built — check back soon."}
        </pre>
      ) : (
        <pre class="code">
          <code innerHTML={html() ?? ""} />
        </pre>
      )}
    </>
  );
}
