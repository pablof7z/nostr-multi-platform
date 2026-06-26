# Product Spec: Chirp Web

Chirp Web is the browser reference client for NMP. It must demonstrate that the
WASM worker runtime can read, render, sign, publish, and diagnose real Nostr
traffic without moving protocol policy into TypeScript.

## First-Run Contract

A new browser profile must open into a usable product surface, not a dead demo.
The first screen must show relay-backed feed state, relay health, signer state,
compose affordances, and diagnostics for publish/action outcomes.

Users without a browser extension must still have a complete write path:
Chirp Web supports a memory-only local-key session by accepting an `nsec` and
handing it directly to `nmp-browser-runtime` as `set_identity kind=local_key`.
Rust decodes the secret, derives the pubkey, registers the signer, and owns all
signing. TypeScript may read the form value only to send that request; it must
not decode, derive from, cache, or sign with the secret.

NIP-07 remains the preferred browser-extension path. NIP-46 is not a web
onboarding path until the browser runtime wires a bunker signer end to end.

## Profile Publish Contract

Chirp Web must let a signed-in user publish their public identity from the first
product session. The browser shell may collect profile fields such as display
name, about text, and picture URL, but it must send them through the typed
profile publish command. Event construction, signing, outbox routing, relay
selection, and acceptance diagnostics remain owned by Rust and the browser
runtime.

Publishing profile metadata must surface the same proof path as note publishing:
the outbox shows the in-flight action, action results show the runtime verdict,
and relay diagnostics show per-relay acceptance or failure. Local validation must
assert that a fixture relay receives a signed kind:0 event with the requested
metadata.

## Secret Storage

Pasted `nsec` values are session-memory only. Chirp Web must not persist them to
localStorage, sessionStorage, IndexedDB, OPFS, snapshots, action history, debug
logs, or URL state. Reloading the page requires the user to paste the key again
unless a future secure-storage decision changes this spec.

All user-visible and diagnostic outputs must be log-safe: redacted request debug,
action stages, action results, and publish outbox projections must never include
the raw secret.
