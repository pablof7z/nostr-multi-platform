// Composer.tsx — note compose + publish box for Chirp Web.
//
// Zero Nostr protocol logic: the `publish_note` action is lowered by
// `chirpActionRequest` via generated typed publish builders in actions.ts.
// No event JSON construction, no signing, no relay framing in TS — all that lives
// behind the wasm seam. This component is pure presentation + UX around a textarea.

import { createSignal } from "solid-js";
import { shortHex } from "@nmp/components-web/src/user-avatar/ProfileWire";
import type { FeedRow } from "../../nmp/feedDecoder";
import { quoteRepostCommand } from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";

const MAX_CHARS = 280;

export function Composer(props: {
  canPublish: boolean;
  quoteTarget?: FeedRow | null;
  onCancelQuote?: () => void;
  onQuotePublished?: () => void;
}) {
  const { client } = useNmpClient();
  const [text, setText] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const charsLeft = () => MAX_CHARS - text().length;
  const quoteTarget = () => props.quoteTarget ?? null;
  const quoteAuthor = () => {
    const target = quoteTarget();
    return target?.authorDisplayName || (target ? shortHex(target.authorPubkey) : "");
  };
  const canSubmit = () =>
    props.canPublish && text().trim().length > 0 && !submitting() && charsLeft() >= 0;

  const handleSubmit = async () => {
    const content = text().trim();
    if (!content || !props.canPublish || submitting()) return;
    setSubmitting(true);
    try {
      const target = quoteTarget();
      if (target) {
        await client.dispatchCommand(
          quoteRepostCommand(
            target.id,
            target.kind,
            target.authorPubkey,
            target.relayProvenance[0] ?? null,
            content,
          ),
        );
        props.onQuotePublished?.();
      } else {
        await client.dispatchChirp({ action: "publish_note", content });
      }
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
        <strong>{quoteTarget() ? "Quote post" : "Compose"}</strong>
        <span>{props.canPublish ? "Signer ready" : "Sign in to post"}</span>
      </div>
      {quoteTarget() && (
        <div class="quote-target" data-testid="quote-target">
          <div>
            <strong>{quoteAuthor()}</strong>
            <span>{quoteTarget()!.contentPreview || quoteTarget()!.content}</span>
          </div>
          <button
            type="button"
            aria-label="Cancel quote"
            onClick={() => props.onCancelQuote?.()}
            disabled={submitting()}
          >
            Cancel
          </button>
        </div>
      )}
      <textarea
        class="composer-textarea"
        aria-label="Compose chirp"
        data-testid="compose-input"
        placeholder={
          props.canPublish
            ? quoteTarget()
              ? "Add your take..."
              : "What's happening?"
            : "Read mode - connect a signer to post"
        }
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
          {submitting() ? "Posting..." : quoteTarget() ? "Post quote" : "Post"}
        </button>
      </div>
    </div>
  );
}
