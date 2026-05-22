# Opus direction review #71 — outside-advisor read

**Posture.** I was asked to review the project as an external technical advisor with no exposure to prior reviews. I read `docs/aim.md`, the workspace `Cargo.toml`, the `nmp-core` public surface, the substrate traits, the kernel/actor module tree, the action registry, `apps/chirp/nmp-app-chirp/src/ffi.rs`, `ios/Chirp/Chirp/Bridge/KernelBridge.swift`, the entire `crates/nmp-codegen/src/` tree (Swift emitter, manifest, generator), `crates/nmp-wasm/src/runtime.rs` end-to-end, `crates/nmp-desktop/src/main.rs`, and a sampling of NIP crates. The verdict here is grounded in those files, not in the review history.

The TL;DR is at the bottom. The body justifies it.

---

## 1. The lede: the web target is a Potemkin village

`docs/aim.md` §1 opens with *"a single Rust core consumed identically by iOS (SwiftUI), Android (Jetpack Compose), desktop (iced or Tauri), and web (wasm)."* That is the framing claim of the whole project.

It is not true on web.

`crates/nmp-wasm/src/runtime.rs:11-294` is a completely separate state machine. It has hardcoded local notes in a `Vec<LocalNote>`, a hardcoded author pubkey `"browser-local"` (line 215), no relay socket, no signer, no kernel actor, no `nmp_core::ActorCommand` use, no `spawn_actor`/`run_actor` call anywhere in the crate (verified with grep — zero hits). Line 190 says the quiet part: *"the browser wasm facade accepts publish-note intents; live relay-backed actions require the full actor driver."* That's the WASM crate's author admitting the actor isn't there.

Worse, the web *protocol* (`web/chirp/src/nmp/protocol.ts:54-58`) ships a bespoke `ChirpAction` enum hardcoded with `publish_note | react | follow | unfollow`. The browser doesn't dispatch through the generic `nmp.publish` / `chirp.react` namespace at all — it dispatches through a parallel ad-hoc envelope that the WASM stub then partially routes. This means the central seam of the project (`dispatch_action`) is bypassed on web by design.

This single fact invalidates the project's framing claim more thoroughly than any other finding in this report. Every other complaint below — KernelBridge.swift bloat, codegen gap, FFI sprawl — becomes a downstream effect of this one: if iOS is the only platform where the kernel actually runs, "multi-platform" is aspirational, not delivered.

## 2. Aim doc §4.1–4.14 cross-checked

The aim doc names 14 capabilities the framework promises. Built vs missing, code-grounded:

| §   | Capability                       | Status                                                                                                                                                  |
| --- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 4.1 | Reactive EventStore              | Built (`crates/nmp-core/src/store/` — 6,275 LoC)                                                                                                        |
| 4.2 | Reactive derived views           | Partial — observer fan-out exists, but `ViewModule`/`ViewRegistry` was deleted (`substrate/mod.rs:16-25`). Views are static per-NIP-crate types.        |
| 4.3 | Action-based writes              | Built — `ActionRegistry` + 11 registered modules. Real path, real `correlation_id`, real terminal verdicts.                                             |
| 4.4 | Outbox routing / NIP-65          | Built — `nmp-nip65` ActionModule + actor `maybe_publish_relay_list_after_edit`.                                                                         |
| 4.5 | Subscription planner             | Built — `crates/nmp-core/src/planner/` (4,689 LoC).                                                                                                     |
| 4.6 | Multi-account sessions           | Built — `IdentityRuntime`, `SwitchActive` ActorCommand.                                                                                                 |
| 4.7 | Web of Trust                     | **Missing.** No `nmp-wot` crate. Zero code.                                                                                                             |
| 4.8 | NIP-77 negentropy sync           | **Stub.** `kernel/status.rs:50` is `nip77_negentropy: "unknown"`; `nip77_probe_state` is a String field. No sync API, no driver.                        |
| 4.9 | Unified wallet                   | Partial — NIP-47 NWC + NIP-57 zap path exist (`actor/commands/wallet.rs`, `zap_lnurl.rs`). Cashu (NIP-60) and nutzaps (NIP-61) absent.                  |
| 4.10| Messaging                        | Built — `nmp-nip17` + actor `commands/dm.rs` with `gift_wrap_with_signer` for bunker.                                                                   |
| 4.11| Blossom media                    | **Missing.** No `nmp-blossom` crate. Zero code.                                                                                                         |
| 4.12| Developer guardrails             | **Missing.** No `nmp-guardrails` crate. Zero code.                                                                                                      |
| 4.13| Testing harness                  | Built — `nmp-testing`.                                                                                                                                  |
| 4.14| Scaffolding CLI                  | **Inadequate.** `nmp init` exists (`crates/nmp-cli/src/init.rs`, 152 LoC) but `nmp-codegen` emits only a *Rust fixture app crate* (`generate.rs:19-29`). No iOS/Android/Web project scaffold. The aim says "builds and runs on all four platforms immediately." It does not. |

