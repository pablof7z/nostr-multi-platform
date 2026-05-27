export default function HowItWorks() {
  return (
    <section class="l-section">
      <p class="l-section__label">Architecture</p>
      <h2 class="l-section__heading">The architecture, in plain terms</h2>
      <div class="how-body">
        <p>
          NMP is one Rust core and four platform shells: SwiftUI on iOS, Compose on Android,
          iced on desktop, wasm in the browser. The Rust core owns everything that touches the
          protocol — state, relays, signing, subscriptions, encryption, replaceable-event
          resolution, time. The platform code does exactly two things: render state, and execute
          OS capabilities like Keychain access or push notifications.{" "}
          <strong>The division is absolute.</strong> That's not a guideline. That's the framework.
        </p>
        <pre class="arch-diagram">{`iOS (SwiftUI) · Android (Compose) · Desktop (iced) · Web (wasm)
        ↓  dispatch(action)          reconcile(update)  ↑
┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
          Rust kernel — one actor thread, no locks
  relays · signing · subscriptions · state · projections`}</pre>
        <p>
          Cross-platform Nostr clients fragment into incompatible bugs because protocol logic
          gets reimplemented per platform. Three times. Badly. NMP writes it once, in Rust, with
          tests. The Swift code can't get NIP-17 wrong because it never sees NIP-17. The Kotlin
          code can't mis-route a post because it never picks a relay. The browser code can't leak
          a signing operation because it doesn't hold a key.
        </p>
        <p>
          The kernel is The Elm Architecture, ported to Rust and pinned to a single actor thread.
          One <code class="inline-code">AppState</code>, one set of actions, one pure update
          function. Platform code calls <code class="inline-code">dispatch(action)</code> — fire
          and forget. Never blocks, never returns a result, never throws. State arrives back as
          <code class="inline-code">reconcile(update)</code> callbacks. The shell hops to its UI
          thread and renders. That's the whole contract.
        </p>
        <p>
          One actor thread means no data races, no concurrent mutation, no locks in app code.
          Snapshots are bounded by what's open — a closed view costs nothing. Reactivity is a
          composite reverse index that recomputes only the projections that changed, capped at
          60 Hz per view.
        </p>
      </div>
    </section>
  );
}
