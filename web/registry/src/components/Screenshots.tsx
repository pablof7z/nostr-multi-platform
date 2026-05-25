import { For, Show, createSignal } from "solid-js";

type Props = {
  componentId: string;
  /** Variant filenames under `/screenshots/`. */
  variants: string[];
};

/**
 * Renders a row of screenshot tiles. Tiles fall back to a dashed
 * placeholder when the image fails to load (404 from `/screenshots/...`).
 */
export default function Screenshots(props: Props) {
  return (
    <div class="screenshots" role="list">
      <Show
        when={props.variants.length > 0}
        fallback={<PlaceholderTile id={props.componentId} />}
      >
        <For each={props.variants}>
          {(variant) => (
            <ScreenshotTile id={props.componentId} src={`/screenshots/${variant}`} />
          )}
        </For>
      </Show>
    </div>
  );
}

function ScreenshotTile(props: { id: string; src: string }) {
  const [failed, setFailed] = createSignal(false);

  return (
    <div class="screenshot" role="listitem">
      {failed() ? (
        <PlaceholderTile id={props.id} />
      ) : (
        <img
          src={props.src}
          alt={`${props.id} preview`}
          loading="lazy"
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}

function PlaceholderTile(props: { id: string }) {
  return (
    <div class="screenshot screenshot--placeholder" role="listitem">
      <div>
        Screenshot — build and run <code>NmpGallery</code> to generate
        <br />
        <span style="opacity: 0.6">({props.id})</span>
      </div>
    </div>
  );
}