7 of 14 promised capabilities are either missing or stubbed. The actually-shipped 7 are a Nostr client engine, not a multi-platform framework.

## 3. Architectural decisions that deserve to stay

These are sound — keep them.

- **Single-actor invariant** (`crates/nmp-core/src/actor/mod.rs`). Real, enforced, 55 `ActorCommand` variants funnel through one thread. The hand-rolled relay transport (ADR-0022) was the right call given the actor model — `nostr-sdk`'s tokio-async pool would have fought the actor at every step.
- **`ActionModule` + `register_action::<M>()` typed seam** (ADR-0027, `apps/chirp/nmp-app-chirp/src/ffi.rs:579-738`). One namespace, one validator, one executor, one trait. 11 live modules. This is the project's strongest abstraction and the only place where the *generic* multi-platform thesis is on solid ground.
- **Snapshot-projection registry** (`register_snapshot_projection` keyed by string namespace, e.g. `"nmp.nip17.dm_inbox"`, `"nmp.nip29.group_chat"`, `"chirp.follow_list"`). Per-app projection bolting works exactly like the aim doc wanted §4.2 to work.
- **D11 lint** (no `extern "C" fn nmp_app_*` body may build `PublishUnsignedEvent` directly). Documented; enforced. Doctrine made structural.
- **NIP-44 signer seal seam** (ADR-0026, `crates/nmp-core/src/actor/commands/dm.rs:279` — `gift_wrap_with_signer`). Bunker DMs actually work now.

## 4. Where it's fragile or wrong

**4a. The shell is not thin and codegen is unwired.** `KernelBridge.swift` is 1,988 LoC; the file declares ~33 hand-written `Decodable` structs (`KernelUpdate`, `SnapshotProjections`, `DmConversation`, `DmInboxSnapshot`, `GroupChatMessage`, `ZapsAggregateSnapshot`, `DiscoveredGroupsSnapshot`, `RelayDiagnosticsRow`, …). Meanwhile `crates/nmp-codegen/src/swift.rs:61` ships a working `emit_codable()` function with byte-exact tests (lines 138-160). It is wired to nothing. The Rust→Swift schema is being maintained by hand, in two places, for the foreseeable future. Every new projection means new hand-typed `Decodable` boilerplate.

**4b. C-ABI sprawl.** 72+ unique `nmp_*` symbols (`grep -rE '^pub extern "C" fn nmp_(app|marmot|broker|signer)_[a-z_]+' crates/ apps/`). The dispatch_action seam was sold as the elimination of per-verb FFI; it has succeeded for *some* verbs (`chirp.react/follow/unfollow`, `nmp.nip17.send`, `nmp.nip57.zap`, `nmp.nip65.publish_relay_list`, NIP-29 cluster) but the surface still carries `nmp_app_open_author`, `nmp_app_open_thread`, `nmp_app_open_firehose_tag`, `nmp_app_claim_profile`, `nmp_app_release_profile`, `nmp_app_close_author`, `nmp_app_close_thread`, the entire wallet cluster, the Marmot cluster (8 symbols), the signer-broker cluster, plus 7 Chirp-specific registration entry points. Every new platform pays the linker tax twice (`extern "C"` declaration on the Swift/Kotlin side, FFI body on the Rust side). With no Swift/Kotlin codegen wired, this scales linearly with feature count.

**4c. nmp-core is the god module.** RMP bible commandment #8 says split at ~1,000 lines. `nmp-core` is **75,792 LoC** of one crate. `crates/nmp-core/src/actor/mod.rs` alone is 1,321 lines; `actor/dispatch.rs` is 1,453; `ffi/mod.rs` is 1,411; `actor/relay_mgmt.rs` is 808. The bible is being violated by the project that quotes it as non-negotiable. A second app crate (or even a second protocol crate that needs to peek at kernel internals) will hit dependency walls here.

**4d. Dead/incoherent NIP crates.** `nmp-nip23` (long-form articles) has decoder + builder + tests but zero consumer in apps, ios, web, or other crates. It is a dead island. `nmp-nip22` (kind:1111 comments) has the same shape — protocol crate, no caller. Both should be deleted or feature-gated.

**4e. Web shell ships a divergent protocol.** Even if WASM gets a real actor, the `web/chirp/src/nmp/protocol.ts:54-58` `ChirpAction` enum is a bespoke envelope, not `dispatch_action(namespace, payload)`. The web shell will need a non-trivial refactor before it consumes the same seam iOS does.

**4f. Marmot is the silent third path.** `nmp_marmot_dispatch` (`apps/chirp/nmp-app-chirp/src/marmot/ffi.rs:384`) parses an op-envelope JSON and dispatches MLS operations through `nmp_marmot::projection::ops::dispatch`. This is a parallel action-dispatch system bypassing the kernel's `ActionRegistry`. ADR-0025 attempts to justify it, but as a structural matter, every future feature with non-trivial state will be tempted to fork its own dispatch the same way.

