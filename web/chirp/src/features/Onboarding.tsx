import { For, createSignal } from "solid-js";
import { KeyRound, Plus, QrCode, ShieldCheck, Smartphone } from "lucide-solid";
import type { OnboardingMode } from "../chirp/model";

const modes: Array<{
  id: OnboardingMode;
  label: string;
  detail: string;
  placeholder: string;
  Icon: typeof KeyRound;
}> = [
  {
    id: "create",
    label: "Create account",
    detail: "Ask Rust to create the Chirp identity, seed follows, and bootstrap relay rows.",
    placeholder: "Display name",
    Icon: Plus,
  },
  {
    id: "nsec",
    label: "Import nsec",
    detail: "Pass the secret to the browser key capability for Rust-owned account import.",
    placeholder: "nsec1...",
    Icon: KeyRound,
  },
  {
    id: "nip46",
    label: "Connect signer",
    detail: "Use bunker:// or nostrconnect:// with the NMP signer broker.",
    placeholder: "bunker://... or nostrconnect://...",
    Icon: QrCode,
  },
  {
    id: "nip07",
    label: "Browser signer",
    detail: "Use a NIP-07 extension as a web signing capability.",
    placeholder: "extension",
    Icon: Smartphone,
  },
];

export function Onboarding(props: {
  onStart: (mode: OnboardingMode, value: string) => void;
  onNip46: (value: string) => void;
}) {
  const [mode, setMode] = createSignal<OnboardingMode>("nip46");
  const [value, setValue] = createSignal("");
  const active = () => modes.find((item) => item.id === mode()) ?? modes[0];

  const submit = () => {
    if (mode() === "nip46") {
      props.onNip46(value());
    } else {
      props.onStart(mode(), value());
    }
  };

  return (
    <section class="onboarding-panel">
      <div class="onboarding-copy">
        <p class="eyebrow">Sign in</p>
        <h1>Bring your Nostr identity to Chirp Web</h1>
        <p>
          Choose an account path. The browser collects capability input; the runtime owns the
          identity, signer, relay, and follow bootstrap decisions.
        </p>
      </div>
      <div class="mode-grid">
        <For each={modes}>
          {(item) => (
            <button class={mode() === item.id ? "mode-card active" : "mode-card"} type="button" onClick={() => setMode(item.id)}>
              <item.Icon size={20} />
              <span>{item.label}</span>
              <small>{item.detail}</small>
            </button>
          )}
        </For>
      </div>
      <label class="field-row">
        <span>{active().label}</span>
        <input
          value={value()}
          placeholder={active().placeholder}
          onInput={(event) => setValue(event.currentTarget.value)}
        />
      </label>
      <button type="button" onClick={submit}>
        <ShieldCheck size={18} />
        Continue
      </button>
    </section>
  );
}
