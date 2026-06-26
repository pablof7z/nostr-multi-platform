// Composer.tsx — note compose + publish box for Chirp Web.
//
// Zero Nostr protocol logic: the `publish_note` action is lowered by
// `chirpActionRequest` via generated typed publish builders in actions.ts.
// No event JSON construction, no signing, no relay framing in TS — all that lives
// behind the wasm seam. This component is pure presentation + UX around a textarea.

import { createSignal } from "solid-js";
import { useNmpClient } from "../../nmp/context";

const MAX_CHARS = 280;

export function Composer(props: { canPublish: boolean }) {
  const { client } = useNmpClient();
  const [text, setText] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const charsLeft = () => MAX_CHARS - text().length;
  const canSubmit = () =>
    props.canPublish && text().trim().length > 0 && !submitting() && charsLeft() >= 0;

  const handleSubmit = async () => {
    const content = text().trim();
    if (!content || !props.canPublish || submitting()) return;
    setSubmitting(true);
    try {
      await client.dispatchChirp({ action: "publish_note", content });
      setText("");
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void handleSubmit();
    }
  };

  return (
    <div class="composer" data-can-publish={props.canPublish ? "true" : "false"}>
      <div class="composer-header">
        <strong>Compose</strong>
        <span>{props.canPublish ? "Signer ready" : "Sign in to post"}</span>
      </div>
      <textarea
        class="composer-textarea"
        aria-label="Compose chirp"
        data-testid="compose-input"
        placeholder={props.canPublish ? "What's happening?" : "Read mode - connect a signer to post"}
        value={text()}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={handleKeyDown}
        disabled={submitting() || !props.canPublish}
        maxLength={MAX_CHARS}
        rows={3}
      />
      <div class="composer-footer">
        <span class="composer-chars" data-tight={charsLeft() < 20 ? "true" : "false"}>
          {charsLeft()}
        </span>
        <button
          class="composer-submit"
          disabled={!canSubmit()}
          onClick={() => void handleSubmit()}
        >
          {submitting() ? "Posting…" : "Post"}
        </button>
      </div>
    </div>
  );
}
