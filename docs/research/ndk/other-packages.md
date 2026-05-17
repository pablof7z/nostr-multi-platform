# NDK Package Family (April 2026 snapshot)

Repository: `/Users/pablofernandez/Work/NDK-nhlteu`.

## Core graph

```
core ─┬─ outbox ─┐
      ├─ subscription
      ├─ relay
      ├─ signers (nip07, nip46, private-key)
      ├─ events (+ wrap, kinds)
      ├─ user (follows, profile, nip05)
      ├─ zap / zapper
      ├─ cache (filter)
      └─ ai-guardrails
```

## sessions

Path: `sessions/src/`. See `wot-and-sessions.md`. Depends on `@nostr-dev-kit/ndk` (peer) + zustand. Storage adapters in `storage/` (local/file/memory). Persistence model: only identity persists; data re-hydrates from cache.

## react

Path: `react/src/`. Subdirs: `ndk/`, `session/`, `subscribe/`, `profiles/`, `mutes/`, `wallet/`, `observer/`. Hooks: `useNDK`, `useNDKCurrentPubkey`, `useNDKSessions`, `useFollows`, `useSubscribe`, `useProfileValue`, `useMuteFilter`. Re-exports much of `@nostr-dev-kit/ndk`. Session store has its own zustand store mirroring sessions, with `updateSession`, `add-monitor`, `start-session` files (single-responsibility split).

## svelte

Path: `svelte/src/lib/`. Svelte 5 runes throughout. Surface:
- `NDKSvelte` class — wraps NDK with reactive getters (`$currentUser`, `$currentSession`, `$follows`, `$mutes`).
- Builders in `builders/`: `subscription.svelte.ts`, `fetch-event.svelte.ts`, `fetch-events.svelte.ts`, `user.svelte.ts`, `wot.svelte.ts`, `zap-subscription.svelte.ts`, `meta-subscription.svelte.ts`, `blossom-url.svelte.ts`, `blossom-upload.svelte.ts`, `relay-info.svelte.ts`.
- Stores: `sessions.svelte.ts`, `follows.svelte.ts`, `mutes.svelte.ts`, `wot.svelte.ts`, `wallet.test.ts` (with companion ts).

Also includes `svelte/registry/` — a jsrepo-managed showcase / component library (vendored cache and NDK deps in node_modules).

## mobile

Path: `mobile/src/`. Re-exports `@nostr-dev-kit/react` plus:
- `cache-adapter/sqlite/` — Expo SQLite backed cache.
- `signers/nip55.ts` — Android Amber-style external signer (intent-based).
- `session-monitor.ts` — wraps `useNDKSessionMonitor` with `NDKSessionExpoSecureStore`.
- `session-storage-adapter.ts` — Expo SecureStore adapter + `migrateLegacyLogin`.
- `mint/`, `stores/wallet.ts`, `components/`, `hooks/`.

## wallet

Path: `wallet/src/`. NIP-60 (Cashu) wallet implementation. Subdirs: `wallets/cashu/`, `wallets/nwc/`, `wallets/webln/`, `nip87/mint-store.ts`, `nutzap-monitor/`, `utils/ln.ts`. Uses `@cashu/cashu-ts v3`. Uses `NDKSync` for relay capability caching (fix `7287bf5e`). DEFERRED to post-v1 per `docs/plan/scope-adjustments-2026-05-18.md`.

## wot

Path: `wot/src/`. Depends on `@nostr-dev-kit/sync` for negentropy-based contact list batch fetching. ~330 LOC. See `wot-and-sessions.md`.

## sync

Path: `sync/src/`. NIP-77 Negentropy implementation. Exports `NDKSync` class (use this) and lower-level `sync()` / `syncAndSubscribe()` static helpers. Files: `ndk-sync-class.ts`, `ndk-sync.ts`, `sync-subscribe.ts`, `negentropy/`, `relay/`, `__tests__/`, `utils/`. Tracks per-relay support; auto-falls-back to `fetchEvents()` when relay rejects NEG-OPEN (`3407126e`).

## blossom

Path: `blossom/src/`. NIP-96 / Blossom blob storage. Subdirs: `upload/`, `healing/url-healing.ts`, `utils/auth.ts`, `utils/sha256.ts`, `types/`. Default export `NDKBlossom`. Handles UTF-8 filename auth headers (fix `85c7eb92`).

## blossom-cli

Path: `blossom-cli/`. Command-line companion to blossom — not pulled into core dist.

## messages

Path: `messages/src/`. Multi-protocol DM stack:
- `NDKMessenger` — orchestrator. Requires `ndk.signer`. Auto-detects cache adapter and upgrades from `MemoryAdapter` → `CacheModuleStorage` on `start()`.
- `protocols/nip17.ts` — gift-wrapped DMs.
- `conversation.ts` — per-conversation event tracker.
- `cache-module.ts` — registers a module with NDK cache for message persistence.
- `storage/` — memory and cache-module adapters.

DEFERRED to post-v1 per `docs/plan/scope-adjustments-2026-05-18.md`.

## cache-* packages

- `cache-memory` — in-memory; testing baseline.
- `cache-browser` — IndexedDB browser cache; pulls in `cache-dexie` + `cache-sqlite-wasm` as deps.
- `cache-dexie` — Dexie-based IndexedDB (browser).
- `cache-sqlite` — better-sqlite3, native Node.
- `cache-sqlite-wasm` — SQLite via WASM (browser worker), with degraded-mode guards (fix `81effa63`) and worker init guards.
- `cache-redis` — Redis backend.
- `cache-upstash` — Upstash Redis HTTP API.
- `cache-nostr` — peer-cache via Nostr relays.

All implement the `NDKCacheAdapter` interface in `core/src/cache/`. Cache-side concerns: relay-provenance tracking, filter unconstrain (defaults `["limit", "since", "until"]`).

## docs

Path: `docs/`. VitePress documentation site. `prepare-docs.sh` helper.

## ai-guardrails

Inside `core/src/ai-guardrails/`. Runtime checks for common LLM/dev mistakes. Off by default. Configurable per check via `aiGuardrails: { skip: Set<string> }` or `ndk.guardrailOff(...)`.

## What NMP should mirror vs skip

| NDK package | NMP relevance |
|---|---|
| core | Required — port concepts. |
| sessions | Required — session model is direct port. |
| sync | Required for M4 negentropy. |
| wot | Required for M13. |
| react/svelte/mobile | NMP's per-platform shims play the same role but native (SwiftUI/Compose). |
| cache-* family | NMP's M3 LMDB chooses ONE strategy per platform; the cache-* multiplicity is a JS-ecosystem artifact. |
| blossom | Required for M10. |
| messages | Deferred. Reference for post-v1 NIP-17 work. |
| wallet | Deferred. Reference for post-v1 wallet work. |
| ai-guardrails | Worth porting as `nmp-guardrails`. |
