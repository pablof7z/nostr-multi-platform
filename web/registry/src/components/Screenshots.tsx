import { For, Show, createSignal } from "solid-js";
import { Platform, PLATFORM_LABELS } from "../registry";

type Props = {
  componentId: string;
  variants: string[];
  platform: Platform;
};

export default function Screenshots(props: Props) {
  return (
    <div class="screenshots" role="list">
      <Show
        when={props.variants.length > 0}
        fallback={<PlaceholderTile id={props.componentId} platform={props.platform} />}
      >
        <For each={props.variants}>
          {(variant) => (
            props.platform === "tui" ? (
              <TerminalMockup
                id={props.componentId}
                src={`/screenshots/${variant}`}
                platform={props.platform}
              />
            ) : (
              <DeviceMockup
                id={props.componentId}
                src={`/screenshots/${variant}`}
                platform={props.platform}
              />
            )
          )}
        </For>
      </Show>
    </div>
  );
}

function DeviceMockup(props: { id: string; src: string; platform: Platform }) {
  const [failed, setFailed] = createSignal(false);

  return (
    <div class="device-mockup" role="listitem">
      <div class="device-mockup__island" />
      <div class="device-mockup__screen">
        {failed() ? (
          <PlaceholderTile id={props.id} platform={props.platform} />
        ) : (
          <img
            src={props.src}
            alt={`${props.id} ${PLATFORM_LABELS[props.platform]} preview`}
            loading="lazy"
            onError={() => setFailed(true)}
          />
        )}
      </div>
      <div class="device-mockup__home" />
    </div>
  );
}

function TerminalMockup(props: { id: string; src: string; platform: Platform }) {
  const [failed, setFailed] = createSignal(false);

  return (
    <div class="terminal-mockup" role="listitem">
      <div class="terminal-mockup__chrome">
        <span />
        <span />
        <span />
      </div>
      <div class="terminal-mockup__screen">
        {failed() ? (
          <PlaceholderTile id={props.id} platform={props.platform} />
        ) : (
          <img
            src={props.src}
            alt={`${props.id} ${PLATFORM_LABELS[props.platform]} preview`}
            loading="lazy"
            onError={() => setFailed(true)}
          />
        )}
      </div>
    </div>
  );
}

function PlaceholderTile(props: { id: string; platform: Platform }) {
  return (
    <div class="device-mockup__placeholder">
      <div>
        No screenshot yet for <strong>{PLATFORM_LABELS[props.platform]}</strong>
        <br />
        Build and run <code>NmpGallery</code> to generate
        <br />
        <span style="opacity: 0.5; font-size: 0.75em">
          {props.id} · {props.platform}
        </span>
      </div>
    </div>
  );
}
