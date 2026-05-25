import { createEffect, onCleanup } from "solid-js";

type Props = {
  title: string;
  description?: string;
};

const DEFAULT_TITLE = "NMP Registry — Native Nostr UI components";
const DEFAULT_DESCRIPTION =
  "Native Nostr UI components for SwiftUI and Compose. Copy-paste components you own, customize freely, update without losing edits.";

/**
 * Sets `<title>` and the `<meta name="description">` content for the
 * current route. Restores the defaults on unmount so a stale title
 * doesn't linger after navigation.
 */
export default function PageMeta(props: Props) {
  createEffect(() => {
    document.title = props.title;
    const desc = props.description ?? DEFAULT_DESCRIPTION;
    let el = document.querySelector('meta[name="description"]');
    if (!el) {
      el = document.createElement("meta");
      el.setAttribute("name", "description");
      document.head.appendChild(el);
    }
    el.setAttribute("content", desc);
  });

  onCleanup(() => {
    document.title = DEFAULT_TITLE;
    const el = document.querySelector('meta[name="description"]');
    if (el) el.setAttribute("content", DEFAULT_DESCRIPTION);
  });

  return null;
}
