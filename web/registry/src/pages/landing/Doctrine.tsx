import { JSX } from "solid-js";

type DoctrineItem = { code: string; name: string; desc: string };

const policy: DoctrineItem[] = [
  {
    code: "D0",
    name: "No app nouns in the kernel",
    desc: "Product vocabulary stays out. Protocol and app concepts extend the kernel through typed traits. Two apps built on NMP cannot poison each other through it.",
  },
  {
    code: "D1",
    name: "Best-effort rendering",
    desc: "Render now, refine in place. Display fields are never optional. Placeholders are part of the type contract. No spinner ever gates content that's already renderable.",
  },
  {
    code: "D2",
    name: "Negentropy is the backfill",
    desc: "NIP-77 reconciliation is how history loads. REQ is the live tail, not a history mechanism. You stop paginating.",
  },
  {
    code: "D3",
    name: "Outbox routing by default",
    desc: "NIP-65 picks the relays. The default is correct. Manual selection is the audited opt-out, not the starting point.",
  },
  {
    code: "D4",
    name: "Single writer per fact",
    desc: "One source of truth per piece of state. Everything else is derived. There is no public cache-invalidation API because there is nothing to invalidate.",
  },
  {
    code: "D5",
    name: "Bounded snapshots",
    desc: "Snapshots include only what's open. The full event store never crosses FFI. Memory cost scales with what's visible, not what exists.",
  },
  {
    code: "D10",
    name: "Provenance",
    desc: "Private events never escape to public relays. The type system carries provenance; the relay manager refuses to publish across that boundary.",
  },
];

const substrate: DoctrineItem[] = [
  {
    code: "D6",
    name: "No exceptions across FFI",
    desc: "Errors surface as state fields. Swift and Kotlin never wrap dispatch() in try/catch — there is nothing to catch.",
  },
  {
    code: "D7",
    name: "Capabilities report, never decide",
    desc: "Keychain, biometrics, push — OS capabilities report state to the kernel. The kernel decides policy. Policy is never written in two languages.",
  },
  {
    code: "D8",
    name: "Reactivity contract",
    desc: "Composite reverse index. 60 Hz per view, max. Working set is bounded. The UI thread is never the bottleneck.",
  },
  {
    code: "D9",
    name: "The kernel owns time",
    desc: "Signing timestamps, replaceable-event resolution, NIP-40 expiration. Wall clock is the kernel's concern, not your view's.",
  },
];

function DoctrineTable(props: { items: DoctrineItem[] }): JSX.Element {
  return (
    <table class="doctrine-table">
      <thead>
        <tr>
          <th></th>
          <th>Doctrine</th>
          <th>What it means for you</th>
        </tr>
      </thead>
      <tbody>
        {props.items.map((d) => (
          <tr>
            <td>{d.code}</td>
            <td>{d.name}</td>
            <td>{d.desc}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export default function Doctrine() {
  return (
    <section class="l-section">
      <p class="l-section__label">Principles</p>
      <h2 class="l-section__heading">Eleven decisions, already made</h2>
      <p class="l-section__lead">
        These are constraints, not suggestions. Every API answers to at least one. They don't
        have exceptions — that's what makes them useful.
      </p>
      <div class="doctrine-blocks">
        <div>
          <p class="doctrine-group__name">Policy — what the framework promises</p>
          <DoctrineTable items={policy} />
        </div>
        <div>
          <p class="doctrine-group__name">Substrate — how the runtime is built</p>
          <DoctrineTable items={substrate} />
        </div>
      </div>
    </section>
  );
}
