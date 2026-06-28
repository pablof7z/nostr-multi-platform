import { createSignal, Show } from "solid-js";
import { publishProfileCommand } from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";
import "./profile.css";

const FIELD_LIMIT = 240;

function clean(value: string): string {
  return value.trim();
}

export function ProfileEditor(props: { canPublish: boolean }) {
  const { client } = useNmpClient();
  const [name, setName] = createSignal("");
  const [about, setAbout] = createSignal("");
  const [picture, setPicture] = createSignal("");
  const [publishing, setPublishing] = createSignal(false);
  const [message, setMessage] = createSignal<string | null>(null);

  const hasValue = () =>
    clean(name()).length > 0 || clean(about()).length > 0 || clean(picture()).length > 0;
  const canSubmit = () => props.canPublish && hasValue() && !publishing();

  const publish = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!canSubmit()) return;
    setPublishing(true);
    setMessage(null);
    try {
      await client.dispatchCommand(
        publishProfileCommand({
          name: clean(name()),
          about: clean(about()),
          picture: clean(picture()),
        }),
      );
      setMessage("Profile update queued. Inspect the outbox for relay verdicts.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setPublishing(false);
    }
  };

  return (
    <section id="profile" class="profile-editor" aria-label="Profile editor">
      <div class="profile-editor-header">
        <div>
          <p class="panel-kicker">Profile</p>
          <h2>Publish metadata</h2>
        </div>
        <span data-enabled={props.canPublish ? "true" : "false"}>
          {props.canPublish ? "Signer ready" : "Sign in"}
        </span>
      </div>

      <form class="profile-editor-form" onSubmit={publish}>
        <label>
          <span>Name</span>
          <input
            data-testid="profile-name-input"
            maxLength={FIELD_LIMIT}
            value={name()}
            onInput={(event) => setName(event.currentTarget.value)}
            placeholder="Display name"
            disabled={!props.canPublish || publishing()}
          />
        </label>
        <label>
          <span>About</span>
          <textarea
            data-testid="profile-about-input"
            maxLength={FIELD_LIMIT}
            rows={3}
            value={about()}
            onInput={(event) => setAbout(event.currentTarget.value)}
            placeholder="Short bio"
            disabled={!props.canPublish || publishing()}
          />
        </label>
        <label>
          <span>Picture URL</span>
          <input
            data-testid="profile-picture-input"
            maxLength={FIELD_LIMIT}
            value={picture()}
            onInput={(event) => setPicture(event.currentTarget.value)}
            placeholder="https://..."
            disabled={!props.canPublish || publishing()}
          />
        </label>

        <div class="profile-editor-footer">
          <span>{hasValue() ? "kind:0 metadata" : "Add at least one field"}</span>
          <button data-testid="profile-publish-submit" type="submit" disabled={!canSubmit()}>
            {publishing() ? "Publishing..." : "Publish profile"}
          </button>
        </div>
      </form>

      <Show when={message()}>
        {(value) => (
          <p class="profile-editor-message" role="status">
            {value()}
          </p>
        )}
      </Show>
    </section>
  );
}
