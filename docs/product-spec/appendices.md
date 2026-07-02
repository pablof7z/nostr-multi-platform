# Product Spec: Appendices

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## Appendix A. Binding Architecture In Detail

The current runtime contract is:

- native public app bindings through UniFFI;
- browser public bindings through `nmp-browser-runtime` wasm-bindgen exports;
- binary `UpdateFrame` push callbacks for state;
- typed projection payloads for host-rendered views;
- ADR-0071 dispatch envelopes for writes;
- no generic JSON snapshot payload and no platform polling for app data.

`SnapshotEnvelope` is bounded by what is open. It carries screen-shaped state and
open projection payloads, never the whole event store or signer internals. Hosts
apply only frames whose `rev` advances their current shadow state.

If future profiling proves that bulk scrolling needs a different transport, the
decision belongs in a fresh ADR.

## Appendix B. Glossary Of NIPs Referenced

| NIP | Purpose | Where it appears |
|---|---|---|
| 01 | Base protocol, replaceable events | §7.1 |
| 05 | DNS-based identifiers | §7.6 |
| 07 | Browser signer | §7.4 |
| 09 | Deletion events | §7.1 |
| 17 | Private DMs | §7.10 |
| 19 | bech32 entities | §7.12 |
| 22 | Comments | comments/read-model docs and ADR-0070 |
| 23 | Long-form content | content rendering and feeds |
| 25 | Reactions | §6.3, §7.6 |
| 29 | Relay-based groups | NIP-29 crate/docs |
| 40 | Expiration | §7.1 |
| 42 | Auth | §6.4 |
| 44 | Encryption | §7.10 |
| 46 | Nostr Connect / bunker | §7.4 |
| 47 | Wallet Connect | §7.9 |
| 49 | Encrypted private key | §7.4 |
| 55 | Android external signer | §7.4 |
| 57 | Lightning zaps | §7.9 |
| 59 | Gift wrap | §7.10 |
| 60 | Cashu wallets | §7.9 |
| 61 | Nutzaps | §7.9 |
| 65 | Relay-list metadata | §7.3 |
| 77 | Negentropy | §7.8 |
