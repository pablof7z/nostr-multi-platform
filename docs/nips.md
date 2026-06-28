# NIP Support Matrix

> **Reviewed:** 2026-06-28. This is a durable support matrix, not a tactical
> roadmap. Active work remains in GitHub Issues.

The browser signer/private-flow capability model is single-sourced in
[`docs/wasm-surface.md`](wasm-surface.md#browser-signerprivate-flow-capability-model).
Rows below point there instead of restating browser signer policy.

| NIP | Scope in NMP | Browser signer/private-flow note |
|---|---|---|
| NIP-07 | Browser extension signing bridge through `window.nostr`; `Nip07Signer` caches pubkey and delegates signing through the browser capability path. | NIP-44 is supported only when the extension exposes both optional `window.nostr.nip44.encrypt` and `window.nostr.nip44.decrypt`. Sign-event support alone is not enough for NIP-17 private flows. See #2247 / PR #2249. |
| NIP-17 | Private direct messages via `nmp-nip17` conversation/inbox and `nmp.nip17.send` action. | Browser local-key and NIP-46 sessions can send when normal DM relay-list inputs are available. NIP-07 sessions depend on the extension's optional NIP-44 verbs. Missing signer capability fails visibly; TypeScript must not decrypt, encrypt, or route private messages. |
| NIP-44 | Encryption primitive reached through signer-owned capability seams. | Local-key browser sessions run inline Rust crypto. Browser NIP-46 sessions use pending provider routing for `nip44_encrypt` / `nip44_decrypt` (#2195 / PR #2248). NIP-07 uses `window.nostr.nip44` only from the signer implementation (#2247 / PR #2249). |
| NIP-46 | Nostr Connect / bunker signing and encryption verbs through Rust-owned signer/runtime transport. | Browser runtime installs `kind = "nip46"` signers from `bunker_uri`, parks pending signer operations, and resumes from provider events. Remote signer rejection or unsupported verbs are honest runtime failures. |
| NIP-55 | Android external signer capability. | Not a browser runtime capability. Browser docs must not treat NIP-55 as a web fallback. |
| NIP-59 | Gift-wrap envelope used by private-message flows. | Browser shell never constructs gift wraps; `nmp-nip17` and the runtime own construction, signing, and relay targeting. |
