// Composer.tsx — note compose + publish box for Chirp Web.
//
// Zero Nostr protocol logic: the `publish_note` action is lowered by
// `chirpActionRequest` via `GeneratedActionBuilders.publishRaw` in actions.ts.
// No event JSON construction, no signing, no relay framing in TS — all that lives
// behind the wasm seam. This component is pure presentation + UX around a textarea.

import { createSignal } from "solid-js";
import { useNmpClient } from "../../nmp/context";

const MAX_CHARS = 280;

export function Composer() {
  const { client } = useNmpClient();
  const [text, setText] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const charsLeft = () => MAX_CHARS - text().length;
  const canSubmit = () => text().trim().length > 0 && !submitting() && charsLeft() >= 0;

  const handleSubmit = async () => {
    const content = text().trim();
    if (!content || submitting()) return;
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
    <div
      class="composer"
      style={{
        padding: "12px 16px",
        "border-bottom": "2px solid rgba(0,0,0,0.1)",
        background: "#fafafa",
      }}
    >
      <textarea
        class="composer-textarea"
        placeholder="What's happening?"
        value={text()}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={handleKeyDown}
        disabled={submitting()}
        maxLength={MAX_CHARS}
        rows={3}
        style={{
          width: "100%",
          "box-sizing": "border-box",
          border: "1px solid #ddd",
          "border-radius": "8px",
          padding: "10px",
          "font-size": "0.95rem",
          resize: "vertical",
          "min-height": "72px",
          "background-color": submitting() ? "#f5f5f5" : "white",
          color: "#222",
          outline: "none",
        }}
      />
      <div
        style={{
          display: "flex",
          "justify-content": "space-between",
          "align-items": "center",
          "margin-top": "8px",
        }}
      >
        <span
          class="composer-chars"
          style={{
            "font-size": "0.8rem",
            color: charsLeft() < 20 ? (charsLeft() < 0 ? "#e53935" : "#f57c00") : "#999",
          }}
        >
          {charsLeft()}
        </span>
        <button
          class="composer-submit"
          disabled={!canSubmit()}
          onClick={() => void handleSubmit()}
          style={{
            background: canSubmit() ? "#7c3aed" : "#c4b5fd",
            color: "white",
            border: "none",
            "border-radius": "20px",
            padding: "7px 18px",
            "font-size": "0.9rem",
            "font-weight": "600",
            cursor: canSubmit() ? "pointer" : "not-allowed",
            transition: "background 0.15s",
          }}
        >
          {submitting() ? "Posting…" : "Post"}
        </button>
      </div>
    </div>
  );
}
