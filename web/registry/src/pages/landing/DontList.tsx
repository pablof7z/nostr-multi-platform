const items = [
  {
    title: "You don't pick relays per operation.",
    desc: "NIP-65 outbox routing is on by default. Posts go where they should. Reads come from where they live. Manual relay selection is the opt-out, not the default.",
  },
  {
    title: "You don't handle stale replaceable events.",
    desc: "Kind-0, kind-3, and parameterized-replaceable events supersede their predecessors on insert. The store will not let you hold a stale version. There is no version to drift to.",
  },
  {
    title: "You don't write subscription cleanup.",
    desc: "Subscriptions auto-close, auto-coalesce, and auto-dedup. When a view goes away, so do its subscriptions. You don't track this. You can't forget to.",
  },
  {
    title: 'You don\'t write "fetch then update state" sequences.',
    desc: "Reads flow through the store. Writes flow through actions. There is no step three. The shape of the API forbids it.",
  },
  {
    title: "You don't handle errors at the FFI boundary.",
    desc: "dispatch() is fire-and-forget, always. No try/catch in Swift. No exceptions in Kotlin. Failures surface as state fields, the same way successes do.",
  },
  {
    title: "You don't invalidate caches.",
    desc: "There are no caches. There are derived views that recompute. Cache invalidation isn't solved — it's not a concept.",
  },
  {
    title: "You don't decrypt DMs in Swift or Kotlin.",
    desc: "NIP-17 decryption happens in Rust, behind the FFI. Plaintext never leaves the kernel except as projection data going to a specific view.",
  },
  {
    title: "You don't write relay reconnection logic.",
    desc: "Reconnect, backoff, and replay are the relay manager's job. You won't find a retry loop in your app code because there isn't one to write.",
  },
  {
    title: "You don't track which subscription belongs to which view.",
    desc: "The framework does. When a view closes, its subscriptions close with it.",
  },
  {
    title: "You don't think about follow-list auto-tracking.",
    desc: "When the active account's kind:3 changes, every open subscription that depends on it recompiles on the wire. Zero app code. Zero hooks. It just happens.",
  },
];

export default function DontList() {
  return (
    <section class="l-section">
      <p class="l-section__label">Responsibility transfer</p>
      <h2 class="l-section__heading">The work you stop doing the day you adopt NMP</h2>
      <p class="l-section__lead">
        Most Nostr clients are 80% protocol plumbing and 20% product. NMP inverts that.
        The framework owns the plumbing. You own the product.
      </p>
      <ul class="dont-list">
        {items.map((item) => (
          <li class="dont-list__item">
            <span class="dont-list__title">{item.title}</span>
            <span class="dont-list__desc">{item.desc}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