## 5. The single most important direction change

**Stop calling NMP multi-platform and prove the core thesis on one non-iOS target before any further iOS work.**

The honest framing as of 2026-05-22 is: NMP is an opinionated Nostr engine for iOS. The web/desktop/Android targets are demos, stubs, and tech debt respectively. Either:

(a) **Commit to a real WASM port now** — actor in a worker, real WebSocket relay, real signer via NIP-07 capability bridge, real `dispatch_action`. Until this is done, every multi-platform claim is unearned. This is also the cleanest path to proving the snapshot+dispatch seam at scale: the browser will exercise it differently from iOS in ways that surface design flaws, and

(b) **drop the framing.** Rebrand to "NMP — a Rust-core Nostr engine for iOS, with experimental web and desktop shells" and stop drawing the four-platform diagram. This is a defensible product position; the current claim is not.

I'd pick (a). It is the only path that retains the project's identity. But (b) is far better than continuing to talk about a four-platform framework whose web target hardcodes `"browser-local"` as the user's pubkey.

## 6. Stop doing this list

1. **Stop hand-writing Swift `Decodable` structs.** `crates/nmp-codegen/src/swift.rs:61` is built and tested. Wire it into a build step that emits one `GeneratedSnapshot.swift` from a Rust source-of-truth manifest. Expected delta: ~1,200 LoC out of `KernelBridge.swift` (33 structs × ~35 LoC).
2. **Stop treating `crates/nmp-wasm` as evidence the kernel is portable.** It is a stub. Either fix it or remove the "wasm" target from §1 of `docs/aim.md` and from every README claim.
3. **Stop adding new bespoke `nmp_app_*` C symbols.** Cap the FFI surface. Every new feature that wants a C symbol must justify why `dispatch_action(namespace, payload)` won't do. The current 72-symbol surface is what makes a second-app proof structurally impossible.
4. **Stop adding NIP crates with zero in-app consumers.** Delete or feature-gate `nmp-nip23`, `nmp-nip22`. If a future feature needs them, restore in one PR. The cost of carrying dead protocol crates is mostly cognitive but it inflates "look how many NIPs we support" claims that don't translate to user-visible features.
5. **Stop expanding `nmp-core` in-place.** 75,792 LoC in one crate is far past the bible's ~1,000-line split rule. Split out `nmp-core-actor`, `nmp-core-planner`, `nmp-core-store` as sibling crates with `pub(super)`-style visibility. Doing this now is cheap; doing it after a second app crate exists is forced.

---

## Summary (≤400 words)

**The lede.** NMP's central claim — *"a single Rust core consumed identically by iOS, Android, desktop, web"* (`docs/aim.md` §1) — is not delivered. `crates/nmp-wasm/src/runtime.rs:11-294` is a completely separate state machine with hardcoded local notes, no kernel actor (zero hits for `spawn_actor`/`run_actor` in the crate), and a hardcoded `"browser-local"` author pubkey at line 215. The crate itself admits at line 190 that *"live relay-backed actions require the full actor driver."* On top of that, `web/chirp/src/nmp/protocol.ts:54-58` ships a bespoke `ChirpAction` enum that bypasses the generic `dispatch_action` seam by design. The web target is a Potemkin village. As of 2026-05-22 NMP is an opinionated Nostr engine for iOS, not a multi-platform framework.

**The aim doc, audited.** Cross-referenced against the §4.1–4.14 capability list, 7 of 14 promised capabilities are missing or stubbed: Web of Trust (no crate), Blossom (no crate), developer guardrails (no crate), NIP-77 sync API (kernel has a status string and nothing else), Cashu + nutzaps wallet (only NWC + zap-request built), reactive derived views beyond per-NIP types (the `ViewModule`/`ViewRegistry` plan was deleted per `substrate/mod.rs:16-25`), and the scaffolding CLI (which emits only a Rust fixture, not an iOS/Android/Web project).

**The single most important direction change.** Prove the core thesis on one non-iOS target — a real WASM port with the actor in a worker, real WebSocket relays, real signer via capability — before any further iOS investment, OR rebrand to "Nostr engine for iOS with experimental shells." The current framing is unsustainable.

**Top three stop-doing items.** (1) Stop hand-writing Swift `Decodable` structs — the Codable emitter is built and tested at `crates/nmp-codegen/src/swift.rs:61` and wired to nothing; 33 hand-written structs in 1,988-LoC `KernelBridge.swift` will be deleted by wiring it. (2) Stop adding bespoke `nmp_app_*` C symbols — the surface is 72+ unique symbols already and grows linearly with features, defeating the `dispatch_action` thesis. (3) Stop expanding `nmp-core` in place — 75,792 LoC in one crate is two orders of magnitude past the bible's ~1,000-line split rule and will block any second app crate.
